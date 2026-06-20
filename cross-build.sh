#!/usr/bin/env bash
set -euo pipefail

# cross-build.sh — Cross-compile travel-net for armhf (NanoPi) and arm64 (Cubie)
#
# Prerequisites:
#   rustup target add armv7-unknown-linux-gnueabihf
#   rustup target add aarch64-unknown-linux-gnu
#   sudo apt install gcc-arm-linux-gnueabihf gcc-aarch64-linux-gnu

TARGET="${1:-all}"

build_armhf() {
    echo "=== Building for armhf (NanoPi NEO Air) ==="
    export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER=arm-linux-gnueabihf-gcc
    cargo build --release --target armv7-unknown-linux-gnueabihf
    echo "Done: target/armv7-unknown-linux-gnueabihf/release/travel-net"
}

build_arm64() {
    echo "=== Building for arm64 (Radxa Cubie A7A) ==="
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
    cargo build --release --target aarch64-unknown-linux-gnu
    echo "Done: target/aarch64-unknown-linux-gnu/release/travel-net"
}

case "$TARGET" in
    armhf) build_armhf ;;
    arm64) build_arm64 ;;
    all)
        build_armhf
        build_arm64
        ;;
    *)
        echo "Usage: $0 {armhf|arm64|all}"
        exit 1
        ;;
esac
