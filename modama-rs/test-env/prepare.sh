#!/bin/bash
set -ex

if [ ! -e ./certs/ ]; then

    cargo run -r --bin ca -- --ca-path=./certs/ --insecure-no-password initialize-ca

    cargo run -r --bin ca -- --ca-path=./certs/ --insecure-no-password enroll-server smbdrop smbdrop
    openssl verify -verify_hostname smbdrop -CAfile certs/ca/pub/ca.cert.pem certs/servers/smbdrop/smbdrop.cert.pem

    cargo run -r --bin ca -- --ca-path=./certs/ --insecure-no-password enroll-agent agent1 testresource
    openssl verify -CAfile certs/ca/pub/ca.cert.pem certs/clients/testresource/agent1.cert.pem

    cargo run -r --bin ca -- --ca-path=./certs/ --insecure-no-password enroll-agent agent2 testresource
    openssl verify -CAfile certs/ca/pub/ca.cert.pem certs/clients/testresource/agent2.cert.pem

    cargo run -r --bin ca -- --ca-path=./certs/ --insecure-no-password generate-agent-config \
      --listen-local-api=0.0.0.0:3003 \
      --event-url=wss://smbdrop:5052 \
      --token-service-url=https://smbdrop:5053 \
      --mgmt-api-url=http://mgmtinterface-api:8000 \
      --smb-hostname=smbdrop \
      --smb-port=445 \
      --smb-domain=ER-C-Ingest \
      --agent-name=agent1 \
      --resource-name=testresource > certs/agent1.toml

    cargo run -r --bin ca -- --ca-path=./certs/ --insecure-no-password generate-agent-config \
      --listen-local-api=0.0.0.0:3003 \
      --event-url=wss://smbdrop:5052 \
      --token-service-url=https://smbdrop:5053 \
      --mgmt-api-url=http://mgmtinterface-api:8000 \
      --smb-hostname=smbdrop \
      --smb-port=445 \
      --smb-domain=ER-C-Ingest \
      --agent-name=agent2 \
      --resource-name=testresource > certs/agent2.toml

    cargo run -r --bin ca -- --ca-path=./certs/ --insecure-no-password generate-agent-config \
      --listen-local-api=0.0.0.0:3003 \
      --event-url=wss://smbdrop:5052 \
      --token-service-url=https://smbdrop:5053 \
      --mgmt-api-url=http://mgmtinterface-api:8000 \
      --smb-hostname=smbdrop \
      --smb-port=445 \
      --smb-domain=ER-C-Ingest > certs/agent-untrusted.toml

fi

find certs/
