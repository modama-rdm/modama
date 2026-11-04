//! One scope's view of its shares: the desired state (from smbdrop via the agent) joined with
//! the report of the scope's session helper (what it actually mounted). [`ScopeView`] owns the
//! rule by which the two are matched ([`same_share`]) and answers every question callers have
//! about the pair: where to write right now (the foreground projection), which other shares are
//! mounted, whether a hand-off is blocked, and whether actual state has caught up. The agent
//! uses it for its HTTP API and mount status, the GUI and the session helper for what they show.

use std::path::Path;

use log::warn;

use crate::{
    internal_api_types::{MountedShare, SessionHelperState, Shares},
    local_api_messages::APICurrentForegroundShare,
    messages::{AgentMountStatus, Share},
};

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ShareDestinationError {
    #[error("share destination has no session: {dest_path:?}")]
    MissingSession { dest_path: String },

    #[error("share destination has no project: {dest_path:?}")]
    MissingProject { dest_path: String },
}

pub fn project_and_session(dest_path: &str) -> Result<(String, String), ShareDestinationError> {
    let mut components = dest_path.split('/').filter(|part| !part.is_empty()).rev();
    let session = components
        .next()
        .ok_or_else(|| ShareDestinationError::MissingSession {
            dest_path: dest_path.to_owned(),
        })?;
    let project = components
        .next()
        .ok_or_else(|| ShareDestinationError::MissingProject {
            dest_path: dest_path.to_owned(),
        })?;
    Ok((project.to_owned(), session.to_owned()))
}

