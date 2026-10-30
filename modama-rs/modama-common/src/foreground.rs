//! Where a client should write right now: the foreground share, projected from the desired
//! state and the session helper's report. Used by the agent for the HTTP API and by the GUI.

use log::warn;

use crate::{
    internal_api_types::{SessionHelperState, Shares},
    local_api_messages::APICurrentForegroundShare,
    messages::Share,
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

/// Shown when no session helper has reported for the caller's scope
pub const NO_HELPER_MESSAGE: &str = "No session helper is running for you. Start it with \
                                 `modama-agent session-helper`.";

/// Shown while the helper is there, but hasn't mounted the share (and hasn't reported an error)
pub const WAITING_MESSAGE: &str = "Waiting for the session helper to mount the share.";

/// Answer "where should I write right now?" from the desired foreground share and the
/// report of the caller's session helper (None if no helper has reported for that scope).
pub fn foreground_projection(
    desired: Option<&Share>,
    helper: Option<&SessionHelperState>,
) -> Result<APICurrentForegroundShare, ShareDestinationError> {
    let Some(desired) = desired else {
        return Ok(APICurrentForegroundShare::None);
    };
    let (project, session) = project_and_session(&desired.dest_path)?;
    let unavailable = |message: String| APICurrentForegroundShare::Unavailable {
        share_id: desired.share_id.clone(),
        project: project.clone(),
        session: session.clone(),
        dest_path: desired.dest_path.clone(),
        message,
    };

    let Some(helper) = helper else {
        return Ok(unavailable(NO_HELPER_MESSAGE.to_owned()));
    };

    // mounts are identified by share name, as the helper may report mounts we know nothing about:
    let mount_path = helper
        .mounted_shares
        .iter()
        .filter(|mounted| mounted.share_name == desired.share_name)
        .find_map(|mounted| mounted.mount_path.as_ref());
    if let Some(mount_path) = mount_path {
        return Ok(match mount_path.to_str() {
            Some(mount_path) => APICurrentForegroundShare::Mounted {
                share_id: desired.share_id.clone(),
                project,
                session,
                dest_path: desired.dest_path.clone(),
                mount_path: mount_path.to_owned(),
            },
            None => {
                let lossy = mount_path.to_string_lossy();
                warn!("Mount path {lossy} contains invalid unicode, cannot report it");
                unavailable(format!(
                    "The share is mounted at {lossy}, but that path can't be represented as \
                     text."
                ))
            }
        });
    }

    let message = helper
        .errors
        .iter()
        .find(|error| error.share_id == desired.share_id)
        .map_or_else(|| WAITING_MESSAGE.to_owned(), |error| error.message.clone());
    Ok(unavailable(message))
}

/// The desired foreground share; the stream only carries its ID
pub fn foreground_share(shares: &Shares) -> Option<&Share> {
    let id = shares.foreground_share_id.as_ref()?;
    shares.shares.iter().find(|share| &share.share_id == id)
}

/// A hand-off is blocked when a mount that is no longer desired is still there and a desired
/// share failed: on Windows, the old mount's credentials keep the new ones out (error 1219)
/// until it is released, by force if files are open on it.
pub fn hand_off_blocked(desired: &Shares, helper: &SessionHelperState) -> bool {
    let is_desired = |name: &str| {
        desired
            .shares
            .iter()
            .any(|share| share.share_name.eq_ignore_ascii_case(name))
    };
    let stale_mount = helper
        .mounted_shares
        .iter()
        .any(|mounted| !is_desired(&mounted.share_name));
    let failed_share = helper
        .errors
        .iter()
        .any(|error| desired.shares.iter().any(|s| s.share_id == error.share_id));
    stale_mount && failed_share
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{
        internal_api_types::{MountedShare, SessionHelperState, ShareError, Shares},
        local_api_messages::APICurrentForegroundShare,
        messages::Share,
    };

    use super::{
        NO_HELPER_MESSAGE, ShareDestinationError, WAITING_MESSAGE, foreground_projection,
        foreground_share, hand_off_blocked, project_and_session,
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
        }
    }

    fn unavailable(message: &str) -> APICurrentForegroundShare {
        APICurrentForegroundShare::Unavailable {
            share_id: "foreground".to_owned(),
            project: "project".to_owned(),
            session: "session".to_owned(),
            dest_path: "/project/session".to_owned(),
            message: message.to_owned(),
        }
    }

    #[test]
    fn no_desired_foreground_is_none_even_if_something_is_mounted() {
        assert_eq!(
            foreground_projection(None, None),
            Ok(APICurrentForegroundShare::None)
        );
        // a share that is no longer desired is not shown, even while it is still mounted:
        assert_eq!(
            foreground_projection(None, Some(&helper(&[("share", Some("/mnt/old"))], &[]))),
            Ok(APICurrentForegroundShare::None)
        );
    }

    #[test]
    fn mounted_foreground_share_is_mounted() {
        let desired = share("foreground");
        assert_eq!(
            foreground_projection(
                Some(&desired),
                Some(&helper(&[("share", Some("/mnt/session"))], &[]))
            ),
            Ok(APICurrentForegroundShare::Mounted {
                share_id: "foreground".to_owned(),
                project: "project".to_owned(),
                session: "session".to_owned(),
                dest_path: "/project/session".to_owned(),
                mount_path: "/mnt/session".to_owned(),
            })
        );
    }

    #[test]
    fn without_helper_the_message_says_how_to_start_one() {
        let desired = share("foreground");
        assert_eq!(
            foreground_projection(Some(&desired), None),
            Ok(unavailable(NO_HELPER_MESSAGE))
        );
    }

    #[test]
    fn helper_error_for_the_share_is_the_message() {
        let desired = share("foreground");
        let report = helper(
            &[],
            &[
                ("other", "unrelated"),
                ("foreground", "credential conflict"),
            ],
        );
        assert_eq!(
            foreground_projection(Some(&desired), Some(&report)),
            Ok(unavailable("credential conflict"))
        );
    }

    #[test]
    fn not_yet_mounted_or_without_usable_path_is_waiting() {
        let desired = share("foreground");
        for report in [
            helper(&[], &[]),
            helper(&[("other-share", Some("/mnt/other"))], &[]),
            // mounted, but without a path an application could use:
            helper(&[("share", None)], &[]),
        ] {
            assert_eq!(
                foreground_projection(Some(&desired), Some(&report)),
                Ok(unavailable(WAITING_MESSAGE))
            );
        }
    }

    #[test]
    fn foreground_share_is_looked_up_by_id() {
        let shares = |id: Option<&str>| Shares {
            foreground_share_id: id.map(str::to_owned),
            shares: vec![share("a"), share("b")],
        };
        assert_eq!(
            foreground_share(&shares(Some("b"))).map(|s| s.share_id.as_str()),
            Some("b")
        );
        assert_eq!(foreground_share(&shares(Some("missing"))), None);
        assert_eq!(foreground_share(&shares(None)), None);
    }

    #[test]
    fn hand_off_is_blocked_by_a_stale_mount_and_a_failed_share() {
        let desired = Shares {
            foreground_share_id: Some("new".to_owned()),
            shares: vec![share("new")],
        };
        let stale = MountedShare {
            share_name: "old-share".to_owned(),
            mount_path: Some(PathBuf::from("Z:\\")),
        };
        let current = MountedShare {
            share_name: "SHARE".to_owned(),
            mount_path: Some(PathBuf::from("Y:\\")),
        };
        let error = ShareError {
            share_id: "new".to_owned(),
            message: "another user's drive is still connected".to_owned(),
        };
        let state =
            |mounted_shares: Vec<MountedShare>, errors: Vec<ShareError>| SessionHelperState {
                mounted_shares,
                errors,
            };

        assert!(hand_off_blocked(
            &desired,
            &state(vec![stale.clone()], vec![error.clone()])
        ));
        // a stale mount alone is just not removed yet; an error alone has another cause
        assert!(!hand_off_blocked(&desired, &state(vec![stale], vec![])));
        assert!(!hand_off_blocked(&desired, &state(vec![], vec![error])));
        // share names are case-insensitive
        assert!(!hand_off_blocked(&desired, &state(vec![current], vec![])));
    }
}
