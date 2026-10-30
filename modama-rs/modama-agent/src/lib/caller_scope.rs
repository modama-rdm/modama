//! Determine which helper scope a local client belongs to, from the loopback TCP connection
//! itself. Clients never state their scope; see the session helper design doc.

use std::net::SocketAddr;

use axum::{extract::connect_info::Connected, serve::IncomingStream};
use tokio::net::TcpListener;

/// The unit within which mounts are shared and exactly one session helper runs.
/// Agent-internal, never on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HelperScope {
    /// Linux: all logins of one user share their GVfs mounts
    Uid(u32),

    /// Windows: one fixed scope, until we resolve sessions via the socket's owning process
    WindowsFixed,
}

/// Both ends of an accepted connection, as seen from the agent
#[derive(Debug, Clone, Copy)]
pub struct ConnInfo {
    /// Our end; None if the OS couldn't tell us
    pub local: Option<SocketAddr>,
    pub peer: SocketAddr,
}

impl Connected<IncomingStream<'_, TcpListener>> for ConnInfo {
    fn connect_info(stream: IncomingStream<'_, TcpListener>) -> Self {
        Self {
            local: stream.io().local_addr().ok(),
            peer: *stream.remote_addr(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ScopeError {
    #[error(
        "could not determine which user this connection belongs to; if a proxy is configured, \
         bypass it for localhost"
    )]
    NotFound,

    #[error("could not read the socket table: {0}")]
    SocketTable(String),
}

pub trait ResolveScope: Send + Sync {
    fn resolve(&self, conn: &ConnInfo) -> Result<HelperScope, ScopeError>;
}

/// Resolve scopes using the mechanism of the current platform
#[derive(Debug, Default)]
pub struct PlatformScopeResolver;

#[cfg(target_os = "linux")]
impl ResolveScope for PlatformScopeResolver {
    fn resolve(&self, conn: &ConnInfo) -> Result<HelperScope, ScopeError> {
        let local = conn.local.ok_or(ScopeError::NotFound)?;
        let entries = procfs::net::tcp()
            .and_then(|mut v4| {
                v4.extend(procfs::net::tcp6()?);
                Ok(v4)
            })
            .map_err(|e| ScopeError::SocketTable(e.to_string()))?;
        uid_of_client(&entries, local, conn.peer).map(HelperScope::Uid)
    }
}

#[cfg(windows)]
impl ResolveScope for PlatformScopeResolver {
    fn resolve(&self, _conn: &ConnInfo) -> Result<HelperScope, ScopeError> {
        Ok(HelperScope::WindowsFixed)
    }
}

/// Find the *client's* socket: its local address is our peer, and its remote address is our end.
/// (Our own accepted socket is owned by us, so its uid would be ours.) Addresses are compared
/// canonicalized, as IPv4 clients show up as IPv4-mapped IPv6 addresses on dual-stack listeners.
#[cfg(target_os = "linux")]
fn uid_of_client(
    entries: &[procfs::net::TcpNetEntry],
    local: SocketAddr,
    peer: SocketAddr,
) -> Result<u32, ScopeError> {
    let canonical = |addr: SocketAddr| (addr.ip().to_canonical(), addr.port());
    entries
        .iter()
        .find(|entry| {
            canonical(entry.local_address) == canonical(peer)
                && canonical(entry.remote_address) == canonical(local)
        })
        .map(|entry| entry.uid)
        .ok_or(ScopeError::NotFound)
}

/// Hands out scopes in connection order; tests connect one client at a time.
#[cfg(test)]
#[derive(Debug, Default)]
pub struct QueueScopeResolver {
    scopes: std::sync::Mutex<std::collections::VecDeque<HelperScope>>,
}

#[cfg(test)]
impl QueueScopeResolver {
    pub fn push(&self, scope: HelperScope) {
        #[allow(clippy::unwrap_used)]
        self.scopes.lock().unwrap().push_back(scope);
    }
}

#[cfg(test)]
impl ResolveScope for QueueScopeResolver {
    fn resolve(&self, _conn: &ConnInfo) -> Result<HelperScope, ScopeError> {
        #[allow(clippy::unwrap_used)]
        self.scopes
            .lock()
            .unwrap()
            .pop_front()
            .ok_or(ScopeError::NotFound)
    }
}

#[cfg(test)]
#[cfg(target_os = "linux")]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use tokio::net::TcpStream;

    use super::*;

    fn own_uid() -> u32 {
        use std::os::unix::fs::MetadataExt as _;
        std::fs::metadata("/proc/self").unwrap().uid()
    }

    async fn resolve_loopback(ip: std::net::IpAddr) -> Option<Result<HelperScope, ScopeError>> {
        let listener = TcpListener::bind((ip, 0)).await.ok()?;
        let client = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (accepted, peer) = listener.accept().await.unwrap();
        let conn = ConnInfo {
            local: Some(accepted.local_addr().unwrap()),
            peer,
        };
        let result = PlatformScopeResolver.resolve(&conn);
        drop(client);
        Some(result)
    }

    #[tokio::test]
    async fn resolves_own_uid_on_ipv4_loopback() {
        let scope = resolve_loopback(Ipv4Addr::LOCALHOST.into()).await.unwrap();
        assert_eq!(scope.unwrap(), HelperScope::Uid(own_uid()));
    }

    #[tokio::test]
    async fn resolves_own_uid_on_ipv6_loopback() {
        // skip if there is no IPv6 loopback (some containers)
        let Some(scope) = resolve_loopback(Ipv6Addr::LOCALHOST.into()).await else {
            return;
        };
        assert_eq!(scope.unwrap(), HelperScope::Uid(own_uid()));
    }

    #[test]
    fn unknown_connection_is_not_found() {
        let conn = ConnInfo {
            local: Some((Ipv4Addr::LOCALHOST, 1).into()),
            peer: (Ipv4Addr::LOCALHOST, 2).into(),
        };
        assert!(matches!(
            PlatformScopeResolver.resolve(&conn),
            Err(ScopeError::NotFound)
        ));
    }

    #[test]
    fn missing_local_address_is_not_found() {
        let conn = ConnInfo {
            local: None,
            peer: (Ipv4Addr::LOCALHOST, 2).into(),
        };
        assert!(matches!(
            PlatformScopeResolver.resolve(&conn),
            Err(ScopeError::NotFound)
        ));
    }
}
