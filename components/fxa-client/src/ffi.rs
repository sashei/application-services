/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! This module implement the traits and some types that make the FFI code easier to manage.
//!
//! Note that the FxA FFI is older than the other FFIs in application-services, and has (direct,
//! low-level) bindings that live in the mozilla-mobile/android-components repo. As a result, it's a
//! bit harder to change (anything breaking the ABI requires careful synchronization of updates
//! across two repos), and doesn't follow all the same conventions that are followed by the other
//! FFIs.
//!
//! None of this is that bad in practice, but much of it is not ideal.

use crate::{
    device::{Device, PushSubscription, Type as DeviceType},
    msg_types, send_tab, AccessTokenInfo, AccountEvent, Error, ErrorKind, Profile, ScopedKey,
};
use ffi_support::{
    implement_into_ffi_by_delegation, implement_into_ffi_by_protobuf, ErrorCode, ExternError,
};

pub mod error_codes {
    // Note: -1 and 0 (panic and success) codes are reserved by the ffi-support library

    /// Catch-all error code used for anything that's not a panic or covered by AUTHENTICATION.
    pub const OTHER: i32 = 1;

    /// Used for `ErrorKind::NotMarried`, `ErrorKind::NoCachedTokens`, `ErrorKind::NoScopedKey`
    /// and `ErrorKind::RemoteError`'s where `code == 401`.
    pub const AUTHENTICATION: i32 = 2;

    /// Code for network errors.
    pub const NETWORK: i32 = 3;
}

fn get_code(err: &Error) -> ErrorCode {
    match err.kind() {
        ErrorKind::RemoteError { code: 401, .. }
        | ErrorKind::NotMarried
        | ErrorKind::NoRefreshToken
        | ErrorKind::NoScopedKey(_)
        | ErrorKind::NoCachedToken(_) => {
            log::warn!("Authentication error: {:?}", err);
            ErrorCode::new(error_codes::AUTHENTICATION)
        }
        ErrorKind::RequestError(_) => {
            log::warn!("Network error: {:?}", err);
            ErrorCode::new(error_codes::NETWORK)
        }
        _ => {
            log::warn!("Unexpected error: {:?}", err);
            ErrorCode::new(error_codes::OTHER)
        }
    }
}

impl From<Error> for ExternError {
    fn from(err: Error) -> ExternError {
        ExternError::new_error(get_code(&err), err.to_string())
    }
}

impl From<AccessTokenInfo> for msg_types::AccessTokenInfo {
    fn from(a: AccessTokenInfo) -> Self {
        msg_types::AccessTokenInfo {
            scope: a.scope,
            token: a.token,
            key: a.key.map(|k| k.into()),
            expires_at: a.expires_at,
        }
    }
}

impl From<ScopedKey> for msg_types::ScopedKey {
    fn from(sk: ScopedKey) -> Self {
        msg_types::ScopedKey {
            kty: sk.kty,
            scope: sk.scope,
            k: sk.k,
            kid: sk.kid,
        }
    }
}

impl From<Profile> for msg_types::Profile {
    fn from(p: Profile) -> Self {
        Self {
            avatar: Some(p.avatar),
            avatar_default: Some(p.avatar_default),
            display_name: p.display_name,
            email: Some(p.email),
            uid: Some(p.uid),
        }
    }
}

impl From<Device> for msg_types::Device {
    fn from(d: Device) -> Self {
        Self {
            id: d.common.id,
            display_name: d.common.display_name,
            r#type: Into::<msg_types::device::Type>::into(d.common.device_type) as i32,
            push_subscription: d.common.push_subscription.map(|p| p.into()),
            push_endpoint_expired: d.common.push_endpoint_expired,
            is_current_device: d.is_current_device,
            last_access_time: d.last_access_time,
        }
    }
}

impl From<DeviceType> for msg_types::device::Type {
    fn from(t: DeviceType) -> Self {
        match t {
            DeviceType::Desktop => msg_types::device::Type::Desktop,
            DeviceType::Mobile => msg_types::device::Type::Mobile,
            DeviceType::Unknown => msg_types::device::Type::Unknown,
        }
    }
}

impl From<PushSubscription> for msg_types::device::PushSubscription {
    fn from(p: PushSubscription) -> Self {
        Self {
            endpoint: p.endpoint,
            public_key: p.public_key,
            auth_key: p.auth_key,
        }
    }
}

impl From<AccountEvent> for msg_types::AccountEvent {
    fn from(e: AccountEvent) -> Self {
        match e {
            AccountEvent::TabReceived((device, payload)) => Self {
                r#type: msg_types::account_event::AccountEventType::TabReceived as i32,
                data: Some(msg_types::account_event::Data::TabReceivedData(
                    msg_types::account_event::TabReceivedData {
                        from: device.map(|d| d.into()),
                        entries: payload.entries.into_iter().map(|e| e.into()).collect(),
                    },
                )),
            },
        }
    }
}

impl From<send_tab::TabData> for msg_types::account_event::tab_received_data::TabData {
    fn from(data: send_tab::TabData) -> Self {
        Self {
            title: data.title,
            url: data.url,
        }
    }
}

implement_into_ffi_by_protobuf!(msg_types::Profile);
implement_into_ffi_by_delegation!(Profile, msg_types::Profile);
implement_into_ffi_by_protobuf!(msg_types::AccessTokenInfo);
implement_into_ffi_by_delegation!(AccessTokenInfo, msg_types::AccessTokenInfo);
implement_into_ffi_by_protobuf!(msg_types::Device);
implement_into_ffi_by_delegation!(Device, msg_types::Device);
implement_into_ffi_by_delegation!(AccountEvent, msg_types::AccountEvent);
implement_into_ffi_by_protobuf!(msg_types::AccountEvent);
implement_into_ffi_by_protobuf!(msg_types::Devices);
implement_into_ffi_by_protobuf!(msg_types::AccountEvents);
