/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::{config::Config, errors::*};
use reqwest::{self, header, Client as ReqwestClient, Method, Request, Response, StatusCode};
use serde_derive::*;
use serde_json::json;
use std::collections::HashMap;

#[cfg(feature = "browserid")]
pub(crate) mod browser_id;

pub trait FxAClient {
    fn oauth_token_with_code(
        &self,
        config: &Config,
        code: &str,
        code_verifier: &str,
    ) -> Result<OAuthTokenResponse>;
    fn oauth_token_with_refresh_token(
        &self,
        config: &Config,
        refresh_token: &str,
        scopes: &[&str],
    ) -> Result<OAuthTokenResponse>;
    fn destroy_oauth_token(&self, config: &Config, token: &str) -> Result<()>;
    fn profile(
        &self,
        config: &Config,
        profile_access_token: &str,
        etag: Option<String>,
    ) -> Result<Option<ResponseAndETag<ProfileResponse>>>;
    fn pending_commands(
        &self,
        config: &Config,
        refresh_token: &str,
        index: u64,
        limit: Option<u64>,
    ) -> Result<PendingCommandsResponse>;
    fn invoke_command(
        &self,
        config: &Config,
        refresh_token: &str,
        command: &str,
        target: &str,
        payload: &serde_json::Value,
    ) -> Result<()>;
    fn devices(&self, config: &Config, refresh_token: &str) -> Result<Vec<GetDeviceResponse>>;
    fn update_device(
        &self,
        config: &Config,
        refresh_token: &str,
        update: DeviceUpdateRequest,
    ) -> Result<UpdateDeviceResponse>;
}

pub struct Client;
impl FxAClient for Client {
    fn profile(
        &self,
        config: &Config,
        access_token: &str,
        etag: Option<String>,
    ) -> Result<Option<ResponseAndETag<ProfileResponse>>> {
        let url = config.userinfo_endpoint()?;
        let client = ReqwestClient::new();
        let mut builder = client
            .request(Method::GET, url)
            .header(header::AUTHORIZATION, bearer_token(access_token));
        if let Some(etag) = etag {
            builder = builder.header(header::IF_NONE_MATCH, format!("\"{}\"", etag));
        }
        let request = builder.build()?;
        let mut resp = Self::make_request(request)?;
        if resp.status() == StatusCode::NOT_MODIFIED {
            return Ok(None);
        }
        let etag = resp
            .headers()
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_owned());
        Ok(Some(ResponseAndETag {
            etag,
            response: resp.json()?,
        }))
    }

    fn oauth_token_with_code(
        &self,
        config: &Config,
        code: &str,
        code_verifier: &str,
    ) -> Result<OAuthTokenResponse> {
        let body = json!({
            "code": code,
            "client_id": config.client_id,
            "code_verifier": code_verifier
        });
        self.make_oauth_token_request(config, body)
    }

    fn oauth_token_with_refresh_token(
        &self,
        config: &Config,
        refresh_token: &str,
        scopes: &[&str],
    ) -> Result<OAuthTokenResponse> {
        let body = json!({
            "grant_type": "refresh_token",
            "client_id": config.client_id,
            "refresh_token": refresh_token,
            "scope": scopes.join(" ")
        });
        self.make_oauth_token_request(config, body)
    }

    fn destroy_oauth_token(&self, config: &Config, token: &str) -> Result<()> {
        let body = json!({
            "token": token,
        });
        let url = config.oauth_url_path("v1/destroy")?;
        let client = ReqwestClient::new();
        let request = client
            .request(Method::POST, url)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .build()?;
        Self::make_request(request)?;
        Ok(())
    }

    fn pending_commands(
        &self,
        config: &Config,
        refresh_token: &str,
        index: u64,
        limit: Option<u64>,
    ) -> Result<PendingCommandsResponse> {
        let url = config.auth_url_path("v1/account/device/commands")?;
        let client = ReqwestClient::new();
        let mut builder = client
            .request(Method::GET, url)
            .header(header::AUTHORIZATION, bearer_token(refresh_token))
            .query(&[("index", index)]);
        if let Some(limit) = limit {
            builder = builder.query(&[("limit", limit)])
        }
        let request = builder.build()?;
        Ok(Self::make_request(request)?.json()?)
    }

    fn invoke_command(
        &self,
        config: &Config,
        refresh_token: &str,
        command: &str,
        target: &str,
        payload: &serde_json::Value,
    ) -> Result<()> {
        let body = json!({
            "command": command,
            "target": target,
            "payload": payload
        });
        let url = config.auth_url_path("v1/account/devices/invoke_command")?;
        let client = ReqwestClient::new();
        let request = client
            .request(Method::POST, url)
            .header(header::AUTHORIZATION, bearer_token(refresh_token))
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .build()?;
        Self::make_request(request)?;
        Ok(())
    }

    fn devices(&self, config: &Config, refresh_token: &str) -> Result<Vec<GetDeviceResponse>> {
        let url = config.auth_url_path("v1/account/devices")?;
        let client = ReqwestClient::new();
        let request = client
            .request(Method::GET, url)
            .header(header::AUTHORIZATION, bearer_token(refresh_token))
            .build()?;
        Ok(Self::make_request(request)?.json()?)
    }

    fn update_device(
        &self,
        config: &Config,
        refresh_token: &str,
        update: DeviceUpdateRequest,
    ) -> Result<UpdateDeviceResponse> {
        let url = config.auth_url_path("v1/account/device")?;
        let client = ReqwestClient::new();
        let request = client
            .request(Method::POST, url)
            .header(header::AUTHORIZATION, bearer_token(refresh_token))
            .header(header::CONTENT_TYPE, "application/json")
            .body(serde_json::to_string(&update)?)
            .build()?;
        Ok(Self::make_request(request)?.json()?)
    }
}

