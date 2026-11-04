//! One reconciliation pass, the same on every platform, over a platform's [`MountTable`]: list
//! our mounts, remove what isn't desired, add what's missing, list again and report.
//!
//! The tables (WNet drive mappings on Windows, GVfs mounts on Linux) decide which mounts are
//! ours, how a share is added and how errors read; where the platforms need different
//! behaviour from the pass itself, the table says so with its [`Policy`].

use std::{collections::HashMap, path::PathBuf};

use log::{debug, warn};
use modama_common::{
    foreground::same_share,
    internal_api_types::{MountedShare, SessionHelperState, ShareError},
    messages::Share,
};

use crate::mount_backend::{EnumerationError, PassResult, unique_by_name};

/// One of our mounts, as a [`MountTable`] lists it
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableEntry<H> {
    pub share_name: String,
    /// `None`: it exists, but offers no path an ordinary application can use
    pub path: Option<PathBuf>,
    /// How the table refers to it for removal
    pub handle: H,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddFailure {
    /// Someone else mounted it since the inventory: not an error, but not ours to announce;
    /// the second inventory decides whether it is there
    AlreadyMounted,
    /// The message for the report: no URIs, never credentials
    Failed(String),
}

/// What a pass does with a desired share's entry that offers no path
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathlessEntries {
    /// Remove it and add the share again, as if it weren't there
    Remove,
    /// Keep it, and report the share with this error
    KeepWithError(&'static str),
}

/// What a pass does with a second entry of the same desired share
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicateEntries {
    RemoveAllButOne,
    Keep,
}

/// Where the platforms need the pass to behave differently
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    pub pathless: PathlessEntries,
    pub duplicates: DuplicateEntries,
}

/// The mounts one platform manages, from the configured host
pub trait MountTable {
    type Handle;

    fn policy(&self) -> Policy;
    /// Our mounts, and only ours; an error means nothing is known, so nothing is changed
    fn entries(&mut self) -> Result<Vec<TableEntry<Self::Handle>>, String>;
    /// With `force`, even if files are open on it
    fn remove(&mut self, entry: &TableEntry<Self::Handle>, force: bool) -> Result<(), String>;
    fn add(&mut self, share: &Share) -> Result<(), AddFailure>;
}

/// One reconciliation pass (see [`crate::mount_backend::MountBackend::reconcile`])
pub fn reconcile_pass<T: MountTable>(
    table: &mut T,
    shares: &[Share],
    force: bool,
) -> Result<PassResult, EnumerationError> {
    let policy = table.policy();
    let before = table.entries().map_err(EnumerationError)?;
    let (desired, mut errors) = unique_by_name(shares);
    let is_desired = |name: &str| desired.iter().any(|s| same_share(&s.share_name, name));

    // removal first: frees resources (on Windows, the credentials that would block our mappings)
    let mut kept: Vec<&str> = Vec::new();
    for entry in &before {
        let name = entry.share_name.as_str();
        let usable = entry.path.is_some() || policy.pathless != PathlessEntries::Remove;
        let duplicate = policy.duplicates == DuplicateEntries::RemoveAllButOne
            && kept.iter().any(|k| same_share(k, name));
        if is_desired(name) && usable && !duplicate {
            kept.push(name);
            continue;
        }
        debug!("removing share {name}");
        if let Err(e) = table.remove(entry, force) {
            // it stays in the report, so the scope reads as not converged; the next pass retries
            warn!("could not remove share {name}: {e}");
        }
    }

    let mut add_errors = HashMap::new();
    let mut created = Vec::new();
    for share in &desired {
        if kept.iter().any(|k| same_share(k, &share.share_name)) {
            continue;
        }
        debug!("adding share {}", share.share_name);
        match table.add(share) {
            Ok(()) => created.push(share.share_name.clone()),
            Err(AddFailure::AlreadyMounted) => {}
            Err(AddFailure::Failed(message)) => {
                add_errors.insert(share.share_id.clone(), message);
            }
        }
    }

    let after = table.entries().map_err(EnumerationError)?;
    for share in &desired {
        let entries: Vec<_> = after
            .iter()
            .filter(|e| same_share(&e.share_name, &share.share_name))
            .collect();
        if entries.iter().any(|e| e.path.is_some()) {
            continue;
        }
        let message = match policy.pathless {
            PathlessEntries::KeepWithError(message) if !entries.is_empty() => message.to_owned(),
            _ => add_errors
                .remove(&share.share_id)
                .unwrap_or_else(|| "not mounted".to_owned()),
        };
        errors.push(ShareError {
            share_id: share.share_id.clone(),
            message,
        });
    }
    Ok(PassResult {
        state: SessionHelperState {
            mounted_shares: after
                .into_iter()
                .map(|entry| MountedShare {
                    share_name: entry.share_name,
                    mount_path: entry.path,
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
pub(crate) mod tests {
    use super::*;

    pub(crate) fn share(id: &str, name: &str) -> Share {
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

    /// `(share name, path)` of every mount in the report, sorted
    pub(crate) fn mounted(state: &SessionHelperState) -> Vec<(String, Option<String>)> {
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

    pub(crate) fn entry(name: &str, path: Option<&str>) -> (String, Option<String>) {
        (name.to_owned(), path.map(str::to_owned))
    }

    fn errors(state: &SessionHelperState) -> Vec<(&str, &str)> {
        state
            .errors
            .iter()
            .map(|e| (e.share_id.as_str(), e.message.as_str()))
            .collect()
    }

    const KEEP_ALL: Policy = Policy {
        pathless: PathlessEntries::KeepWithError("no path"),
        duplicates: DuplicateEntries::Keep,
    };

    /// Entries with a unique id (the handle); adding mounts at `/mnt/<name>`. Calls and
    /// scripted removal errors name an entry as `name@path` (`-` for no path).
    struct MemoryTable {
        policy: Policy,
        entries: Vec<(usize, String, Option<PathBuf>)>,
        next_id: usize,
        inventory_error: bool,
        /// as `name@path`
        remove_errors: Vec<String>,
        /// by share name
        add_failures: HashMap<String, AddFailure>,
        /// share names that are added without a path
        add_pathless: Vec<String>,
        calls: Vec<String>,
    }

    impl MemoryTable {
        fn new(policy: Policy, entries: &[(&str, Option<&str>)]) -> Self {
            Self {
                policy,
                entries: entries
                    .iter()
                    .enumerate()
                    .map(|(id, (name, path))| (id, (*name).to_owned(), path.map(PathBuf::from)))
                    .collect(),
                next_id: entries.len(),
                inventory_error: false,
                remove_errors: Vec::new(),
                add_failures: HashMap::new(),
                add_pathless: Vec::new(),
                calls: Vec::new(),
            }
        }

        fn pass(&mut self, shares: &[Share]) -> SessionHelperState {
            reconcile_pass(self, shares, false).unwrap().state
        }
    }

    impl MountTable for MemoryTable {
        type Handle = usize;

        fn policy(&self) -> Policy {
            self.policy
        }

        fn entries(&mut self) -> Result<Vec<TableEntry<usize>>, String> {
            if self.inventory_error {
                return Err("no inventory".to_owned());
            }
            Ok(self
                .entries
                .iter()
                .map(|(id, name, path)| TableEntry {
                    share_name: name.clone(),
                    path: path.clone(),
                    handle: *id,
                })
                .collect())
        }

        fn remove(&mut self, entry: &TableEntry<usize>, force: bool) -> Result<(), String> {
            let label = format!("{}@{}", entry.share_name, path_str(entry.path.as_ref()));
            let forced = if force { " (forced)" } else { "" };
            self.calls.push(format!("remove {label}{forced}"));
            if self.remove_errors.contains(&label) {
                return Err("busy".to_owned());
            }
            self.entries.retain(|(id, _, _)| *id != entry.handle);
            Ok(())
        }

        fn add(&mut self, share: &Share) -> Result<(), AddFailure> {
            let name = &share.share_name;
            self.calls.push(format!("add {name}"));
            if let Some(failure) = self.add_failures.remove(name) {
                return Err(failure);
            }
            let path =
                (!self.add_pathless.contains(name)).then(|| PathBuf::from(format!("/mnt/{name}")));
            self.entries.push((self.next_id, name.clone(), path));
            self.next_id += 1;
            Ok(())
        }
    }

    fn path_str(path: Option<&PathBuf>) -> String {
        path.map_or_else(|| "-".to_owned(), |p| p.display().to_string())
    }

    #[test]
    fn removes_undesired_before_adding_missing_and_keeps_desired() {
        let mut table = MemoryTable::new(KEEP_ALL, &[("keep", Some("/k")), ("stale", Some("/s"))]);
        let state = table.pass(&[share("1", "new"), share("2", "KEEP")]);
        assert_eq!(table.calls, vec!["remove stale@/s", "add new"]);
        assert_eq!(
            mounted(&state),
            vec![entry("keep", Some("/k")), entry("new", Some("/mnt/new"))]
        );
        assert!(state.errors.is_empty());
    }

    #[test]
    fn inventory_error_fails_the_pass_and_changes_nothing() {
        let mut table = MemoryTable::new(KEEP_ALL, &[("stale", Some("/s"))]);
        table.inventory_error = true;
        assert!(reconcile_pass(&mut table, &[share("1", "a")], false).is_err());
        assert!(table.calls.is_empty());
    }

    #[test]
    fn force_applies_to_removals() {
        for force in [false, true] {
            let mut table = MemoryTable::new(KEEP_ALL, &[("old", Some("/o"))]);
            reconcile_pass(&mut table, &[], force).unwrap();
            let expected = if force {
                "remove old@/o (forced)"
            } else {
                "remove old@/o"
            };
            assert_eq!(table.calls, vec![expected]);
        }
    }

    #[test]
    fn created_lists_only_what_this_pass_added() {
        let mut table = MemoryTable::new(KEEP_ALL, &[("keep", Some("/k"))]);
        // someone else mounted "raced" first: not ours to announce
        table
            .add_failures
            .insert("raced".to_owned(), AddFailure::AlreadyMounted);
        let result = reconcile_pass(
            &mut table,
            &[share("1", "keep"), share("2", "new"), share("3", "raced")],
            false,
        )
        .unwrap();
        assert_eq!(result.created, vec!["new"]);
    }

    #[test]
    fn duplicate_desired_names_get_an_error() {
        let mut table = MemoryTable::new(KEEP_ALL, &[]);
        let state = table.pass(&[share("1", "a"), share("2", "A")]);
        assert_eq!(table.calls, vec!["add a"]);
        assert_eq!(
            errors(&state),
            vec![("2", "another desired share has the same name")]
        );
    }

    #[test]
    fn failed_removal_stays_in_the_report() {
        let mut table = MemoryTable::new(KEEP_ALL, &[("busy", Some("/b")), ("gone", None)]);
        table.remove_errors = vec!["busy@/b".to_owned(), "gone@-".to_owned()];
        let state = table.pass(&[]);
        assert_eq!(
            mounted(&state),
            vec![entry("busy", Some("/b")), entry("gone", None)]
        );
    }

    #[test]
    fn add_failures_are_reported_per_share() {
        let mut table = MemoryTable::new(KEEP_ALL, &[]);
        table.add_failures = HashMap::from([
            (
                "a".to_owned(),
                AddFailure::Failed("mount failed: no".to_owned()),
            ),
            ("b".to_owned(), AddFailure::AlreadyMounted),
        ]);
        let state = table.pass(&[share("1", "a"), share("2", "b"), share("3", "c")]);
        // "b" was never actually there: the second inventory decides
        assert_eq!(
            errors(&state),
            vec![("1", "mount failed: no"), ("2", "not mounted")]
        );
        assert_eq!(mounted(&state), vec![entry("c", Some("/mnt/c"))]);
    }

    #[test]
    fn keep_policy_keeps_pathless_and_duplicate_entries_and_reports_them() {
        let mut table = MemoryTable::new(KEEP_ALL, &[("a", None), ("A", Some("/a")), ("b", None)]);
        let state = table.pass(&[share("1", "a"), share("2", "b")]);
        assert!(table.calls.is_empty());
        // one usable entry is enough for "a"
        assert_eq!(errors(&state), vec![("2", "no path")]);
        assert_eq!(mounted(&state).len(), 3);
    }

    #[test]
    fn keep_policy_reports_a_share_added_without_path() {
        let mut table = MemoryTable::new(KEEP_ALL, &[]);
        table.add_pathless = vec!["a".to_owned()];
        let state = table.pass(&[share("1", "a")]);
        assert_eq!(errors(&state), vec![("1", "no path")]);
        assert_eq!(mounted(&state), vec![entry("a", None)]);
    }

    const REMOVE_ALL: Policy = Policy {
        pathless: PathlessEntries::Remove,
        duplicates: DuplicateEntries::RemoveAllButOne,
    };

    #[test]
    fn remove_policy_keeps_one_usable_entry_per_share() {
        let mut table = MemoryTable::new(
            REMOVE_ALL,
            &[
                ("a", None),
                ("a", Some("/a1")),
                ("A", Some("/a2")),
                ("b", None),
            ],
        );
        let state = table.pass(&[share("1", "a"), share("2", "b")]);
        assert_eq!(
            table.calls,
            vec!["remove a@-", "remove A@/a2", "remove b@-", "add b"]
        );
        assert_eq!(
            mounted(&state),
            vec![entry("a", Some("/a1")), entry("b", Some("/mnt/b"))]
        );
        assert!(state.errors.is_empty());
    }

    #[test]
    fn removing_a_duplicate_keeps_its_identical_twin() {
        let mut table = MemoryTable::new(REMOVE_ALL, &[("a", Some("/a")), ("a", Some("/a"))]);
        let state = table.pass(&[share("1", "a")]);
        assert_eq!(table.calls, vec!["remove a@/a"]);
        assert_eq!(mounted(&state), vec![entry("a", Some("/a"))]);
        assert!(state.errors.is_empty());
    }

    #[test]
    fn remove_policy_does_not_count_a_pathless_entry_as_mounted() {
        let mut table = MemoryTable::new(REMOVE_ALL, &[("a", None)]);
        table.remove_errors = vec!["a@-".to_owned()];
        table
            .add_failures
            .insert("a".to_owned(), AddFailure::Failed("conflict".to_owned()));
        let state = table.pass(&[share("1", "a")]);
        assert_eq!(errors(&state), vec![("1", "conflict")]);
        assert_eq!(mounted(&state), vec![entry("a", None)]);
    }
}
