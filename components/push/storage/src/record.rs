use rusqlite::Row;

use crypto::Key;
use push_errors::Result;

use crate::types::Timestamp;

pub type ChannelID = String;

#[derive(Clone, Debug, PartialEq)]
pub struct MetaRecord {
    /// Meta information are various push related values that need to persist across restarts.
    /// e.g. "UAID", server "auth" token, etc. This table should not be exposed outside of
    /// the push component.
    // User Agent unique identifier
    pub key: String,
    // Server authorization token
    pub val: String,
}

#[derive(Debug)]
pub struct DeliveryRecord {
    // ChannelID / Key
    pub channel_id: ChannelID,

    // Name of the service associated with this channelID,
    pub service_name: String,

    // Is this a User Agent System function, and immune from quota checks?
    pub is_system: bool,

    // Max Quota of notifications before being cut off
    pub quota: Option<u32>,

    // UTC for last message receipt
    pub last_recvd: Option<Timestamp>,

    // Number of notifications received since last reset
    pub recv_count: Option<u32>,

    // UA provided recipient information
    pub recipient_info: Option<String>,
}

impl DeliveryRecord {
    // can't use `impl From` because of tries
    pub fn from_row(row: Row) -> Result<Self> {
        Ok(Self {
            channel_id: row.get_checked("channel_id")?,
            service_name: row.get_checked("svc_name")?,
            is_system: row.get_checked("is_system")?,
            quota: row.get_checked("quota")?,
            last_recvd: row.get_checked("last_recvd")?,
            recv_count: row.get_checked("recv_count")?,
            recipient_info: row.get_checked("recipient_info")?,
        })
    }
}

impl Default for DeliveryRecord {
    fn default() -> Self {
        Self {
            channel_id: "".to_owned(),
            service_name: "".to_owned(),
            is_system: false,
            quota: None,
            last_recvd: None,
            recv_count: None,
            recipient_info: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PushRecord {
    // User Agent's unique identifier
    pub uaid: String,

    // Designation label provided by the subscribing service
    pub channel_id: ChannelID,

    // Endpoint provided from the push server
    pub endpoint: String,

    // The recipient (service worker)'s scope
    pub scope: String,

    // Private EC Prime256v1 key info. (Public key can be derived from this)
    pub key: Vec<u8>,

    // Time this subscription was created.
    pub ctime: Timestamp,

    // VAPID public key to restrict subscription updates for only those that sign
    // using the private VAPID key.
    pub app_server_key: Option<String>,

    // (if this is a bridged connection (e.g. on Android), this is the native OS Push ID)
    pub native_id: Option<String>,
}

impl PushRecord {
    /// Create a Push Record from the Subscription info: endpoint, encryption
    /// keys, etc.
    pub fn new(uaid: &str, chid: &str, endpoint: &str, scope: &str, key: Key) -> Self {
        // XXX: unwrap
        Self {
            uaid: uaid.to_owned(),
            channel_id: chid.to_owned(),
            endpoint: endpoint.to_owned(),
            scope: scope.to_owned(),
            key: key.serialize().unwrap(),
            ctime: Timestamp::now(),
            app_server_key: None,
            native_id: None,
        }
    }

    pub(crate) fn from_row(row: &Row) -> Result<Self> {
        Ok(PushRecord {
            uaid: row.get_checked("uaid")?,
            channel_id: row.get_checked("channel_id")?,
            endpoint: row.get_checked("endpoint")?,
            scope: row.get_checked("scope")?,
            key: row.get_checked("key")?,
            ctime: row.get_checked("ctime")?,
            app_server_key: row.get_checked("app_server_key")?,
            native_id: row.get_checked("native_id")?,
        })
    }
}
