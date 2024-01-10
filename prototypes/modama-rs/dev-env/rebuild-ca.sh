#!/bin/bash
set -ex

rm -rf certs/

cargo run -r --bin ca initialize-ca

cargo run -r --bin ca enroll-server localhost
openssl verify -verify_hostname localhost -CAfile certs/ca/ca.cert.pem certs/servers/localhost.cert.pem

cargo run -r --bin ca enroll-agent dld holo
openssl verify -CAfile certs/ca/ca.cert.pem certs/clients/holo/dld.cert.pem

find certs/
