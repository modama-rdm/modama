//! Windows mount backend: one reconciliation pass over drive mappings. The WNet FFI lives in
//! `platform_windows.rs`; this part is platform-independent, so it is tested everywhere.
//!
//! On Windows, every connection to the configured host is ours, whatever share it is for:
//! Windows allows only one set of credentials per server and logon session, so any other
//! connection to that host (another researcher's mapping, a deviceless connection, `IPC$`)
//! blocks our mappings with error 1219.

use std::{collections::HashMap, path::PathBuf};

use log::{debug, warn};
use modama_common::{
    foreground::same_share,
    internal_api_types::{MountedShare, SessionHelperConfig, SessionHelperState, ShareError},
    messages::Share,
};

use crate::mount_backend::{EnumerationError, PassResult, unique_by_name};

/// A network connection of this logon session
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Connection {
    /// The drive letter with colon (`Z:`); `None` for a deviceless connection
    pub local: Option<String>,
    /// `\\host\share`, possibly followed by a path
    pub remote: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddError {
    /// The drive letter was taken since we looked
    LetterTaken,
    /// Another set of credentials is connected to the server (error 1219)
    CredentialConflict,
    AuthenticationFailed,
    Other(String),
}

impl std::fmt::Display for AddError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LetterTaken => write!(f, "mount failed: drive letter already in use"),
            Self::CredentialConflict => write!(
                f,
                "another user's drive is still connected; close their files or release it"
            ),
            Self::AuthenticationFailed => write!(f, "mount failed: authentication failed"),
            Self::Other(message) => write!(f, "mount failed: {message}"),
        }
    }
}

/// The WNet operations a pass needs
pub trait WNetApi {
    /// All network connections of this logon session
    fn connections(&self) -> Result<Vec<Connection>, String>;
    /// Bitmask of drive letters in use, bit 0 being `A:` (`GetLogicalDrives`)
    fn used_drives(&self) -> u32;
    fn add(&self, drive: &str, remote: &str, user: &str, password: &str) -> Result<(), AddError>;
    /// Remove a connection by drive letter, or by UNC name for a deviceless one; with `force`,
    /// even if files are open on it
    fn cancel(&self, name: &str, force: bool) -> Result<(), String>;
}

/// The share name (empty for a bare `\\host`), if `remote` is a connection to `host`
fn our_share<'a>(remote: &'a str, host: &str) -> Option<&'a str> {
    let mut parts = remote.strip_prefix(r"\\")?.split('\\');
    let remote_host = parts.next()?;
    remote_host
        .eq_ignore_ascii_case(host)
        .then(|| parts.next().unwrap_or(""))
}

fn valid_host(config: &SessionHelperConfig) -> Result<&str, EnumerationError> {
    let host = config.smb_hostname.as_str();
    if host.is_empty() || host.chars().any(|c| c.is_whitespace() || "\\/".contains(c)) {
        return Err(EnumerationError(format!(
            "invalid SMB hostname in the configuration: {host:?}"
        )));
    }
    Ok(host)
}

/// `DOMAIN\user`: an unqualified name on a domain-joined client could be taken as a domain
/// account by the standalone Samba server
fn qualified_username(username: &str, domain: &str) -> String {
    if username.contains('\\') || domain.is_empty() {
        username.to_owned()
    } else {
        format!(r"{domain}\{username}")
    }
}

fn drive_bit(drive: &str) -> u32 {
    match drive.as_bytes().first() {
        Some(letter @ b'A'..=b'Z') => 1 << (letter - b'A'),
        Some(letter @ b'a'..=b'z') => 1 << (letter - b'a'),
        _ => 0,
    }
}

