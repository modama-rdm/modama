//! Asserts that clients built with `pinned_ca_client_builder` trust only the given CA, and never
//! the system roots.
//!
//! On Linux, rustls-platform-verifier gets the system roots from rustls-native-certs, which reads
//! *only* `SSL_CERT_FILE` when that is set. Pointing it at a second test CA turns that CA into the
//! whole "system" store. The test sets a process-wide env var, so it lives in its own
//! integration-test binary (its own process).
//!
//! Windows can't be faked this way (it would need the real system store), so it is checked by hand.
#![cfg(target_os = "linux")]

use std::{error::Error, sync::Arc, time::Duration};

use modama_common::{ca::Ca, pinned_ca_client_builder, test_helpers::TempDir};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, pem::PemObject};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_rustls::TlsAcceptor;

/// Minimal HTTPS server answering every request with `200 ok`, presenting a `localhost` cert
/// signed by `ca`.
async fn spawn_server(ca: &Ca) -> Result<u16, Box<dyn Error>> {
    let server = ca.server_cert(&["localhost".to_owned()])?;
    let chain = vec![
        CertificateDer::from(server.cert.der().to_vec()),
        CertificateDer::from_pem_slice(ca.get_ca_cert_pem().as_bytes())?,
    ];
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(server.key_pair.serialize_der()));
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(chain, key)?;
    let acceptor = TlsAcceptor::from(Arc::new(config));

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                // A failed handshake is the expected outcome for the pinned client.
                let Ok(mut tls) = acceptor.accept(stream).await else {
                    return;
                };
                let mut buf = [0u8; 4096];
                let _ = tls.read(&mut buf).await;
                let _ = tls
                    .write_all(
                        b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok",
                    )
                    .await;
                let _ = tls.shutdown().await;
            });
        }
    });
    Ok(port)
}

#[test]
fn pinned_client_ignores_system_roots() {
    modama_common::install_rustls_crypto_provider();

    let ours = Ca::new(None::<&str>, "unused").expect("our ca");
    let system = Ca::new(None::<&str>, "unused").expect("system ca");

    let tmpdir = TempDir::new("pinned-ca-test");
    let system_roots = tmpdir.join("system-roots.pem");
    std::fs::write(&system_roots, system.get_ca_cert_pem()).expect("write system roots");
    // SAFETY: no other threads exist yet; the tokio runtime is only created below.
    unsafe {
        std::env::set_var("SSL_CERT_FILE", &system_roots);
        std::env::remove_var("SSL_CERT_DIR");
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        // The server's cert is signed by the "system" CA, not by ours.
        let port = spawn_server(&system).await.expect("test server");
        let url = format!("https://localhost:{port}/");
        let our_cert =
            reqwest::Certificate::from_pem(ours.get_ca_cert_pem().as_bytes()).expect("our cert");

        // Control: a default client (system roots only) does trust the fake system root.
        // Without this, the assertion below would also pass if the env var were simply ignored.
        let system_only = reqwest::ClientBuilder::new()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("system-roots client");
        let resp = system_only
            .get(&url)
            .send()
            .await
            .expect("control: default client must trust the SSL_CERT_FILE root");
        assert_eq!(resp.status(), 200);

        // The pinned client must not trust the system root.
        let pinned = pinned_ca_client_builder(our_cert)
            .timeout(Duration::from_secs(10))
            .build()
            .expect("pinned client");
        let err = pinned
            .get(&url)
            .send()
            .await
            .expect_err("pinned client must reject a cert from a system-trusted CA");
        assert!(err.is_connect(), "expected TLS/connect error, got {err:?}");
        // Both CAs come from `Ca::new` and share a subject DN, so webpki matches our CA by name
        // and reports `BadSignature` instead of `UnknownIssuer`. Either means the server cert
        // was rejected.
        let msg = format!("{err:?}");
        assert!(
            msg.contains("InvalidCertificate"),
            "expected a certificate verification error, got {msg}"
        );
    });
}
