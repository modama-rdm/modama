use std::{
    net::{Ipv4Addr, SocketAddrV4},
    time::Duration,
};

use modama_common::agent::{AgentId, FromURIError};
use network_interface::{Addr, NetworkInterface, NetworkInterfaceConfig};
use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use url::Url;

pub const NOTIFY_HEADER: &str = "MODAMA-AGENT-NOTIFY";
pub const NOTIFY_CURRENT_VERSION: u32 = 1u32;
pub const NOTIFY_PORT: u16 = 5055;
pub const NOTIFY_MCAST_ADDR: Ipv4Addr = const { Ipv4Addr::new(239, 192, 0, 1) };
/// How often a token proxy is announced
pub const ANNOUNCEMENT_INTERVAL: Duration = Duration::from_secs(5);

/// Serializes the tests that announce or listen on loopback, which would hear each other
#[cfg(test)]
pub(crate) static DISCOVERY_MUTEX: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

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
        Some(DiscoveryInnerValidated {
            version: self.version,
            api: url,
        })
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

/// An IPv4 network this computer is directly on: one of its addresses, with that address's
/// netmask
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalNetwork {
    pub addr: Ipv4Addr,
    /// `None` if the system doesn't say; then only `addr` itself is on the network
    pub netmask: Option<Ipv4Addr>,
}

impl LocalNetwork {
    pub fn contains(&self, ip: Ipv4Addr) -> bool {
        let mask = self.netmask.map_or(u32::MAX, u32::from);
        u32::from(ip) & mask == u32::from(self.addr) & mask
    }
}

/// The IPv4 networks of all of this computer's interfaces; an interface with several addresses
/// is on several networks
pub fn local_networks() -> Result<Vec<LocalNetwork>, network_interface::Error> {
    Ok(NetworkInterface::show()?
        .into_iter()
        .flat_map(|interface| interface.addr)
        .filter_map(|addr| match addr {
            Addr::V4(v4) => Some(LocalNetwork {
                addr: v4.ip,
                netmask: v4.netmask,
            }),
            Addr::V6(_) => None,
        })
        .collect())
}

/// A socket that sends discovery announcements on exactly the interface with address `local`.
/// An announcement carries the URL of this interface's token API, so it must not reach any other
/// network: the interface is set explicitly (Windows doesn't derive it from the bound address for
/// IPv4 multicast, Linux only by a special case), and a TTL of 1 keeps it on the local link.
pub fn create_mcast_socket_for_sending(local: Ipv4Addr) -> std::io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_nonblocking(true)?;
    socket.bind(&SocketAddrV4::new(local, 0).into())?;
    socket.set_multicast_if_v4(&local)?;
    socket.set_multicast_ttl_v4(1)?;
    UdpSocket::from_std(socket.into())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sending_socket_is_pinned_to_its_interface_and_the_local_link() {
        let local = Ipv4Addr::LOCALHOST;
        let socket = create_mcast_socket_for_sending(local).unwrap();
        let socket = socket2::SockRef::from(&socket);
        assert_eq!(socket.multicast_if_v4().unwrap(), local);
        assert_eq!(socket.multicast_ttl_v4().unwrap(), 1);
    }

    #[test]
    fn a_network_contains_the_addresses_its_netmask_covers() {
        let network = LocalNetwork {
            addr: Ipv4Addr::new(192, 168, 0, 20),
            netmask: Some(Ipv4Addr::new(255, 255, 255, 0)),
        };
        assert!(network.contains(Ipv4Addr::new(192, 168, 0, 1)));
        assert!(network.contains(Ipv4Addr::new(192, 168, 0, 255)));
        assert!(!network.contains(Ipv4Addr::new(192, 168, 1, 1)));
        assert!(!network.contains(Ipv4Addr::new(10, 0, 0, 1)));
    }

    #[test]
    fn without_a_netmask_only_the_address_itself_is_on_the_network() {
        let network = LocalNetwork {
            addr: Ipv4Addr::new(192, 168, 0, 20),
            netmask: None,
        };
        assert!(network.contains(Ipv4Addr::new(192, 168, 0, 20)));
        assert!(!network.contains(Ipv4Addr::new(192, 168, 0, 21)));
    }
}
