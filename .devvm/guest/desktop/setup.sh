#!/bin/bash
# One-time setup of the virtual desktop in the VM (Debian 13): Xvfb, openbox, xfce4-panel,
# the user's systemd session (GVfs, AT-SPI), software Vulkan, and Rust for building
# modama-agent and modama-agent-gui. Run through `devvm desktop-setup`.
set -euo pipefail

sudo DEBIAN_FRONTEND=noninteractive apt-get -qq update
sudo DEBIAN_FRONTEND=noninteractive apt-get -qq install -y --no-install-recommends \
    xvfb openbox xfce4-panel x11vnc xdotool imagemagick xterm x11-utils \
    dbus-user-session at-spi2-core python3-pyatspi gir1.2-atspi-2.0 \
    mesa-vulkan-drivers libvulkan1 vulkan-tools \
    libxkbcommon-x11-0 libxcursor1 libxi6 libxrandr2 \
    gvfs gvfs-backends gvfs-fuse thunar librsvg2-common \
    build-essential clang pkg-config libglib2.0-dev > /dev/null

# The desktop runs as transient units of the user's systemd manager, which keeps running
# between SSH logins
sudo loginctl enable-linger "$USER"

# The agent and smbdrop's certificate use the name smbdrop; the test deployment publishes
# its ports on 127.0.0.1
grep -q ' smbdrop$' /etc/hosts || echo '127.0.0.1 smbdrop' | sudo tee -a /etc/hosts > /dev/null

if [ ! -x ~/.cargo/bin/cargo ]; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
fi
echo "desktop setup done; next: devvm desktop-up"
