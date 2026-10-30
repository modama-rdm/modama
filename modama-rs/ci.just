# Recipes only CI runs: release builds that need tools a development setup doesn't have.
# Run them with `just -f ci.just <recipe>`.

linux_glibc := "2.17"
linux_target := "x86_64-unknown-linux-gnu"

# Build the Linux agent binaries against an old glibc with cargo-zigbuild (needs zig on PATH).
linux-build-agent:
    #!/usr/bin/env bash
    set -euo pipefail
    # zig links against its own glibc {{ linux_glibc }} stubs and doesn't search the host's
    # library directories, so it can't find the GLib libraries the session helper's GIO backend
    # links (pkg-config leaves out -L for standard paths). Hand it a directory with only those
    # (not the host's libc). The host GLib itself needs a newer glibc than the stubs offer; that
    # is the runtime system's business, hence --allow-shlib-undefined. The glibc check below
    # still covers every glibc symbol our binaries import.
    glib_link_dir="$PWD/target/glib-link"
    glib_libdir="$(pkg-config --variable=libdir gio-2.0)"
    mkdir -p "$glib_link_dir"
    for lib in gio-2.0 gobject-2.0 glib-2.0; do
        ln -sf "$glib_libdir/lib$lib.so" "$glib_link_dir/lib$lib.so"
    done
    export RUSTFLAGS="${RUSTFLAGS:-} -L native=$glib_link_dir -C link-arg=-Wl,--allow-shlib-undefined"
    cargo zigbuild -q --locked --release -p modama-agent --bin modama-agent -p modama-agent-gui --bin modama-agent-gui --target {{ linux_target }}.{{ linux_glibc }}

# Require both binaries to only reference glibc symbol versions up to linux_glibc.
linux-check-agent-glibc: linux-build-agent
    #!/usr/bin/env bash
    set -euo pipefail
    for binary in modama-agent modama-agent-gui; do
        binary="target/{{ linux_target }}/release/$binary"
        newest="$(objdump -T "$binary" | grep -o 'GLIBC_[0-9.]*' | sed 's/^GLIBC_//' | sort -Vu | tail -n 1)"
        if [[ "$(printf '%s\n%s\n' "$newest" "{{ linux_glibc }}" | sort -V | tail -n 1)" != "{{ linux_glibc }}" ]]; then
            echo "$binary requires glibc $newest, newer than the supported {{ linux_glibc }}." >&2
            exit 1
        fi
        echo "glibc check passed for $binary (newest symbol version: $newest)"
    done
