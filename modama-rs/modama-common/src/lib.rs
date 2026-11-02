pub mod agent;
pub mod ca;
pub mod cert_helpers;
pub mod config;
pub mod foreground;
pub mod internal_api_types;
pub mod local_api_messages;
pub mod logging;
pub mod messages;
pub mod mount_popup;
pub mod requests;
pub mod runtime;
pub mod test_helpers;
pub mod tokens;
pub mod version;
#[cfg(windows)]
pub mod windows_folders;

/// Install the workspace's selected rustls provider before constructing clients that defer
/// provider selection to the application.
pub fn install_rustls_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// A reqwest [`ClientBuilder`](reqwest::ClientBuilder) that trusts *only* `ca`.
///
/// System and built-in roots are not used. This matters beyond trust hygiene: since reqwest
/// 0.13, adding a root without `tls_certs_only` selects rustls-platform-verifier, which on
/// Windows verifies through CryptoAPI and rejects our private-CA chains.
pub fn pinned_ca_client_builder(ca: reqwest::Certificate) -> reqwest::ClientBuilder {
    install_rustls_crypto_provider();
    reqwest::ClientBuilder::new().tls_certs_only([ca])
}
