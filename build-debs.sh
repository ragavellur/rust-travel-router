#!/usr/bin/env bash
# Build travel-net .deb packages for all three architectures.
# Usage: ./build-debs.sh   (run from the repo root)
set -euo pipefail

cd "$(dirname "$0")"

export RUSTC="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc"
export CARGO="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin/cargo"
ZIGBUILD="$HOME/.cargo/bin/cargo-zigbuild"

mkdir -p target/debs
OUT="target/debs"

build_arch() {
    local target="$1"
    local arch="$2"
    local deps="$3"
    echo "=== Building $arch ($target) ==="
    "$ZIGBUILD" zigbuild --release --target "$target"
    rm -f "$OUT/travel-net_0.2.10-1_${arch}.deb"
    cat > pkg_deb/DEBIAN/control <<EOF
Package: travel-net
Version: 0.2.10-1
Architecture: ${arch}
Depends: ${deps}
Recommends: ntpdate
Priority: optional
Section: net
Maintainer: Travel Net Dev <dev@travel-net.local>
Description: Travel NAT Router with web UI
 Turn any Linux SBC with WiFi into a travel NAT router.
 Features: AP+STA hybrid mode, web UI, WiFi scanning,
 VPN (WireGuard + Tailscale), hostapd/dnsmasq/nftables management.
 Inspired by the ESP32 NAT Router.
EOF
    cp "target/$target/release/travel-net" pkg_deb/usr/sbin/travel-net
    chmod 755 pkg_deb/usr/sbin/travel-net
    cp debian/travel-net.service pkg_deb/lib/systemd/system/travel-net.service
    cp debian/travel-net-overlay.service pkg_deb/lib/systemd/system/travel-net-overlay.service
    cp debian/travel-net-overlay pkg_deb/usr/sbin/travel-net-overlay
    chmod 755 pkg_deb/usr/sbin/travel-net-overlay
    cp debian/travel-net-tailscale pkg_deb/usr/sbin/travel-net-tailscale
    chmod 755 pkg_deb/usr/sbin/travel-net-tailscale
    cp debian/travel-net-tailscale.service pkg_deb/lib/systemd/system/travel-net-tailscale.service
    cp debian/travel-net-timesync pkg_deb/usr/sbin/travel-net-timesync
    chmod 755 pkg_deb/usr/sbin/travel-net-timesync
    cp debian/travel-net-timesync.service pkg_deb/lib/systemd/system/travel-net-timesync.service
    cp config/etc/travel-net/config.json pkg_deb/etc/travel-net/config.json
    dpkg-deb -Zgzip --root-owner-group --build pkg_deb "$OUT/travel-net_0.2.10-1_${arch}.deb"
    echo "=== $arch -> $OUT/travel-net_0.2.10-1_${arch}.deb ==="
}

# hostapd backend devices (NanoPi, RPi): brcmfmac/armhf
build_arch armv7-unknown-linux-gnueabihf armhf "hostapd, dnsmasq, wpasupplicant, nftables, iw, wireguard-tools"
# NetworkManager backend devices (Cubie A5E/A7A, RPi 4/5, laptops)
build_arch aarch64-unknown-linux-gnu arm64 "network-manager, wpasupplicant, iw, wireguard-tools"
build_arch x86_64-unknown-linux-gnu amd64 "network-manager, wpasupplicant, iw, wireguard-tools"

echo "=== Done. Debs in $OUT ==="