/// Whether two share names name the same share. SMB share names are case-insensitive, and a
/// helper may report a mount under another case than smbdrop's (`Foo` for `foo`). Mounts are
/// identified by name, as the helper may report mounts we know nothing about.
pub fn same_share(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// Shown when no session helper has reported for the caller's scope
pub const NO_HELPER_MESSAGE: &str = "No session helper is running for you. Open the Modama \
                                 GUI, which starts it, or run `modama-agent-gui session-helper`.";

/// Shown while the helper is there, but hasn't mounted the share (and hasn't reported an error)
pub const WAITING_MESSAGE: &str = "Waiting for the session helper to mount the share.";

/// Where one desired share stands, according to the session helper
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareMount<'a> {
    /// No session helper has reported for the scope
    NoHelper,
    /// Mounted, at a path applications can use
    Mounted(&'a Path),
    /// Not mounted (or without a usable path); the helper's error for it
    Failed(&'a str),
    /// Not mounted (or without a usable path) yet, and no error either
    Waiting,
}

/// A desired share other than the foreground one that the caller's session helper has mounted
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OtherMount {
    pub share_id: String,
    pub project: String,
    pub session: String,
    pub created_by: Option<String>,
    pub mount_path: String,
}

/// The desired shares of a scope, joined with what its session helper reported; build it once
/// per state and read the answers from it
#[derive(Clone, Copy, Debug)]
pub struct ScopeView<'a> {
    shares: &'a [Share],
    foreground_share_id: Option<&'a str>,
    helper: Option<&'a SessionHelperState>,
}

impl<'a> ScopeView<'a> {
    /// `desired`: None while the desired state isn't known yet; nothing counts as desired then.
    /// `helper`: the last report of the scope's session helper; None if none has reported.
    pub fn new(desired: Option<&'a Shares>, helper: Option<&'a SessionHelperState>) -> Self {
        Self {
            shares: desired.map_or(&[], |desired| desired.shares.as_slice()),
            foreground_share_id: desired.and_then(|desired| desired.foreground_share_id.as_deref()),
            helper,
        }
    }

    fn mount_of(&self, share: &'a Share) -> ShareMount<'a> {
        let Some(helper) = self.helper else {
            return ShareMount::NoHelper;
        };
        let mount_path = helper
            .mounted_shares
            .iter()
            .filter(|mounted| same_share(&mounted.share_name, &share.share_name))
            .find_map(|mounted| mounted.mount_path.as_deref());
        if let Some(mount_path) = mount_path {
            return ShareMount::Mounted(mount_path);
        }
        helper
            .errors
            .iter()
            .find(|error| error.share_id == share.share_id)
            .map_or(ShareMount::Waiting, |error| {
                ShareMount::Failed(error.message.as_str())
            })
    }

    /// Every desired share with where it stands, in the order of the desired state (oldest
    /// first)
    pub fn desired(&self) -> impl Iterator<Item = (&'a Share, ShareMount<'a>)> + '_ {
        self.shares
            .iter()
            .map(|share| (share, self.mount_of(share)))
    }

    /// The reported mounts that no desired share matches: left over from a share that is no
    /// longer desired, awaiting removal
    pub fn stale_mounts(&self) -> impl Iterator<Item = &'a MountedShare> + '_ {
        self.helper
            .into_iter()
            .flat_map(|helper| &helper.mounted_shares)
            .filter(|mounted| {
                !self
                    .shares
                    .iter()
                    .any(|share| same_share(&share.share_name, &mounted.share_name))
            })
    }

    /// The desired foreground share; the stream only carries its ID
    fn foreground_share(&self) -> Option<&'a Share> {
        let id = self.foreground_share_id?;
        self.shares.iter().find(|share| share.share_id == id)
    }

    /// Answer "where should I write right now?"
    pub fn foreground(&self) -> Result<APICurrentForegroundShare, ShareDestinationError> {
        let Some(desired) = self.foreground_share() else {
            return Ok(APICurrentForegroundShare::None);
        };
        let (project, session) = project_and_session(&desired.dest_path)?;
        let unavailable = |message: String| APICurrentForegroundShare::Unavailable {
            share_id: desired.share_id.clone(),
            project: project.clone(),
            session: session.clone(),
            dest_path: desired.dest_path.clone(),
            message,
            created_by: desired.created_by.clone(),
        };
        Ok(match self.mount_of(desired) {
            ShareMount::NoHelper => unavailable(NO_HELPER_MESSAGE.to_owned()),
            ShareMount::Waiting => unavailable(WAITING_MESSAGE.to_owned()),
            ShareMount::Failed(message) => unavailable(message.to_owned()),
            ShareMount::Mounted(mount_path) => match mount_path.to_str() {
                Some(mount_path) => APICurrentForegroundShare::Mounted {
                    share_id: desired.share_id.clone(),
                    project,
                    session,
                    dest_path: desired.dest_path.clone(),
                    mount_path: mount_path.to_owned(),
                    created_by: desired.created_by.clone(),
                },
                None => {
                    let lossy = mount_path.to_string_lossy();
                    warn!("Mount path {lossy} contains invalid unicode, cannot report it");
                    unavailable(format!(
                        "The share is mounted at {lossy}, but that path can't be represented \
                         as text."
                    ))
                }
            },
        })
    }

    /// Where the foreground share is mounted; None unless it is mounted at a usable path
    pub fn foreground_mount_path(&self) -> Option<&'a Path> {
        match self.mount_of(self.foreground_share()?) {
            ShareMount::Mounted(path) => Some(path),
            ShareMount::NoHelper | ShareMount::Failed(_) | ShareMount::Waiting => None,
        }
    }

    /// The desired shares besides the foreground one that are mounted in the scope, in the
    /// order of the desired state (oldest first). On Linux, a user's earlier shares on this
    /// resource stay mounted next to the newest one.
    pub fn other_mounts(&self) -> Vec<OtherMount> {
        self.desired()
            .filter(|(share, _)| self.foreground_share_id != Some(share.share_id.as_str()))
            .filter_map(|(share, mount)| {
                let ShareMount::Mounted(mount_path) = mount else {
                    return None;
                };
                let Some(mount_path) = mount_path.to_str() else {
                    warn!(
                        "Mount path {} contains invalid unicode, not listing it",
                        mount_path.to_string_lossy()
                    );
                    return None;
                };
                let (project, session) = match project_and_session(&share.dest_path) {
                    Ok(names) => names,
                    Err(error) => {
                        warn!("not listing mounted share {}: {error}", share.share_id);
                        return None;
                    }
                };
                Some(OtherMount {
                    share_id: share.share_id.clone(),
                    project,
                    session,
                    created_by: share.created_by.clone(),
                    mount_path: mount_path.to_owned(),
                })
            })
            .collect()
    }

    /// A hand-off is blocked when a mount that is no longer desired is still there and a
    /// desired share failed: on Windows, the old mount's credentials keep the new ones out
    /// (error 1219) until it is released, by force if files are open on it.
    pub fn hand_off_blocked(&self) -> bool {
        self.stale_mounts().next().is_some()
            && self
                .desired()
                .any(|(_, mount)| matches!(mount, ShareMount::Failed(_)))
    }

    /// Whether actual state differs from desired state: a desired share is missing or has no
    /// usable path, a mount that is no longer desired is still there, or the helper hasn't
    /// reported at all. Every reason for it heals on a later pass or has its desire withdrawn;
    /// whether a pass for the current desired state already ran is a separate question, see
    /// [`Self::mount_status`].
    pub fn actual_differs_from_desired(&self) -> bool {
        self.helper.is_none()
            || self
                .desired()
                .any(|(_, mount)| !matches!(mount, ShareMount::Mounted(_)))
            || self.stale_mounts().next().is_some()
    }

    /// Mount status of the scope's session helper (no report: connected, but hasn't reported
    /// yet)
    pub fn mount_status(&self) -> AgentMountStatus {
        let Some(helper) = self.helper else {
            return AgentMountStatus::Reconciling;
        };
        let differs = self.actual_differs_from_desired();
        match &helper.reconciled_share_ids {
            Some(ids) if *ids != sorted_ids(self.shares) => AgentMountStatus::Reconciling,
            Some(_) if differs => AgentMountStatus::LeftoverWork,
            // a helper that doesn't echo can't tell us whether its pass already ran
            None if differs => AgentMountStatus::Reconciling,
            Some(_) | None => AgentMountStatus::Idle,
        }
    }
}

