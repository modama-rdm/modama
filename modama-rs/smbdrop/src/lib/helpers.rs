use std::{
    ffi::OsStr,
    fs::File,
    io,
    os::{fd::AsRawFd, unix::ffi::OsStrExt},
    path::{Path, PathBuf},
};

use anyhow::Result;
use modama_common::agent::{AgentId, ClientCertError};
use nix::{errno::Errno, ioctl_read, ioctl_write_ptr};
use tokio::net::TcpStream;
use tokio_rustls::server::TlsStream;

const FS_IMMUTABLE_FL: u64 = 0x010;

#[derive(thiserror::Error, Debug)]
pub enum IOCFlagsError {
    #[error("OS error: {0}")]
    OsError(#[from] Errno),
    #[error("I/O error: {0}")]
    IoError(#[from] io::Error),
}

#[derive(thiserror::Error, Debug)]
pub enum ImmutableError {
    #[error("already immutable")]
    AlreadyImmutable,
    #[error("I/O error: {0}")]
    OsError(#[from] IOCFlagsError),
}

ioctl_read!(fs_ioc_getflags_ioctl, b'f', 1, u64);
pub fn fs_ioc_getflags(path: &Path) -> Result<u64, IOCFlagsError> {
    let f = File::open(path)?;
    let fd = f.as_raw_fd();

    let mut result: u64 = 0;
    let result_ptr: *mut u64 = &mut result;

    unsafe {
        let ret = Errno::result(fs_ioc_getflags_ioctl(fd, result_ptr)?);
        ret?;
    }

    Ok(result)
}

ioctl_write_ptr!(fs_ioc_setflags_ioctl, b'f', 2, u64);
pub fn fs_ioc_setflags(path: &Path, flags: u64) -> Result<(), IOCFlagsError> {
    let f = File::open(path)?;
    let fd = f.as_raw_fd();
    let flags_ptr: *const u64 = &flags;

    unsafe {
        let ret = Errno::result(fs_ioc_setflags_ioctl(fd, flags_ptr)?);
        ret?;
    }

    Ok(())
}

pub fn is_immutable(path: &Path) -> Result<bool, IOCFlagsError> {
    let flags = fs_ioc_getflags(path)?;
    Ok((flags & FS_IMMUTABLE_FL) != 0)
}

pub fn set_immutable(path: &Path, value: bool) -> Result<(), IOCFlagsError> {
    let mut flags: u64 = fs_ioc_getflags(path)?;
    if value {
        flags |= FS_IMMUTABLE_FL;
    } else {
        flags &= !FS_IMMUTABLE_FL;
    }
    fs_ioc_setflags(path, flags)?;

    Ok(())
}

pub fn have_trailing_slash(path: &Path) -> bool {
    path.as_os_str().as_bytes().last() == Some(&b'/')
}

pub fn strip_trailing_slash(path: &Path) -> PathBuf {
    if !have_trailing_slash(path) {
        return PathBuf::from(path);
    }
    let bytes = path.as_os_str().as_bytes();
    let without_last = &bytes[0..bytes.len() - 1];
    PathBuf::from(OsStr::from_bytes(without_last))
}

/// Keep immutable until the end of this scope. Useful if the `path` needs to be
/// made mutable again after the scope.
pub fn keep_immutable(path: &Path) -> Result<ImmutableGuard, ImmutableError> {
    if is_immutable(path)? {
        Err(ImmutableError::AlreadyImmutable)
    } else {
        set_immutable(path, true)?;
        Ok(ImmutableGuard {
            path: path.to_owned(),
        })
    }
}

pub struct ImmutableGuard {
    path: PathBuf,
}

impl Drop for ImmutableGuard {
    fn drop(&mut self) {
        if let Err(e) = set_immutable(&self.path, false) {
            log::error!("could not reset immutable flag for {:?}: {e}", self.path);
        }
    }
}

/// The agent identified by the client certificate of a TLS connection: `None` if the client
/// sent no certificate, an error if its certificate names no agent. The certificate itself is
/// verified against the CA in the handshake.
pub fn peer_agent_id(stream: &TlsStream<TcpStream>) -> Result<Option<AgentId>, ClientCertError> {
    let (_, server_connection) = stream.get_ref();
    let Some(peer_cert) = server_connection
        .peer_certificates()
        .and_then(|certs| certs.first())
    else {
        return Ok(None);
    };
    AgentId::from_cert(peer_cert).map(Some)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::helpers::{have_trailing_slash, strip_trailing_slash};

    #[test]
    fn test_have_trailing_slash() {
        assert!(have_trailing_slash(&PathBuf::from("/")));
        assert!(have_trailing_slash(&PathBuf::from("/stuff/")));
        assert!(have_trailing_slash(&PathBuf::from("/more/stuff/")));
    }

    #[test]
    fn test_strip_trailing_slash() {
        assert_eq!(strip_trailing_slash(&PathBuf::from("/")), PathBuf::from(""));
        assert_eq!(
            strip_trailing_slash(&PathBuf::from("/stuff/")),
            PathBuf::from("/stuff")
        );
        assert_eq!(
            strip_trailing_slash(&PathBuf::from("/more/stuff/")),
            PathBuf::from("/more/stuff")
        );
    }
}
