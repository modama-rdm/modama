//! Where smbdrop finds the data tree, and how canonical paths map into it.
//!
//! Callers name a folder by its canonical path, like `adhoc/testproject/session2`; smbdrop
//! shares the folder at that path under its data root, which is where the data tree is
//! mounted in smbdrop's container (`/data` by default).

use std::path::{Component, Path, PathBuf};

/// Why a canonical path was refused
#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub enum CanonicalPathError {
    #[error("the canonical path {0:?} is absolute; leave out the leading `/`")]
    Absolute(String),

    #[error("the canonical path {0:?} contains `..`, which could leave the data root")]
    ParentComponent(String),

    #[error("the canonical path is empty")]
    Empty,
}

/// The folder smbdrop's shares lie in
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataRoot(PathBuf);

impl DataRoot {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// `canonical_path` with empty and `.` components dropped, after checking that it is
    /// relative and stays inside the data root
    pub fn normalize(canonical_path: &str) -> Result<String, CanonicalPathError> {
        if canonical_path.starts_with('/') {
            return Err(CanonicalPathError::Absolute(canonical_path.to_owned()));
        }
        let mut components = Vec::new();
        for component in canonical_path.split('/') {
            match component {
                "" | "." => {}
                ".." => {
                    return Err(CanonicalPathError::ParentComponent(
                        canonical_path.to_owned(),
                    ));
                }
                component => components.push(component),
            }
        }
        if components.is_empty() {
            return Err(CanonicalPathError::Empty);
        }
        Ok(components.join("/"))
    }

    /// The folder at `canonical_path`, which must be normalized
    pub fn folder(&self, canonical_path: &str) -> PathBuf {
        self.0.join(canonical_path)
    }

    /// The canonical path of `dest_path`, the folder's full path under the data root; None if
    /// it lies outside of the data root or names the data root itself
    pub fn canonical_path_of(&self, dest_path: &Path) -> Option<String> {
        let relative = dest_path.strip_prefix(&self.0).ok()?;
        let mut components = Vec::new();
        for component in relative.components() {
            match component {
                Component::Normal(name) => components.push(name.to_str()?),
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
            }
        }
        if components.is_empty() {
            return None;
        }
        Some(components.join("/"))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{CanonicalPathError, DataRoot};

    #[test]
    fn normalize_drops_empty_and_dot_components() {
        assert_eq!(
            DataRoot::normalize("adhoc//project/./session/"),
            Ok("adhoc/project/session".to_owned())
        );
    }

    #[test]
    fn normalize_refuses_paths_that_leave_the_data_root() {
        assert_eq!(
            DataRoot::normalize("/adhoc/project/session"),
            Err(CanonicalPathError::Absolute(
                "/adhoc/project/session".to_owned()
            ))
        );
        assert_eq!(
            DataRoot::normalize("adhoc/../../etc"),
            Err(CanonicalPathError::ParentComponent(
                "adhoc/../../etc".to_owned()
            ))
        );
        assert_eq!(DataRoot::normalize(""), Err(CanonicalPathError::Empty));
        assert_eq!(DataRoot::normalize("./"), Err(CanonicalPathError::Empty));
    }

    #[test]
    fn folder_is_under_the_data_root() {
        let root = DataRoot::new("/data");
        assert_eq!(
            root.folder("adhoc/project/session"),
            Path::new("/data/adhoc/project/session")
        );
    }

    #[test]
    fn canonical_path_of_strips_the_data_root() {
        let root = DataRoot::new("/data");
        assert_eq!(
            root.canonical_path_of(Path::new("/data/adhoc/project/session/")),
            Some("adhoc/project/session".to_owned())
        );
        assert_eq!(
            root.canonical_path_of(Path::new("/data/./adhoc/session")),
            Some("adhoc/session".to_owned())
        );
    }

    #[test]
    fn canonical_path_of_is_none_outside_the_data_root() {
        let root = DataRoot::new("/data");
        assert_eq!(
            root.canonical_path_of(Path::new("/srv/adhoc/session")),
            None
        );
        assert_eq!(root.canonical_path_of(Path::new("/database/session")), None);
        assert_eq!(root.canonical_path_of(Path::new("/data")), None);
        assert_eq!(root.canonical_path_of(Path::new("/data/adhoc/../..")), None);
    }
}
