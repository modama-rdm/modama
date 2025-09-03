#!/bin/bash
cargo build -r --bin modama-agent && sudo ../target/release/modama-agent --chain certs/clients/holo/k2.chain.pem --key certs/clients/holo/k2.priv.pem --ca-cert certs/ca/ca.cert.pem  --listen-local-api localhost:3004 --smb-hostname localhost --smb-port 1446
