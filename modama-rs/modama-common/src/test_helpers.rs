#![allow(clippy::unwrap_used)]
use std::{
    env::temp_dir,
    ops::Deref,
    path::{Path, PathBuf},
    sync::Arc,
};

use rand::{RngExt, distr::Alphanumeric};

use crate::tokens::{BoxFutureToken, Claims, LocalTokenSigner, TokenProvider};

pub struct TempDir {
    pub path: PathBuf,
}

impl TempDir {
    pub fn new(prefix: &str) -> Self {
        let tmp_base = temp_dir();
        loop {
            let rnd: String = rand::rng()
                .sample_iter(&Alphanumeric)
                .take(15)
                .map(char::from)
                .collect();
            let rand_name = tmp_base.join(format!("{}-tmp{}", prefix, rnd));
            if !rand_name.exists() {
                std::fs::create_dir(&rand_name).unwrap();
                return Self { path: rand_name };
            }
        }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.path).unwrap();
    }
}

impl Deref for TempDir {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.path
    }
}

#[derive(Debug)]
pub struct MockTokenProvider {
    pub signer: LocalTokenSigner,
    pub claims: Claims,
}

impl TokenProvider for MockTokenProvider {
    fn fetch_token(self: Arc<Self>) -> BoxFutureToken {
        Box::pin(async move { Ok(self.signer.sign(&self.claims).unwrap()) })
    }
}