/// Map `remote` at the highest free drive letter from `Z:` down to `D:`; returns the drive
fn map_at_free_letter(
    api: &impl WNetApi,
    in_use: &mut u32,
    remote: &str,
    user: &str,
    password: &str,
) -> Result<String, AddError> {
    for letter in (b'D'..=b'Z').rev() {
        let drive = format!("{}:", char::from(letter));
        let bit = drive_bit(&drive);
        if *in_use & bit != 0 {
            continue;
        }
        *in_use |= bit;
        match api.add(&drive, remote, user, password) {
            Ok(()) => return Ok(drive),
            Err(AddError::LetterTaken) => {}
            Err(e) => return Err(e),
        }
    }
    Err(AddError::Other("no free drive letter".to_owned()))
}

/// One reconciliation pass: remove our connections that aren't a desired drive mapping (with
/// `force`, even if files are open on them), map the missing shares, and return the actual
/// state afterwards.
pub fn reconcile_pass(
    api: &impl WNetApi,
    shares: &[Share],
    config: &SessionHelperConfig,
    force: bool,
) -> Result<PassResult, EnumerationError> {
    let host = valid_host(config)?;
    let ours = |all: Vec<Connection>| {
        all.into_iter()
            .filter_map(|c| Some((our_share(&c.remote, host)?.to_owned(), c)))
            .collect::<Vec<_>>()
    };
    let before = ours(api.connections().map_err(EnumerationError)?);
    let (desired, mut errors) = unique_by_name(shares);

    // removal first: credentials of other connections to the host would block our mappings.
    // We keep one drive mapping per desired share; deviceless connections never count.
    let mut kept: Vec<&str> = Vec::new();
    let mut still_mapped = 0;
    for (name, connection) in &before {
        if connection.local.is_some()
            && desired.iter().any(|s| same_share(&s.share_name, name))
            && !kept.iter().any(|k| same_share(k, name))
        {
            kept.push(name);
            continue;
        }
        let target = connection.local.as_deref().unwrap_or(&connection.remote);
        debug!("removing connection {target} to {}", connection.remote);
        if let Err(e) = api.cancel(target, force) {
            // it stays in the report, so the scope reads as not converged; the next pass retries
            warn!(
                "could not remove connection {target} to {}: {e}",
                connection.remote
            );
            still_mapped |= connection.local.as_deref().map_or(0, drive_bit);
        }
    }

    let mut in_use = api.used_drives() | still_mapped;
    let mut add_errors = HashMap::new();
    let mut created = Vec::new();
    for share in &desired {
        if kept.iter().any(|k| same_share(k, &share.share_name)) {
            continue;
        }
        if share.share_name.is_empty() || share.share_name.contains(['\\', '/']) {
            add_errors.insert(share.share_id.clone(), "invalid share name".to_owned());
            continue;
        }
        let remote = format!(r"\\{host}\{}", share.share_name);
        let user = qualified_username(&share.username, &config.smb_domain);
        match map_at_free_letter(api, &mut in_use, &remote, &user, &share.password) {
            Ok(drive) => {
                debug!("mapped {remote} at {drive}");
                created.push(share.share_name.clone());
            }
            Err(e) => {
                add_errors.insert(share.share_id.clone(), e.to_string());
            }
        }
    }

    let after = ours(api.connections().map_err(EnumerationError)?);
    for share in &desired {
        let mapped = after
            .iter()
            .any(|(name, c)| c.local.is_some() && same_share(name, &share.share_name));
        if !mapped {
            errors.push(ShareError {
                share_id: share.share_id.clone(),
                message: add_errors
                    .remove(&share.share_id)
                    .unwrap_or_else(|| "not mounted".to_owned()),
            });
        }
    }
    Ok(PassResult {
        state: SessionHelperState {
            mounted_shares: after
                .into_iter()
                .map(|(share_name, connection)| MountedShare {
                    share_name,
                    // a deviceless connection exists, but offers no path
                    mount_path: connection
                        .local
                        .map(|drive| PathBuf::from(format!(r"{drive}\"))),
                })
                .collect(),
            errors,
            // the main loop adds which desired state the pass was for
            reconciled_share_ids: None,
        },
        created,
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    fn config() -> SessionHelperConfig {
        SessionHelperConfig {
            smb_hostname: "smb.example.test".to_owned(),
            smb_port: 445,
            smb_domain: "WORKGROUP".to_owned(),
        }
    }

    fn share(id: &str, name: &str) -> Share {
        Share {
            resource: None,
            share_id: id.to_owned(),
            share_name: name.to_owned(),
            dest_path: format!("/project/{id}"),
            created_timestamp: 0,
            username: "user".to_owned(),
            password: "s3cret-password".to_owned(),
            uid: 1000,
            gid: 1000,
            created_by: None,
        }
    }

    fn mapping(drive: &str, remote: &str) -> Connection {
        Connection {
            local: Some(drive.to_owned()),
            remote: remote.to_owned(),
        }
    }

    fn deviceless(remote: &str) -> Connection {
        Connection {
            local: None,
            remote: remote.to_owned(),
        }
    }

    #[derive(Default)]
    struct FakeWNet {
        connections: RefCell<Vec<Connection>>,
        /// letters in use besides the connections, e.g. local volumes
        local_drives: u32,
        inventory_error: Option<String>,
        /// by remote name
        add_errors: HashMap<String, AddError>,
        /// letters that turn out to be taken when adding
        taken_on_add: Vec<String>,
        cancel_errors: Vec<String>,
        calls: RefCell<Vec<String>>,
    }

    impl FakeWNet {
        fn with(connections: Vec<Connection>) -> Self {
            Self {
                connections: RefCell::new(connections),
                ..Self::default()
            }
        }
    }

    impl WNetApi for FakeWNet {
        fn connections(&self) -> Result<Vec<Connection>, String> {
            match &self.inventory_error {
                Some(e) => Err(e.clone()),
                None => Ok(self.connections.borrow().clone()),
            }
        }

        fn used_drives(&self) -> u32 {
            self.connections
                .borrow()
                .iter()
                .filter_map(|c| c.local.as_deref())
                .fold(self.local_drives, |mask, drive| mask | drive_bit(drive))
        }

        fn add(
            &self,
            drive: &str,
            remote: &str,
            user: &str,
            password: &str,
        ) -> Result<(), AddError> {
            self.calls
                .borrow_mut()
                .push(format!("add {drive} {remote} {user} {password}"));
            if self.taken_on_add.iter().any(|d| d == drive) {
                return Err(AddError::LetterTaken);
            }
            match self.add_errors.get(remote) {
                Some(AddError::CredentialConflict) => Err(AddError::CredentialConflict),
                Some(AddError::AuthenticationFailed) => Err(AddError::AuthenticationFailed),
                Some(AddError::Other(m)) => Err(AddError::Other(m.clone())),
                Some(AddError::LetterTaken) => Err(AddError::LetterTaken),
                None => {
                    self.connections.borrow_mut().push(mapping(drive, remote));
                    Ok(())
                }
            }
        }

        fn cancel(&self, name: &str, force: bool) -> Result<(), String> {
            let forced = if force { " (forced)" } else { "" };
            self.calls
                .borrow_mut()
                .push(format!("cancel {name}{forced}"));
            if self.cancel_errors.iter().any(|n| n == name) {
                return Err("open files".to_owned());
            }
            self.connections
                .borrow_mut()
                .retain(|c| c.local.as_deref().unwrap_or(&c.remote) != name);
            Ok(())
        }
    }

    fn mounted(state: &SessionHelperState) -> Vec<(String, Option<String>)> {
        let mut mounted: Vec<_> = state
            .mounted_shares
            .iter()
            .map(|m| {
                (
                    m.share_name.clone(),
                    m.mount_path.as_ref().map(|p| p.display().to_string()),
                )
            })
            .collect();
        mounted.sort();
        mounted
    }

    fn entry(name: &str, path: Option<&str>) -> (String, Option<String>) {
        (name.to_owned(), path.map(str::to_owned))
    }

    #[test]
    fn our_share_matches_the_host_only() {
        let host = "smb.example.test";
        assert_eq!(our_share(r"\\smb.example.test\a", host), Some("a"));
        assert_eq!(our_share(r"\\SMB.Example.Test\Share", host), Some("Share"));
        assert_eq!(our_share(r"\\smb.example.test\a\sub\dir", host), Some("a"));
        assert_eq!(our_share(r"\\smb.example.test\IPC$", host), Some("IPC$"));
        assert_eq!(our_share(r"\\smb.example.test", host), Some(""));
        assert_eq!(our_share(r"\\other.example.test\a", host), None);
        assert_eq!(our_share(r"\\smb.example.test.evil\a", host), None);
        assert_eq!(our_share("smb.example.test\\a", host), None);
        assert_eq!(our_share("", host), None);
    }

    #[test]
    fn keeps_desired_removes_obsolete_and_leaves_other_hosts_alone() {
        let api = FakeWNet::with(vec![
            mapping("Z:", r"\\smb.example.test\keep"),
            mapping("Y:", r"\\smb.example.test\stale"),
            mapping("X:", r"\\other.example.test\stale"),
        ]);
        let state = reconcile_pass(
            &api,
            &[share("1", "keep"), share("2", "new")],
            &config(),
            false,
        )
        .unwrap()
        .state;
        assert_eq!(
            mounted(&state),
            vec![entry("keep", Some(r"Z:\")), entry("new", Some(r"Y:\"))]
        );
        assert!(state.errors.is_empty());
        assert_eq!(
            *api.calls.borrow(),
            vec![
                "cancel Y:",
                r"add Y: \\smb.example.test\new WORKGROUP\user s3cret-password"
            ]
        );
    }

    #[test]
    fn deviceless_connections_are_removed_and_never_count_as_mounted() {
        let api = FakeWNet {
            cancel_errors: vec![r"\\smb.example.test\IPC$".to_owned()],
            ..FakeWNet::with(vec![
                deviceless(r"\\smb.example.test\a"),
                deviceless(r"\\smb.example.test\IPC$"),
            ])
        };
        let state = reconcile_pass(&api, &[share("1", "a")], &config(), false)
            .unwrap()
            .state;
        assert_eq!(
            api.calls.borrow()[..2],
            [
                r"cancel \\smb.example.test\a",
                r"cancel \\smb.example.test\IPC$"
            ]
        );
        // the IPC$ connection couldn't be removed: it exists, without a path
        assert_eq!(
            mounted(&state),
            vec![entry("IPC$", None), entry("a", Some(r"Z:\"))]
        );
    }

    #[test]
    fn duplicate_mappings_of_one_share_are_reduced_to_one() {
        let api = FakeWNet::with(vec![
            mapping("Z:", r"\\smb.example.test\a"),
            mapping("Y:", r"\\SMB.EXAMPLE.TEST\A"),
        ]);
        let state = reconcile_pass(&api, &[share("1", "a")], &config(), false)
            .unwrap()
            .state;
        assert_eq!(*api.calls.borrow(), vec!["cancel Y:"]);
        assert_eq!(mounted(&state), vec![entry("a", Some(r"Z:\"))]);
    }

    #[test]
    fn picks_the_highest_free_letter_and_retries_when_taken() {
        let api = FakeWNet {
            local_drives: drive_bit("Z:"),
            taken_on_add: vec!["Y:".to_owned()],
            ..FakeWNet::with(vec![mapping("X:", r"\\other.example.test\x")])
        };
        let state = reconcile_pass(&api, &[share("1", "a")], &config(), false)
            .unwrap()
            .state;
        assert_eq!(
            *api.calls.borrow(),
            vec![
                r"add Y: \\smb.example.test\a WORKGROUP\user s3cret-password",
                r"add W: \\smb.example.test\a WORKGROUP\user s3cret-password",
            ]
        );
        assert_eq!(mounted(&state), vec![entry("a", Some(r"W:\"))]);
    }

    #[test]
    fn no_free_letter_is_an_error() {
        let api = FakeWNet {
            local_drives: u32::MAX,
            ..FakeWNet::default()
        };
        let state = reconcile_pass(&api, &[share("1", "a")], &config(), false)
            .unwrap()
            .state;
        assert_eq!(
            state.errors[0].message,
            "mount failed: no free drive letter"
        );
    }

    #[test]
    fn add_errors_are_reported_per_share() {
        let api = FakeWNet {
            add_errors: HashMap::from([
                (
                    r"\\smb.example.test\a".to_owned(),
                    AddError::CredentialConflict,
                ),
                (
                    r"\\smb.example.test\b".to_owned(),
                    AddError::AuthenticationFailed,
                ),
            ]),
            ..FakeWNet::default()
        };
        let state = reconcile_pass(&api, &[share("1", "a"), share("2", "b")], &config(), false)
            .unwrap()
            .state;
        let messages: Vec<_> = state.errors.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(
            messages,
            vec![
                "another user's drive is still connected; close their files or release it",
                "mount failed: authentication failed",
            ]
        );
        assert!(state.mounted_shares.is_empty());
    }

    #[test]
    fn usernames_are_qualified_with_the_domain() {
        assert_eq!(qualified_username("user", "WORKGROUP"), r"WORKGROUP\user");
        assert_eq!(qualified_username(r"DOM\user", "WORKGROUP"), r"DOM\user");
        assert_eq!(qualified_username("user", ""), "user");
    }

    #[test]
    fn failed_removal_stays_in_the_report_and_keeps_its_letter() {
        let api = FakeWNet {
            cancel_errors: vec!["Z:".to_owned()],
            ..FakeWNet::with(vec![mapping("Z:", r"\\smb.example.test\busy")])
        };
        let state = reconcile_pass(&api, &[share("1", "a")], &config(), false)
            .unwrap()
            .state;
        assert_eq!(
            mounted(&state),
            vec![entry("a", Some(r"Y:\")), entry("busy", Some(r"Z:\"))]
        );
    }

    #[test]
    fn inventory_error_fails_the_pass() {
        let api = FakeWNet {
            inventory_error: Some("access denied".to_owned()),
            ..FakeWNet::default()
        };
        assert!(reconcile_pass(&api, &[share("1", "a")], &config(), false).is_err());
        assert!(api.calls.borrow().is_empty());
    }

    #[test]
    fn duplicate_and_invalid_desired_names_get_errors() {
        let api = FakeWNet::default();
        let state = reconcile_pass(
            &api,
            &[share("1", "a"), share("2", "A"), share("3", r"b\..")],
            &config(),
            false,
        )
        .unwrap()
        .state;
        let errors: Vec<_> = state
            .errors
            .iter()
            .map(|e| (e.share_id.as_str(), e.message.as_str()))
            .collect();
        assert_eq!(
            errors,
            vec![
                ("2", "another desired share has the same name"),
                ("3", "invalid share name"),
            ]
        );
        assert_eq!(mounted(&state), vec![entry("a", Some(r"Z:\"))]);
    }

    #[test]
    fn invalid_hostname_fails_the_pass() {
        let config = SessionHelperConfig {
            smb_hostname: r"host\share".to_owned(),
            ..config()
        };
        assert!(reconcile_pass(&FakeWNet::default(), &[], &config, false).is_err());
    }

    #[test]
    fn removals_are_forced_only_when_asked() {
        for force in [false, true] {
            let api = FakeWNet::with(vec![mapping("Z:", r"\\smb.example.test\old")]);
            reconcile_pass(&api, &[], &config(), force).unwrap();
            let expected = if force {
                "cancel Z: (forced)"
            } else {
                "cancel Z:"
            };
            assert_eq!(*api.calls.borrow(), vec![expected]);
        }
    }

    #[test]
    fn created_lists_only_what_this_pass_mapped() {
        let api = FakeWNet::with(vec![mapping("Z:", r"\\smb.example.test\keep")]);
        let result = reconcile_pass(
            &api,
            &[share("1", "keep"), share("2", "new")],
            &config(),
            false,
        )
        .unwrap();
        assert_eq!(result.created, vec!["new"]);
    }
}
