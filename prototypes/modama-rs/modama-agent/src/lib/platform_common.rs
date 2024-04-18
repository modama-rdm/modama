use thiserror::Error;

use crate::config::DNSLookupFailure;

#[derive(Error, Debug)]
pub enum MountListError {
    #[error("procfs error: {0}")]
    ProcError(String),
}

#[derive(Error, Debug)]
pub enum EnsureMountedError {
    #[error("checking list of mounts failed: {0}")]
    MountListFailed(#[from] MountListError),

    #[error("DNS failure: {0}")]
    LookupFailure(#[from] DNSLookupFailure),

    #[error("failed to execute mount: {0}")]
    MountFailed(String),

    #[error("ingest share is in use, please re-try umount")]
    InUseError,

    #[error("invalid credentials: {0}")]
    CredentialsError(String),

    #[error("io error: {0}")]
    IOError(#[from] std::io::Error),
}