impl Client {
    pub fn new() -> Self {
        Self {}
    }

    fn make_oauth_token_request(
        &self,
        config: &Config,
        body: serde_json::Value,
    ) -> Result<OAuthTokenResponse> {
        let url = config.token_endpoint()?;
        let client = ReqwestClient::new();
        let request = client
            .request(Method::POST, url)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .build()?;
        Self::make_request(request)?.json().map_err(|e| e.into())
    }

    fn make_request(request: Request) -> Result<Response> {
        let client = ReqwestClient::new();
        let mut resp = client.execute(request)?;
        let status = resp.status();

        if status.is_success() || status == StatusCode::NOT_MODIFIED {
            Ok(resp)
        } else {
            let json: std::result::Result<serde_json::Value, reqwest::Error> = resp.json();
            match json {
                Ok(json) => Err(ErrorKind::RemoteError {
                    code: json["code"].as_u64().unwrap_or(0),
                    errno: json["errno"].as_u64().unwrap_or(0),
                    error: json["error"].as_str().unwrap_or("").to_string(),
                    message: json["message"].as_str().unwrap_or("").to_string(),
                    info: json["info"].as_str().unwrap_or("").to_string(),
                }
                .into()),
                Err(_) => Err(resp.error_for_status().unwrap_err().into()),
            }
        }
    }
}

fn bearer_token(token: &str) -> String {
    format!("Bearer {}", token)
}

#[derive(Clone)]
pub struct ResponseAndETag<T> {
    pub response: T,
    pub etag: Option<String>,
}

#[derive(Deserialize)]
pub struct PendingCommandsResponse {
    pub index: u64,
    pub last: Option<bool>,
    pub messages: Vec<PendingCommand>,
}

#[derive(Deserialize)]
pub struct PendingCommand {
    pub index: u64,
    pub data: CommandData,
}

