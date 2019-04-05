/* Handle delivery concerns around incoming notifications.
 * A notification may be discarded because of quota restrictions or may not require
 * encryption because it is privileged.
 *
 * While this leans heavily on data in Storage, the functions are separated out so that
 * Storage is only focused on actual data storage and retrieval.
 */
#![allow(unknown_lints)]
extern crate storage;

use std::Default;

use storage::{Storage, DeliveryRecord};
use push_errors::{Result, Error, ErrorKind};

struct Manager{
    storage: Storage,
}

pub trait DeliveryManager {
    fn new<D: DeliveryManager, S: Storage>(storage: S) -> D;

    // checks and increments quota (if needed)
    fn check_quota(&self, chid: &str) -> Result<bool>;

    // resets quota back to zero.
    fn reset_quota(&self, chid: &str) -> Result<bool>;

    // sets the info for the chid.
    fn set_quota(&self, chid: &str, quota: u64, system: bool);

    // is this a private, high priviledge "system" call?
    fn is_system(&self, chid: &str) -> Result<bool>;

    // send the notification to the recipient.
    fn dispatch(&self, chid: &str, content: Vec<u8>);
}

impl DeliveryManager for Manager{
    fn new<D: DeliveryManager, S: Storage>(storage: S) -> D {
        return Self{
            storage
        }
    }

    fn check_quota(&self, chid: &str) -> Result<bool> {
        Err(ErrorKind::NotImpemented("check_quota"))
    }

    fn reset_quota(&self, chid: &str) -> Result<bool> {
        Err(ErrorKind::NotImpemented("set_quota"))
    }

    fn set_quota(&self, chid: &str, quota: u64, system: bool) -> Result<bool> {
        Err(ErrorKind::NotImpemented("set_quota"))
    }

    fn is_system(&self, chid: &str) -> Result<bool> {
        Ok(self.get_delivery_record(chid)?.is_system)
    }

    fn dispatch(&self, chid: &str, content: Vec<u8>) {
        Err(ErrorKind::NotImplemented("dispatch"))
    }

}

impl Manager {
    fn get_delivery_record(&self, chid: &str) -> Result<Self> {
        Ok(self.storage.get_delivery_record(&chid).unwrap_or_else(|_|
            DeliveryRecord{
                conn: self.storage.db,
                channel_id: chid,
                .. Default::default()
            }
        ))
    }
}

/*
struct Dispatch {
    storage: Storage
}
*/
