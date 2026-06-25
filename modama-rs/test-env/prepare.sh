#!/bin/bash
set -ex

rm -rf certs/

cargo run -r --bin ca -- --ca-path=./certs/ initialize-ca

cargo run -r --bin ca -- --ca-path=./certs/ enroll-server smbdrop smbdrop
openssl verify -verify_hostname smbdrop -CAfile certs/ca/pub/ca.cert.pem certs/servers/smbdrop/smbdrop.cert.pem

cargo run -r --bin ca -- --ca-path=./certs/ enroll-agent agent1 testresource
openssl verify -CAfile certs/ca/pub/ca.cert.pem certs/clients/testresource/agent1.cert.pem

cargo run -r --bin ca -- --ca-path=./certs/ enroll-agent agent2 testresource
openssl verify -CAfile certs/ca/pub/ca.cert.pem certs/clients/testresource/agent2.cert.pem

find certs/
