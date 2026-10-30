//! Exactly one session helper per helper scope: per user on Linux (a lock file in the user's
//! runtime directory), per desktop session on Windows (a named mutex in the session's namespace).

#[derive(Debug, thiserror::Error)]
pub enum SingletonError {
    #[error(
        "XDG_RUNTIME_DIR is not set; the session helper needs a per-user runtime directory, \
         usually provided by systemd-logind on login"
    )]
    NoRuntimeDir,

    #[error("could not create the lock file {path}: {source}")]
    LockFile {
        path: std::path::PathBuf,
        source: std::io::Error,
    },

    #[error("could not lock {path}: {source}")]
    Lock {
        path: std::path::PathBuf,
        source: std::io::Error,
    },

    #[cfg(windows)]
    #[error("could not create the session helper mutex: {0}")]
    Mutex(windows::core::Error),
}

#[derive(Debug)]
pub enum Singleton {
    /// We are the session helper of this scope, as long as the guard lives
    Acquired(SingletonGuard),

    /// Another session helper already runs in this scope
    AlreadyRunning,
}

#[cfg(unix)]
pub use unix::{SingletonGuard, acquire, acquire_at};

#[cfg(windows)]
pub use windows_impl::{SingletonGuard, acquire, is_elevated};

#[cfg(unix)]
mod unix {
    use std::{
        fs::{DirBuilder, File, OpenOptions, TryLockError},
        os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _},
        path::{Path, PathBuf},
    };

    use super::{Singleton, SingletonError};

    /// Holds the lock; the kernel releases it when the process exits, however it exits
    #[derive(Debug)]
    pub struct SingletonGuard {
        _file: File,
    }

    pub fn acquire() -> Result<Singleton, SingletonError> {
        let runtime_dir =
            std::env::var_os("XDG_RUNTIME_DIR").ok_or(SingletonError::NoRuntimeDir)?;
        acquire_at(&PathBuf::from(runtime_dir).join("modama/session-helper.lock"))
    }

    pub fn acquire_at(path: &Path) -> Result<Singleton, SingletonError> {
        let lock_file_error = |source| SingletonError::LockFile {
            path: path.to_owned(),
            source,
        };
        if let Some(dir) = path.parent() {
            DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)
                .map_err(lock_file_error)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(path)
            .map_err(lock_file_error)?;

        match file.try_lock() {
            Ok(()) => Ok(Singleton::Acquired(SingletonGuard { _file: file })),
            // only contention means "already running"; anything else is a real failure
            Err(TryLockError::WouldBlock) => Ok(Singleton::AlreadyRunning),
            Err(TryLockError::Error(source)) => Err(SingletonError::Lock {
                path: path.to_owned(),
                source,
            }),
        }
    }
}

#[cfg(windows)]
mod windows_impl {
    use windows::{
        Win32::{
            Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE},
            Security::{
                GetTokenInformation, TOKEN_ELEVATION_TYPE, TOKEN_QUERY, TokenElevationType,
                TokenElevationTypeFull,
            },
            System::Threading::{CreateMutexW, GetCurrentProcess, OpenProcessToken},
        },
        core::w,
    };

    use super::{Singleton, SingletonError};

    /// Holds the mutex; Windows releases it when the process exits, however it exits
    #[derive(Debug)]
    pub struct SingletonGuard {
        handle: HANDLE,
    }

    impl Drop for SingletonGuard {
        fn drop(&mut self) {
            // SAFETY: we own the handle, and close it exactly once
            if let Err(e) = unsafe { CloseHandle(self.handle) } {
                log::warn!("could not close the session helper mutex: {e}");
            }
        }
    }

    pub fn acquire() -> Result<Singleton, SingletonError> {
        // `Local\` is scoped to the current desktop session
        // SAFETY: no security attributes, and a valid, nul-terminated name
        let handle = unsafe {
            CreateMutexW(
                None,
                true,
                w!("Local\\ModamaSessionHelper-5f3c1a2e-8d47-4b9e-a6f1-2c7d9e0b4a85"),
            )
        }
        .map_err(SingletonError::Mutex)?;
        // SAFETY: called directly after CreateMutexW, which sets the last error on success, too
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            // SAFETY: we own this handle (to the existing mutex) and don't use it afterwards
            let _ = unsafe { CloseHandle(handle) };
            return Ok(Singleton::AlreadyRunning);
        }
        Ok(Singleton::Acquired(SingletonGuard { handle }))
    }

    /// Whether we run as the elevated half of a split (UAC) token, i.e. via "Run as
    /// administrator". Administrator accounts without elevation, a disabled UAC and the built-in
    /// Administrator account have no split token and are fine: Explorer runs with the same token,
    /// so it sees our drive mappings.
    pub fn is_elevated() -> windows::core::Result<bool> {
        let mut token = HANDLE::default();
        // SAFETY: the pseudo handle of the current process needs no closing, `token` is ours
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }?;
        let mut elevation_type = TOKEN_ELEVATION_TYPE::default();
        let mut returned = 0u32;
        // SAFETY: the buffer is a TOKEN_ELEVATION_TYPE, and its size is passed along
        let result = unsafe {
            GetTokenInformation(
                token,
                TokenElevationType,
                Some(std::ptr::from_mut(&mut elevation_type).cast()),
                u32::try_from(size_of::<TOKEN_ELEVATION_TYPE>()).unwrap_or(u32::MAX),
                &mut returned,
            )
        };
        // SAFETY: we opened the token above and close it exactly once
        let _ = unsafe { CloseHandle(token) };
        result?;
        Ok(elevation_type == TokenElevationTypeFull)
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;

    fn temp_lock_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!("modama-singleton-test-{}", std::process::id()))
            .join(name)
    }

    #[test]
    fn second_acquire_finds_the_first_one_and_drop_releases() {
        let path = temp_lock_path("helper.lock");

        let first = acquire_at(&path).unwrap();
        assert!(matches!(first, Singleton::Acquired(_)));
        assert!(matches!(
            acquire_at(&path).unwrap(),
            Singleton::AlreadyRunning
        ));

        drop(first);
        assert!(matches!(acquire_at(&path).unwrap(), Singleton::Acquired(_)));
    }

    #[test]
    fn uncreatable_lock_directory_is_an_error() {
        // a regular file can't be a parent directory:
        let blocker = temp_lock_path("not-a-dir");
        std::fs::create_dir_all(blocker.parent().unwrap()).unwrap();
        std::fs::write(&blocker, b"").unwrap();

        assert!(matches!(
            acquire_at(&blocker.join("helper.lock")),
            Err(SingletonError::LockFile { .. })
        ));
    }
}
