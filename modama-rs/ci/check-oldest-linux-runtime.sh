#!/bin/sh
# Start the Linux release binaries on the oldest supported distribution, Ubuntu 18.04
# (glibc 2.27, GLib 2.56.4), with nothing but GLib installed.
#
# LD_BIND_NOW makes the dynamic linker resolve every imported symbol at startup, so a glibc or
# GLib function the old libraries lack fails the start right away, naming the symbol. `--help`
# is answered before anything else runs, so no display, D-Bus or GVfs is needed.
#
# Usage, inside an ubuntu:18.04 container: check-oldest-linux-runtime.sh <folder with the binaries>
# Locally, from modama-rs/ after `just -f ci.just linux-build-agent`:
#   podman run --rm -v "$PWD/target/x86_64-unknown-linux-gnu/release:/bin-under-test:ro" \
#     -v "$PWD/ci:/ci:ro" docker.io/library/ubuntu:18.04 /ci/check-oldest-linux-runtime.sh /bin-under-test
set -eu

dir="${1:?usage: $0 <folder with modama-agent and modama-agent-gui>}"

apt-get -qq update >/dev/null
DEBIAN_FRONTEND=noninteractive apt-get -qq install -y --no-install-recommends libglib2.0-0 >/dev/null
echo "runtime: $(ldd --version | head -n 1), GLib $(dpkg-query -W -f '${Version}' libglib2.0-0)"

for binary in modama-agent modama-agent-gui; do
    if ! LD_BIND_NOW=1 "$dir/$binary" --help >/dev/null; then
        echo "$binary does not start on this distribution (see the loader's error above)" >&2
        exit 1
    fi
    echo "$binary starts"
done
