pub mod agent;
pub mod ca;
pub mod cert_helpers;
pub mod config;
pub mod local_api_messages;
pub mod logging;
pub mod messages;
pub mod mgmt_client;
pub mod requests;
pub mod test_helpers;
pub mod tokens;
pub mod version;
pub mod internal_api_types;

/// Install the workspace's selected rustls provider before constructing clients that defer
/// provider selection to the application.
pub fn install_rustls_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}
