#![allow(clippy::expect_used)]

use std::{env, path::PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR must be set by Cargo"));
    openapi_codegen::generate_api_client(
        &manifest_dir.join("../../python/mgmtinterface-api/openapi.json"),
        &out_dir,
        Some("/mgmt-api"),
    )
    .expect("failed to generate the mgmtinterface-api client");
}
