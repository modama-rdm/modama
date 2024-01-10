use std::path::PathBuf;

use modama_common::tokens::{TokenFetcher, TokenVerifier};

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

    let token = token_fetcher.fetch_token().await.unwrap();
    eprintln!("{}", &token);
    eprintln!("{:?}", token_verifier.verify(&token).await.unwrap());
}
