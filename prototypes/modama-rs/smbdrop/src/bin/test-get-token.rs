use std::{
    fs::File,
    io::Read,
};

use reqwest::{Identity, Certificate};


#[tokio::main]
async fn main() {
    let sign_url = "https://localhost:5053/sign/";
    let mut id_pem: Vec<u8> = Vec::new();
    let mut key_file = File::open("./certs/clients/holo/dld.priv.pem").unwrap();
    key_file.read_to_end(&mut id_pem).unwrap();
    let mut cert_file = File::open("./certs/clients/holo/dld.chain.pem").unwrap();
    cert_file.read_to_end(&mut id_pem).unwrap();

    let mut ca_certs = File::open("./certs/ca/ca.cert.pem").unwrap();
    let mut ca_pem: Vec<u8> = Vec::new();
    ca_certs.read_to_end(&mut ca_pem).unwrap();
    let root_cert = Certificate::from_pem(&ca_pem).unwrap();

    let identity = Identity::from_pem(&id_pem).unwrap();
    let client = reqwest::ClientBuilder::new()
        .identity(identity)
        .add_root_certificate(root_cert)
        .build()
        .unwrap();

    let resp = client.put(sign_url).send().await.unwrap();

    let token = String::from_utf8(resp.bytes().await.unwrap().iter().cloned().collect()).unwrap();
    eprintln!("{}", &token);
    let resp_verify = client.put("https://localhost:5053/verify/").body(token.clone()).send().await.unwrap();
    let resp_verify_doc = String::from_utf8(resp_verify.bytes().await.unwrap().iter().cloned().collect()).unwrap();
    eprintln!("{}", &resp_verify_doc);
}
