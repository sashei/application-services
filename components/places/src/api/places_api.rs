/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::db::db::PlacesDb;
use crate::error::*;
use crate::history_sync::store::HistoryStore;
use crate::util::normalize_path;
use lazy_static::lazy_static;
use rusqlite::OpenFlags;
use std::cell::Cell;
use std::collections::HashMap;
use std::mem;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex, Weak,
};
use sync15::{telemetry, ClientInfo};

#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ConnectionType {
    ReadOnly = 1,
    ReadWrite = 2,
    Sync = 3,
}

impl ConnectionType {
    pub fn from_primitive(p: u8) -> Option<Self> {
        match p {
            1 => Some(ConnectionType::ReadOnly),
            2 => Some(ConnectionType::ReadWrite),
            3 => Some(ConnectionType::Sync),
            _ => None,
        }
    }
}

impl ConnectionType {
    pub fn rusqlite_flags(self) -> OpenFlags {
        let common_flags = OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI;
        match self {
            ConnectionType::ReadOnly => common_flags | OpenFlags::SQLITE_OPEN_READ_ONLY,
            ConnectionType::ReadWrite => {
                common_flags | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_READ_WRITE
            }
            ConnectionType::Sync => common_flags | OpenFlags::SQLITE_OPEN_READ_WRITE,
        }
    }
}

// We only allow a single PlacesApi per filename.
// XXX - probably need encryption key here too so we can do something sane
// if we attempt to open the same file with different keys.
lazy_static! {
    static ref APIS: Mutex<HashMap<PathBuf, Weak<PlacesApi>>> = Mutex::new(HashMap::new());
}

static ID_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct SyncState {
    client_info: Cell<Option<ClientInfo>>,
}

/// The entry-point to the places API. This object gives access to database
/// connections and other helpers. It enforces that only 1 write connection
/// can exist to the database at once.
pub struct PlacesApi {
    db_name: PathBuf,
    encryption_key: Option<String>,
    write_connection: Mutex<Option<PlacesDb>>,
    sync_state: Mutex<Option<SyncState>>,
    coop_tx_lock: Arc<Mutex<()>>,
    sync_conn_active: AtomicBool,
    id: usize,
}
impl PlacesApi {
    /// Create a new, or fetch an already open, PlacesApi backed by a file on disk.
    pub fn new(db_name: impl AsRef<Path>, encryption_key: Option<&str>) -> Result<Arc<Self>> {
        let db_name = normalize_path(db_name)?;
        Self::new_or_existing(db_name, encryption_key)
    }

    /// Create a new, or fetch an already open, memory-based PlacesApi. You must
    /// provide a name, but you are still able to have a single writer and many
    ///  reader connections to the same memory DB open.
    pub fn new_memory(db_name: &str, encryption_key: Option<&str>) -> Result<Arc<Self>> {
        let name = PathBuf::from(format!("file:{}?mode=memory&cache=shared", db_name));
        Self::new_or_existing(name, encryption_key)
    }

    fn new_or_existing(db_name: PathBuf, encryption_key: Option<&str>) -> Result<Arc<Self>> {
        // XXX - we should check encryption_key via the HashMap here too. Also, we'd
        // rather not keep the key in memory forever, and instead it would be better to
        // require it in open_connection.
        // (Or maybe given these issues (and the surprising performance hit), we shouldn't
        // support encrypted places databases...)
        let mut guard = APIS.lock().unwrap();
        let id = ID_COUNTER.fetch_add(1, Ordering::SeqCst);
        match guard.get(&db_name).and_then(Weak::upgrade) {
            Some(existing) => Ok(existing.clone()),
            None => {
                // We always create a new read-write connection for an initial open so
                // we can create the schema and/or do version upgrades.
                let coop_tx_lock = Arc::new(Mutex::new(()));
                let connection = PlacesDb::open(
                    &db_name,
                    encryption_key,
                    ConnectionType::ReadWrite,
                    id,
                    coop_tx_lock.clone(),
                )?;
                let new = PlacesApi {
                    db_name: db_name.clone(),
                    encryption_key: encryption_key.map(ToString::to_string),
                    write_connection: Mutex::new(Some(connection)),
                    sync_state: Mutex::new(None),
                    sync_conn_active: AtomicBool::new(false),
                    id,
                    coop_tx_lock,
                };
                let arc = Arc::new(new);
                (*guard).insert(db_name, Arc::downgrade(&arc));
                Ok(arc)
            }
        }
    }

    /// Open a connection to the database.
    pub fn open_connection(&self, conn_type: ConnectionType) -> Result<PlacesDb> {
        let ec = self.encryption_key.as_ref().map(String::as_str);
        match conn_type {
            ConnectionType::ReadOnly => {
                // make a new one - we can have as many of these as we want.
                PlacesDb::open(
                    self.db_name.clone(),
                    ec,
                    ConnectionType::ReadOnly,
                    self.id,
                    self.coop_tx_lock.clone(),
                )
            }
            ConnectionType::ReadWrite => {
                // We only allow one of these.
                let mut guard = self.write_connection.lock().unwrap();
                match mem::replace(&mut *guard, None) {
                    None => Err(ErrorKind::ConnectionAlreadyOpen.into()),
                    Some(db) => Ok(db),
                }
            }
            ConnectionType::Sync => {
                panic!("Use `open_sync_connection` to open a sync connection");
            }
        }
    }

    pub fn open_sync_connection(&self) -> Result<SyncConn> {
        let prev_value = self
            .sync_conn_active
            .compare_and_swap(false, true, Ordering::SeqCst);
        if prev_value {
            Err(ErrorKind::ConnectionAlreadyOpen.into())
        } else {
            let ec = self.encryption_key.as_ref().map(String::as_str);
            let db = PlacesDb::open(
                self.db_name.clone(),
                ec,
                ConnectionType::Sync,
                self.id,
                self.coop_tx_lock.clone(),
            )?;
            Ok(SyncConn {
                db,
                flag: &self.sync_conn_active,
            })
        }
    }

