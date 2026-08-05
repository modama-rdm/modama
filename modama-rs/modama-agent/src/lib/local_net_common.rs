use std::net::Ipv4Addr;

use modama_common::agent::{AgentId, FromURIError};
use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;
use url::Url;

pub const NOTIFY_HEADER: &str = "MODAMA-AGENT-NOTIFY";
pub const NOTIFY_CURRENT_VERSION: u32 = 1u32;
pub const NOTIFY_PORT: u16 = 5055;
pub const NOTIFY_MCAST_ADDR: Ipv4Addr = const { Ipv4Addr::new(239, 192, 0, 1) };

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryInner {
    pub version: u32,
    pub api: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DiscoveryInnerValidated {
    pub version: u32,
    pub api: Url,
}

impl DiscoveryInner {
    pub fn validate(self) -> Option<DiscoveryInnerValidated> {
        let url = Url::try_from(self.api.as_str()).ok()?;
        Some(DiscoveryInnerValidated { version: self.version, api: url })
    }
}

impl DiscoveryInnerValidated {
    pub fn serialize(&self) -> String {
        let inner = DiscoveryInner {
            version: self.version,
            api: self.api.as_str().to_owned(),
        };
        #[allow(clippy::unwrap_used)] // infalliable
        serde_json::to_string(&inner).unwrap()
    }
}

// TODO: move to modama-common
#[derive(Debug, PartialEq, Eq)]
pub struct DiscoveryMessage {
    pub inner: DiscoveryInnerValidated,
}

impl DiscoveryMessage {
    /// Serialize the discovery message to a String.
    pub fn serialize(&self) -> String {
        let payload = self.inner.serialize();
        format!("{NOTIFY_HEADER}\n{payload}",)
    }

    pub fn api_url(&self) -> &Url {
        &self.inner.api
    }

    /// Try to parse from the string representation to a `DiscoveryMessage`,
    /// returning None if this is not possible.
    pub fn try_parse(value: &str) -> Option<Self> {
        if !value.contains("\n") {
            return None;
        }
        let mut parts = value.split("\n");
        let header = parts.next()?;
        if header != NOTIFY_HEADER {
            return None;
        }
        let payload = parts.next()?;
        let inner: DiscoveryInner = serde_json::from_str(payload).ok()?;
        Some(Self {
            inner: inner.validate()?,
        })
    }
}

pub async fn create_mcast_socket(
    port: u16,
    group: Ipv4Addr,
    local: Ipv4Addr,
) -> Result<UdpSocket, std::io::Error> {
    let group_w_port = format!("{group}:{port}", group = group, port = port);
    let socket = UdpSocket::bind(&group_w_port).await?;
    socket.join_multicast_v4(group, local)?;
    Ok(socket)
}

pub async fn create_mcast_socket_for_sending(
    group: Ipv4Addr,
    local: Ipv4Addr,
) -> Result<UdpSocket, std::io::Error> {
    let socket = UdpSocket::bind((local, 0)).await?;
    socket.join_multicast_v4(group, local)?;
    Ok(socket)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LocalNetTokenResponse {
    pub token: String,
    pub agent_name_uri: String,
}

impl LocalNetTokenResponse {
    pub fn agent_id(&self) -> Result<AgentId, FromURIError> {
        AgentId::from_uri(&self.agent_name_uri)
    }
}
