use git_version::git_version;

pub fn modama_git_version() -> &'static str {
    git_version!()
}
