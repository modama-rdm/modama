use git_version::git_version;

/// `git describe` of the build, like `v0.8.0-4-gb32bc77`.
///
/// `--tags` because release tags are lightweight, and a plain `git describe` only considers
/// annotated tags. The other arguments are git-version's defaults.
pub fn modama_git_version() -> &'static str {
    git_version!(
        args = ["--always", "--dirty=-modified", "--tags"],
        fallback = "unknown"
    )
}
