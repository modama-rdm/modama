#!/bin/bash
set -ex

rm -r certs/

cargo run -r --bin ca initialize-ca
cargo run -r --bin ca enroll-server localhost
cargo run -r --bin ca enroll-agent dld holo
find certs/