/// The sorted IDs of the desired shares, as the session helper echoes them in its report
pub fn desired_share_ids(desired: &Shares) -> Vec<String> {
    sorted_ids(&desired.shares)
}

fn sorted_ids(shares: &[Share]) -> Vec<String> {
    let mut ids: Vec<String> = shares.iter().map(|share| share.share_id.clone()).collect();
    ids.sort();
    ids
}

/// Mount status of the agent: `NoHelper` without any connected session helper, otherwise the
/// worst status of its helpers' scopes
pub fn agent_mount_status<'a>(scopes: impl IntoIterator<Item = ScopeView<'a>>) -> AgentMountStatus {
    fn badness(status: AgentMountStatus) -> u8 {
        match status {
            AgentMountStatus::Idle => 0,
            AgentMountStatus::Reconciling => 1,
            AgentMountStatus::LeftoverWork => 2,
            AgentMountStatus::NoHelper | AgentMountStatus::Unknown => 3,
        }
    }
    scopes
        .into_iter()
        .map(|scope| scope.mount_status())
        .max_by_key(|status| badness(*status))
        .unwrap_or(AgentMountStatus::NoHelper)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::{
        internal_api_types::{MountedShare, SessionHelperState, ShareError, Shares},
        local_api_messages::APICurrentForegroundShare,
        messages::{AgentMountStatus, Share},
    };

    use super::{
        NO_HELPER_MESSAGE, OtherMount, ScopeView, ShareDestinationError, ShareMount,
        WAITING_MESSAGE, agent_mount_status, project_and_session, same_share,
    };

    fn share(id: &str) -> Share {
        Share {
            resource: None,
            share_id: id.to_owned(),
            share_name: "share".to_owned(),
            dest_path: "/project/session".to_owned(),
            created_timestamp: 0,
            username: "user".to_owned(),
            password: "password".to_owned(),
            uid: 1000,
            gid: 1000,
            created_by: None,
        }
    }

    #[test]
    fn project_and_session_uses_the_final_non_empty_components() {
        assert_eq!(
            project_and_session("/adhoc//project/session///"),
            Ok(("project".to_owned(), "session".to_owned()))
        );
    }

    #[test]
    fn project_and_session_reports_a_missing_session() {
        assert_eq!(
            project_and_session("///"),
            Err(ShareDestinationError::MissingSession {
                dest_path: "///".to_owned(),
            })
        );
    }

    #[test]
    fn project_and_session_reports_a_missing_project() {
        assert_eq!(
            project_and_session("/session/"),
            Err(ShareDestinationError::MissingProject {
                dest_path: "/session/".to_owned(),
            })
        );
    }

    fn helper(mounted: &[(&str, Option<&str>)], errors: &[(&str, &str)]) -> SessionHelperState {
        SessionHelperState {
            mounted_shares: mounted
                .iter()
                .map(|(share_name, path)| MountedShare {
                    share_name: (*share_name).to_owned(),
                    mount_path: path.map(PathBuf::from),
                })
                .collect(),
            errors: errors
                .iter()
                .map(|(share_id, message)| ShareError {
                    share_id: (*share_id).to_owned(),
                    message: (*message).to_owned(),
                })
                .collect(),
            reconciled_share_ids: None,
        }
    }

    /// The desired state with `share` as its only and foreground share
    fn only(share: Share) -> Shares {
        Shares {
            foreground_share_id: Some(share.share_id.clone()),
            shares: vec![share],
        }
    }

    fn foreground(
        desired: Option<&Shares>,
        helper: Option<&SessionHelperState>,
    ) -> Result<APICurrentForegroundShare, ShareDestinationError> {
        ScopeView::new(desired, helper).foreground()
    }

    fn unavailable(message: &str) -> APICurrentForegroundShare {
        APICurrentForegroundShare::Unavailable {
            share_id: "foreground".to_owned(),
            project: "project".to_owned(),
            session: "session".to_owned(),
            dest_path: "/project/session".to_owned(),
            message: message.to_owned(),
            created_by: None,
        }
    }

    #[test]
    fn no_desired_foreground_is_none_even_if_something_is_mounted() {
        let mounted = helper(&[("share", Some("/mnt/old"))], &[]);
        let no_foreground = Shares {
            foreground_share_id: None,
            shares: vec![share("a")],
        };
        let unknown_foreground = Shares {
            foreground_share_id: Some("missing".to_owned()),
            shares: vec![share("a")],
        };
        assert_eq!(foreground(None, None), Ok(APICurrentForegroundShare::None));
        // a share that is no longer desired is not shown, even while it is still mounted:
        assert_eq!(
            foreground(None, Some(&mounted)),
            Ok(APICurrentForegroundShare::None)
        );
        // the foreground share is looked up by ID
        for desired in [&no_foreground, &unknown_foreground] {
            assert_eq!(
                foreground(Some(desired), Some(&mounted)),
                Ok(APICurrentForegroundShare::None)
            );
        }
    }

    #[test]
    fn mounted_foreground_share_is_mounted() {
        let desired = only(share("foreground"));
        let report = helper(&[("share", Some("/mnt/session"))], &[]);
        assert_eq!(
            foreground(Some(&desired), Some(&report)),
            Ok(APICurrentForegroundShare::Mounted {
                share_id: "foreground".to_owned(),
                project: "project".to_owned(),
                session: "session".to_owned(),
                dest_path: "/project/session".to_owned(),
                mount_path: "/mnt/session".to_owned(),
                created_by: None,
            })
        );
        assert_eq!(
            ScopeView::new(Some(&desired), Some(&report)).foreground_mount_path(),
            Some(Path::new("/mnt/session"))
        );
    }

    #[test]
    fn the_projection_says_who_started_the_share() {
        let desired = only(Share {
            created_by: Some("alice".to_owned()),
            ..share("foreground")
        });
        let mounted = helper(&[("share", Some("/mnt/session"))], &[]);
        for helper in [None, Some(&mounted)] {
            let created_by = match foreground(Some(&desired), helper) {
                Ok(APICurrentForegroundShare::Mounted { created_by, .. })
                | Ok(APICurrentForegroundShare::Unavailable { created_by, .. }) => created_by,
                other => panic!("unexpected: {other:?}"),
            };
            assert_eq!(created_by.as_deref(), Some("alice"));
        }
    }

    fn named_share(id: &str, share_name: &str, session: &str) -> Share {
        Share {
            share_name: share_name.to_owned(),
            dest_path: format!("/project/{session}"),
            created_by: Some("alice".to_owned()),
            ..share(id)
        }
    }

    #[test]
    fn other_mounts_are_the_mounted_non_foreground_ones() {
        let desired = Shares {
            foreground_share_id: Some("new".to_owned()),
            shares: vec![
                named_share("old", "share-old", "first"),
                named_share("unmounted", "share-unmounted", "second"),
                named_share("new", "share-new", "third"),
            ],
        };
        let helper = helper(
            &[
                ("share-old", Some("/mnt/first")),
                ("share-unmounted", None),
                ("share-new", Some("/mnt/third")),
                ("share-not-desired", Some("/mnt/stale")),
            ],
            &[],
        );
        assert_eq!(
            ScopeView::new(Some(&desired), Some(&helper)).other_mounts(),
            vec![OtherMount {
                share_id: "old".to_owned(),
                project: "project".to_owned(),
                session: "first".to_owned(),
                created_by: Some("alice".to_owned()),
                mount_path: "/mnt/first".to_owned(),
            }]
        );
    }

    #[test]
    fn without_a_foreground_share_every_mounted_share_is_listed() {
        let desired = Shares {
            foreground_share_id: None,
            shares: vec![named_share("a", "share-a", "first")],
        };
        let helper = helper(&[("share-a", Some("/mnt/first"))], &[]);
        assert_eq!(
            ScopeView::new(Some(&desired), Some(&helper))
                .other_mounts()
                .len(),
            1
        );
    }

    #[test]
    fn without_helper_the_message_says_how_to_start_one() {
        let desired = only(share("foreground"));
        assert_eq!(
            foreground(Some(&desired), None),
            Ok(unavailable(NO_HELPER_MESSAGE))
        );
        let view = ScopeView::new(Some(&desired), None);
        assert_eq!(view.other_mounts(), vec![]);
        assert_eq!(view.foreground_mount_path(), None);
    }

    #[test]
    fn helper_error_for_the_share_is_the_message() {
        let desired = only(share("foreground"));
        let report = helper(
            &[],
            &[
                ("other", "unrelated"),
                ("foreground", "credential conflict"),
            ],
        );
        assert_eq!(
            foreground(Some(&desired), Some(&report)),
            Ok(unavailable("credential conflict"))
        );
    }

    #[test]
    fn not_yet_mounted_or_without_usable_path_is_waiting() {
        let desired = only(share("foreground"));
        for report in [
            helper(&[], &[]),
            helper(&[("other-share", Some("/mnt/other"))], &[]),
            // mounted, but without a path an application could use:
            helper(&[("share", None)], &[]),
        ] {
            assert_eq!(
                foreground(Some(&desired), Some(&report)),
                Ok(unavailable(WAITING_MESSAGE))
            );
        }
    }

    #[test]
    fn each_desired_share_is_mounted_failed_or_waiting() {
        let desired = Shares {
            foreground_share_id: None,
            shares: vec![
                named_share("mounted", "share-mounted", "first"),
                named_share("failed", "share-failed", "second"),
                named_share("waiting", "share-waiting", "third"),
            ],
        };
        let report = helper(
            &[
                ("share-mounted", Some("/mnt/first")),
                ("share-waiting", None),
                ("share-stale", Some("/mnt/stale")),
            ],
            &[("failed", "access denied")],
        );
        let view = ScopeView::new(Some(&desired), Some(&report));
        let states: Vec<_> = view
            .desired()
            .map(|(share, mount)| (share.share_id.as_str(), mount))
            .collect();
        assert_eq!(
            states,
            vec![
                ("mounted", ShareMount::Mounted(Path::new("/mnt/first"))),
                ("failed", ShareMount::Failed("access denied")),
                ("waiting", ShareMount::Waiting),
            ]
        );
        let stale: Vec<_> = view
            .stale_mounts()
            .map(|mounted| mounted.share_name.as_str())
            .collect();
        assert_eq!(stale, vec!["share-stale"]);

        let no_helper = ScopeView::new(Some(&desired), None);
        assert!(
            no_helper
                .desired()
                .all(|(_, mount)| mount == ShareMount::NoHelper)
        );
        assert_eq!(no_helper.stale_mounts().count(), 0);
    }

    /// The one matching rule, pinned for every question: a share reported under another case
    /// is the desired share, not a stale mount
    #[test]
    fn share_names_match_case_insensitively_everywhere() {
        for (desired_name, reported_name, same) in [
            ("foo", "foo", true),
            ("foo", "Foo", true),
            ("Foo", "foo", true),
            ("foo", "FOO", true),
            ("ingest-ÄÖ", "INGEST-äö", true),
            ("foo", "foo2", false),
            ("foo", "fo", false),
        ] {
            let case = format!("desired {desired_name:?}, reported {reported_name:?}");
            assert_eq!(same_share(desired_name, reported_name), same, "{case}");

            let foreground = named_share("fg", desired_name, "session");
            let other = named_share("other", &format!("{desired_name}-other"), "earlier");
            let desired = Shares {
                foreground_share_id: Some("fg".to_owned()),
                shares: vec![other, foreground],
            };
            let report = helper(
                &[
                    (reported_name, Some("/mnt/fg")),
                    (&format!("{reported_name}-OTHER"), Some("/mnt/other")),
                ],
                &[("fg", "failed")],
            );
            let view = ScopeView::new(Some(&desired), Some(&report));

            let projected_mounted = matches!(
                view.foreground(),
                Ok(APICurrentForegroundShare::Mounted { ref mount_path, .. })
                    if mount_path == "/mnt/fg"
            );
            assert_eq!(projected_mounted, same, "foreground: {case}");
            assert_eq!(
                view.foreground_mount_path(),
                same.then(|| Path::new("/mnt/fg")),
                "foreground path: {case}"
            );
            let other_mounts: Vec<_> = view
                .other_mounts()
                .into_iter()
                .map(|other| other.mount_path)
                .collect();
            assert_eq!(
                other_mounts,
                if same { vec!["/mnt/other"] } else { vec![] },
                "other mounts: {case}"
            );
            assert_eq!(view.stale_mounts().count() == 0, same, "stale: {case}");
            assert_eq!(view.hand_off_blocked(), !same, "hand-off: {case}");
            assert_eq!(view.actual_differs_from_desired(), !same, "differs: {case}");
        }
    }

    #[test]
    fn hand_off_is_blocked_by_a_stale_mount_and_a_failed_share() {
        let desired = only(share("new"));
        let stale = ("old-share", Some("Z:\\"));
        let current = ("SHARE", Some("Y:\\"));
        let error = ("new", "another user's drive is still connected");
        let blocked = |mounted: &[(&str, Option<&str>)], errors: &[(&str, &str)]| {
            ScopeView::new(Some(&desired), Some(&helper(mounted, errors))).hand_off_blocked()
        };

        assert!(blocked(&[stale], &[error]));
        // a stale mount alone is just not removed yet; an error alone has another cause
        assert!(!blocked(&[stale], &[]));
        assert!(!blocked(&[], &[error]));
        // share names are case-insensitive
        assert!(!blocked(&[current], &[]));
        // without a helper report, nothing is known to block
        assert!(!ScopeView::new(Some(&desired), None).hand_off_blocked());
    }

    #[test]
    fn differs_until_every_desired_share_has_a_path_and_nothing_else_is_mounted() {
        let desired = Shares {
            foreground_share_id: None,
            shares: vec![share("a")],
        };
        let differs = |desired: &Shares, mounted: &[(&str, Option<&str>)]| {
            ScopeView::new(Some(desired), Some(&helper(mounted, &[]))).actual_differs_from_desired()
        };

        // converged; share names are case-insensitive
        assert!(!differs(&desired, &[("SHARE", Some("/mnt/a"))]));
        // missing, or without a usable path
        assert!(differs(&desired, &[]));
        assert!(differs(&desired, &[("share", None)]));
        // a stale mount awaiting removal
        assert!(differs(
            &desired,
            &[("share", Some("/mnt/a")), ("old", Some("/mnt/b"))]
        ));
        // nothing desired, nothing mounted
        let nothing = Shares {
            foreground_share_id: None,
            shares: vec![],
        };
        assert!(!differs(&nothing, &[]));
        // without a report, actual state is unknown
        assert!(ScopeView::new(Some(&nothing), None).actual_differs_from_desired());
    }

    /// A helper state with share "a" (name "share") mounted, reconciled against `ids`
    fn reconciled(mounted: bool, ids: Option<&[&str]>) -> SessionHelperState {
        SessionHelperState {
            reconciled_share_ids: ids.map(|ids| ids.iter().map(|id| (*id).to_owned()).collect()),
            ..helper(
                if mounted {
                    &[("share", Some("/mnt/a"))]
                } else {
                    &[]
                },
                &[],
            )
        }
    }

    fn scope_status(desired: &Shares, helper: Option<&SessionHelperState>) -> AgentMountStatus {
        ScopeView::new(Some(desired), helper).mount_status()
    }

    #[test]
    fn scope_status_from_the_echoed_desired_state() {
        let desired = only(share("a"));
        // not reported yet
        assert_eq!(scope_status(&desired, None), AgentMountStatus::Reconciling);
        // reported for an older desired state, even if it happens to match
        assert_eq!(
            scope_status(&desired, Some(&reconciled(true, Some(&[])))),
            AgentMountStatus::Reconciling
        );
        // reported for the current desired state
        assert_eq!(
            scope_status(&desired, Some(&reconciled(true, Some(&["a"])))),
            AgentMountStatus::Idle
        );
        assert_eq!(
            scope_status(&desired, Some(&reconciled(false, Some(&["a"])))),
            AgentMountStatus::LeftoverWork
        );
    }

    #[test]
    fn scope_status_of_helpers_that_dont_echo() {
        let desired = only(share("a"));
        assert_eq!(
            scope_status(&desired, Some(&reconciled(true, None))),
            AgentMountStatus::Idle
        );
        // can't tell a pending pass from leftovers
        assert_eq!(
            scope_status(&desired, Some(&reconciled(false, None))),
            AgentMountStatus::Reconciling
        );
    }

    #[test]
    fn agent_status_is_the_worst_helper_status() {
        let desired = only(share("a"));
        let idle = reconciled(true, Some(&["a"]));
        let leftover = reconciled(false, Some(&["a"]));
        let status = |helpers: &[Option<&SessionHelperState>]| {
            agent_mount_status(
                helpers
                    .iter()
                    .map(|helper| ScopeView::new(Some(&desired), *helper)),
            )
        };
        assert_eq!(status(&[]), AgentMountStatus::NoHelper);
        assert_eq!(status(&[Some(&idle)]), AgentMountStatus::Idle);
        assert_eq!(status(&[Some(&idle), None]), AgentMountStatus::Reconciling);
        assert_eq!(
            status(&[None, Some(&leftover), Some(&idle)]),
            AgentMountStatus::LeftoverWork
        );
    }
}