    /// Close a connection to the database. If the connection is the write
    /// connection, you can re-fetch it using open_connection.
    pub fn close_connection(&self, connection: PlacesDb) -> Result<()> {
        if connection.api_id() != self.id {
            return Err(ErrorKind::WrongApiForClose.into());
        }
        if connection.conn_type() == ConnectionType::ReadWrite {
            // We only allow one of these.
            let mut guard = self.write_connection.lock().unwrap();
            assert!((*guard).is_none());
            *guard = Some(connection);
        }
        Ok(())
    }

    // TODO: We need a better result here so we can return telemetry.
    // We possibly want more than just a `SyncTelemetryPing` so we can
    // return additional "custom" telemetry if the app wants it.
    pub fn sync(
        &self,
        client_init: &sync15::Sync15StorageClientInit,
        key_bundle: &sync15::KeyBundle,
    ) -> Result<telemetry::SyncTelemetryPing> {
        let mut guard = self.sync_state.lock().unwrap();
        let conn = self.open_sync_connection()?;
        if guard.is_none() {
            *guard = Some(SyncState {
                client_info: Cell::new(None),
            });
        }
        let sync_state = guard.as_ref().unwrap();
        let store = HistoryStore::new(&conn, &sync_state.client_info);
        let mut sync_ping = telemetry::SyncTelemetryPing::new();
        store.sync(&client_init, &key_bundle, &mut sync_ping)?;
        Ok(sync_ping)
    }
}

/// Wrapper around PlacesDb that automatically sets a flag (`sync_conn_active`)
/// to false when finished
pub struct SyncConn<'api> {
    db: PlacesDb,
    flag: &'api AtomicBool,
}

impl<'a> Drop for SyncConn<'a> {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::SeqCst)
    }
}

impl<'a> std::ops::Deref for SyncConn<'a> {
    type Target = PlacesDb;
    fn deref(&self) -> &PlacesDb {
        &self.db
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // A helper for our tests to get their own memory Api.
    static ATOMIC_COUNTER: AtomicUsize = AtomicUsize::new(0);

    pub fn new_mem_api() -> Arc<PlacesApi> {
        let counter = ATOMIC_COUNTER.fetch_add(1, Ordering::Relaxed);
        PlacesApi::new_memory(&format!("test-api-{}", counter), None).expect("should get an API")
    }

    pub fn new_mem_connection() -> PlacesDb {
        new_mem_api()
            .open_connection(ConnectionType::ReadWrite)
            .expect("should get a connection")
    }
}

#[cfg(test)]
mod tests {
    use super::test::*;
    use super::*;
    use sql_support::ConnExt;

    #[test]
    fn test_multi_writers_fails() {
        let api = new_mem_api();
        let writer1 = api
            .open_connection(ConnectionType::ReadWrite)
            .expect("should get writer");
        api.open_connection(ConnectionType::ReadWrite)
            .expect_err("should fail to get second writer");
        // But we should be able to re-get it after closing it.
        api.close_connection(writer1)
            .expect("should be able to close");
        api.open_connection(ConnectionType::ReadWrite)
            .expect("should get a writer after closing the other");
    }

    #[test]
    fn test_shared_memory() {
        let api = new_mem_api();
        let writer = api
            .open_connection(ConnectionType::ReadWrite)
            .expect("should get writer");
        writer
            .execute_batch(
                "CREATE TABLE test_table (test_value INTEGER);
                              INSERT INTO test_table VALUES (999)",
            )
            .expect("should insert");
        let reader = api
            .open_connection(ConnectionType::ReadOnly)
            .expect("should get reader");
        let val = reader
            .query_one::<i64>("SELECT test_value FROM test_table")
            .expect("should get value");
        assert_eq!(val, 999);
    }

    #[test]
    fn test_reader_before_writer() {
        let api = new_mem_api();
        let reader = api
            .open_connection(ConnectionType::ReadOnly)
            .expect("should get reader");
        let writer = api
            .open_connection(ConnectionType::ReadWrite)
            .expect("should get writer");
        writer
            .execute_batch(
                "CREATE TABLE test_table (test_value INTEGER);
                              INSERT INTO test_table VALUES (999)",
            )
            .expect("should insert");
        let val = reader
            .query_one::<i64>("SELECT test_value FROM test_table")
            .expect("should get value");
        assert_eq!(val, 999);
    }

    #[test]
    fn test_wrong_writer_close() {
        let api = new_mem_api();
        // Grab this so `api` doesn't think it still has a writer.
        let _writer = api
            .open_connection(ConnectionType::ReadWrite)
            .expect("should get writer");

        let fake_api = new_mem_api();
        let fake_writer = fake_api
            .open_connection(ConnectionType::ReadWrite)
            .expect("should get writer 2");

        // No PartialEq on ErrorKind, so we abuse match.
        match api.close_connection(fake_writer).unwrap_err().kind() {
            &ErrorKind::WrongApiForClose => {}
            e => panic!("Expected error WrongApiForClose, got {:?}", e),
        }
    }

    #[test]
    fn test_valid_writer_close() {
        let api = new_mem_api();
        let writer = api
            .open_connection(ConnectionType::ReadWrite)
            .expect("should get writer");

        api.close_connection(writer)
            .expect("Should allow closing own connection");

        // Make sure we can open it again.
        assert!(api.open_connection(ConnectionType::ReadWrite).is_ok());
    }
}
