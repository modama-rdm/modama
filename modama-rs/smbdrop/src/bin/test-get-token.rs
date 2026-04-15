use std::path::PathBuf;

use modama_common::tokens::{TokenFetcher, TokenVerifier};

#[allow(clippy::unwrap_used)]  // don't care in this test tool
#[tokio::main]
async fn main() {
    let token_fetcher = TokenFetcher::new(
        "https://localhost:5053/sign/",
        &PathBuf::from("./certs/clients/holo/dld.priv.pem"),
        &PathBuf::from("./certs/clients/holo/dld.chain.pem"),
        &PathBuf::from("./certs/ca/ca.cert.pem"),
    )
    .unwrap();

    let token_verifier = TokenVerifier::new(
        "https://localhost:5053/verify/",
        &PathBuf::from("./certs/ca/ca.cert.pem"),
    )
    .unwrap();

    let sign_response = token_fetcher.fetch_token().await.unwrap();
    eprintln!("{:?}", &sign_response);
    eprintln!("{:?}", token_verifier.verify(&sign_response.token).await.unwrap());
}
