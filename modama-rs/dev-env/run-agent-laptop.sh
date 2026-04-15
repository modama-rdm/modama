#!/bin/bash
cargo build -r --bin modama-agent && sudo ../target/release/modama-agent --chain certs/clients/laptop/dev.chain.pem --key certs/clients/laptop/dev.priv.pem --ca-cert certs/ca/ca.cert.pem  --listen-local-api localhost:3003 --smb-hostname localhost --smb-port 1446
