#![cfg(test)]
#![allow(clippy::unwrap_used)]
use std::{
    ffi::{CStr, CString},
    io,
    path::PathBuf,
    sync::Arc,
};

use etc_passwd::Passwd;
use log::warn;
use tokio::sync::{Mutex, mpsc};

use crate::{
    mount::{BindMount, MountBackend}, process_manager::{ProcessManager, ProcessManagerCommand}, share_manager::{CreateUserError, UserProvider}
};

#[derive(Clone)]
pub struct MockUserProvider {
    users: Arc<std::sync::Mutex<Vec<(String, u32)>>>,
}

impl MockUserProvider {
    pub fn new(users: Vec<(String, u32)>) -> Self {
        Self { users: Arc::new(std::sync::Mutex::new(users)) }
    }
}

impl UserProvider for MockUserProvider {
    fn passwd_from_name(&self, name: impl AsRef<CStr>) -> io::Result<Option<etc_passwd::Passwd>> {
        let name: &CStr = name.as_ref();
        let name_str = String::from_utf8(name.to_bytes().to_owned()).unwrap();

        for entry in self.users.lock().unwrap().iter() {
            if name_str == entry.0 {
                return Ok(Some(Passwd {
                    name: CString::new(entry.0.as_bytes()).unwrap(),
                    passwd: CString::new("x".as_bytes()).unwrap(),
                    uid: entry.1,
                    gid: entry.1,
                    gecos: CString::new("".as_bytes()).unwrap(),
                    dir: CString::new("".as_bytes()).unwrap(),
                    shell: CString::new("".as_bytes()).unwrap(),
                }));
            }
        }
        Ok(None)
    }

    fn passwd_from_uid(&self, uid: u32) -> io::Result<Option<etc_passwd::Passwd>> {
        for entry in self.users.lock().unwrap().iter() {
            if uid == entry.1 {
                return Ok(Some(Passwd {
                    name: CString::new(entry.0.as_bytes()).unwrap(),
                    passwd: CString::new("x".as_bytes()).unwrap(),
                    uid: entry.1,
                    gid: entry.1,
                    gecos: CString::new("".as_bytes()).unwrap(),
                    dir: CString::new("".as_bytes()).unwrap(),
                    shell: CString::new("".as_bytes()).unwrap(),
                }));
            }
        }
        Ok(None)
    }

    async fn add_system_user(
        &self,
        user: &crate::share_manager::User,
    ) -> Result<(), CreateUserError> {
        warn!("mock: add_system_user {user:?}");
        self.users.lock().unwrap().push((user.username.clone(), user.uid));
        Ok(())
    }

    async fn add_samba_user(
        &self,
        user: &crate::share_manager::User,
    ) -> Result<(), CreateUserError> {
        warn!("mock: add_samba_user {user:?}");
        Ok(())
    }

    async fn remove_system_user(&self, username: &str) -> Result<(), CreateUserError> {
        self.users.lock().unwrap().retain(|f| f.0 != username);
        warn!("mock: remove_samba_user {username:?}");
        Ok(())
    }

    async fn remove_samba_user(&self, _username: &str) -> Result<(), CreateUserError> {
        todo!();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockCalls {
    Mount { mount: BindMount },
    Preflight { mount: BindMount },
    Umount { mount_point: PathBuf },
    ListMount { path_prefix: PathBuf },
}

#[derive(Debug, Clone)]
pub struct MockMountBackend {
    // might be fishy in async context, but we're only using it for testing...
    pub calls: Arc<std::sync::Mutex<Vec<MockCalls>>>,
}

impl MockMountBackend {
    pub fn new() -> Self {
        Self {
            calls: Default::default(),
        }
    }
}

impl MountBackend for MockMountBackend {
    fn mount(
        &self,
        mount: &crate::mount::BindMount,
    ) -> Result<(), crate::mount::MountBackendError> {
        self.calls.lock().unwrap().push(MockCalls::Mount {
            mount: mount.clone(),
        });
        Ok(())
    }

    fn preflight(
        &self,
        mount: &crate::mount::BindMount,
    ) -> Result<(), crate::mount::MountBackendError> {
        self.calls.lock().unwrap().push(MockCalls::Preflight {
            mount: mount.clone(),
        });
        Ok(())
    }

    fn umount(&self, mount_point: &std::path::Path) -> Result<(), crate::mount::MountBackendError> {
        self.calls.lock().unwrap().push(MockCalls::Umount {
            mount_point: mount_point.to_owned(),
        });
        Ok(())
    }

    fn list_mounts(
        &self,
        path_prefix: &std::path::Path,
    ) -> Result<std::collections::HashSet<crate::mount::BindMount>, crate::mount::MountBackendError>
    {
        self.calls.lock().unwrap().push(MockCalls::ListMount {
            path_prefix: path_prefix.to_owned(),
        });
        Ok(Default::default())
    }
}


#[derive(Clone)]
pub struct MockProcessManager {
    pub q: Arc<Mutex<Option<mpsc::Receiver<ProcessManagerCommand>>>>,
}

impl MockProcessManager {
    pub fn new() -> Self { 
        Self { q: Arc::new(Mutex::new(None)) }
    }
}

impl ProcessManager for MockProcessManager {
    async fn start_in_background(&self, debug: bool) -> mpsc::Sender<ProcessManagerCommand> {
        warn!("MockProcessManager::start_in_background debug={debug}");
        let (sender, receiver) = mpsc::channel::<ProcessManagerCommand>(1024);
        self.q.lock().await.replace(receiver);

        sender
    }
}