#[derive(Debug, Deserialize)]
pub struct CommandData {
    pub command: String,
    pub payload: serde_json::Value, // Need https://github.com/serde-rs/serde/issues/912 to make payload an enum instead.
    pub sender: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct PushSubscription {
    #[serde(rename = "pushCallback")]
    pub endpoint: String,
    #[serde(rename = "pushPublicKey")]
    pub public_key: String,
    #[serde(rename = "pushAuthKey")]
    pub auth_key: String,
}

/// We use the double Option pattern in this struct.
/// The outer option represents the existence of the field
/// and the inner option its value or null.
/// TL;DR:
/// `None`: the field will not be present in the JSON body.
/// `Some(None)`: the field will have a `null` value.
/// `Some(Some(T))`: the field will have the serialized value of T.
#[derive(Serialize)]
#[allow(clippy::option_option)]
pub struct DeviceUpdateRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "name")]
    display_name: Option<Option<&'a str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    device_type: Option<Option<&'a DeviceType>>,
    #[serde(flatten)]
    push_subscription: Option<&'a PushSubscription>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "availableCommands")]
    available_commands: Option<Option<&'a HashMap<String, String>>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum DeviceType {
    #[serde(rename = "desktop")]
    Desktop,
    #[serde(rename = "mobile")]
    Mobile,
    #[serde(other)]
    #[serde(skip_serializing)] // Don't you dare trying.
    Unknown,
}

#[allow(clippy::option_option)]
pub struct DeviceUpdateRequestBuilder<'a> {
    device_type: Option<Option<&'a DeviceType>>,
    display_name: Option<Option<&'a str>>,
    push_subscription: Option<&'a PushSubscription>,
    available_commands: Option<Option<&'a HashMap<String, String>>>,
}

impl<'a> DeviceUpdateRequestBuilder<'a> {
    pub fn new() -> Self {
        Self {
            device_type: None,
            display_name: None,
            push_subscription: None,
            available_commands: None,
        }
    }

    pub fn push_subscription(mut self, push_subscription: &'a PushSubscription) -> Self {
        self.push_subscription = Some(push_subscription);
        self
    }

    pub fn available_commands(mut self, available_commands: &'a HashMap<String, String>) -> Self {
        self.available_commands = Some(Some(available_commands));
        self
    }

    pub fn clear_available_commands(mut self) -> Self {
        self.available_commands = Some(None);
        self
    }

    pub fn display_name(mut self, display_name: &'a str) -> Self {
        self.display_name = Some(Some(display_name));
        self
    }

    pub fn clear_display_name(mut self) -> Self {
        self.display_name = Some(None);
        self
    }

    #[allow(dead_code)]
    pub fn device_type(mut self, device_type: &'a DeviceType) -> Self {
        self.device_type = Some(Some(device_type));
        self
    }

    pub fn build(self) -> DeviceUpdateRequest<'a> {
        DeviceUpdateRequest {
            display_name: self.display_name,
            device_type: self.device_type,
            push_subscription: self.push_subscription,
            available_commands: self.available_commands,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DeviceLocation {
    pub city: Option<String>,
    pub country: Option<String>,
    pub state: Option<String>,
    #[serde(rename = "stateCode")]
    pub state_code: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GetDeviceResponse {
    #[serde(flatten)]
    pub common: DeviceResponseCommon,
    #[serde(rename = "isCurrentDevice")]
    pub is_current_device: bool,
    pub location: DeviceLocation,
    #[serde(rename = "lastAccessTime")]
    pub last_access_time: Option<u64>,
}

impl std::ops::Deref for GetDeviceResponse {
    type Target = DeviceResponseCommon;
    fn deref(&self) -> &DeviceResponseCommon {
        &self.common
    }
}

pub type UpdateDeviceResponse = DeviceResponseCommon;

#[derive(Clone, Serialize, Deserialize)]
pub struct DeviceResponseCommon {
    pub id: String,
    #[serde(rename = "name")]
    pub display_name: String,
    #[serde(rename = "type")]
    pub device_type: DeviceType,
    #[serde(flatten)]
    pub push_subscription: Option<PushSubscription>,
    #[serde(rename = "availableCommands")]
    pub available_commands: HashMap<String, String>,
    #[serde(rename = "pushEndpointExpired")]
    pub push_endpoint_expired: bool,
}

#[derive(Deserialize)]
pub struct OAuthTokenResponse {
    pub keys_jwe: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_in: u64,
    pub scope: String,
    pub access_token: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ProfileResponse {
    pub uid: String,
    pub email: String,
    pub locale: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    pub avatar: String,
    #[serde(rename = "avatarDefault")]
    pub avatar_default: bool,
    #[serde(rename = "amrValues")]
    pub amr_values: Vec<String>,
    #[serde(rename = "twoFactorAuthentication")]
    pub two_factor_authentication: bool,
}
