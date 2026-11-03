//! Client for mgmtinterface-api, generated at build time from the committed
//! `python/mgmtinterface-api/openapi.json` (see `build.rs`).

#[allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]
pub mod generated {
    pub mod types {
        include!(concat!(env!("OUT_DIR"), "/generated/types.rs"));
    }

    pub mod client {
        include!(concat!(env!("OUT_DIR"), "/generated/client.rs"));
    }

    pub mod info {
        include!(concat!(env!("OUT_DIR"), "/generated/info.rs"));
    }

    pub use client::*;
    pub use info::*;
    pub use types::*;
}
