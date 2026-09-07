#!/usr/bin/env bash
# Generate flat APT Packages index for the 3 latest .deb files only.
set -euo pipefail
cd "$(dirname "$0")"
: > Packages
for arch in amd64 arm64 armhf; do
  for f in travel-net_0.2.33-1_${arch}.deb; do
    sz=$(stat -f%z "$f")
    md5=$(md5 -q "$f")
    sha1=$(shasum -a 1 "$f" | awk '{print $1}')
    sha256=$(shasum -a 256 "$f" | awk '{print $1}')
    line=$(dpkg-deb -f "$f" | tr '\n' ';')
    name=$(dpkg-deb -f "$f" Package)
    ver=$(dpkg-deb -f "$f" Version)
    arch2=$(dpkg-deb -f "$f" Architecture)
    maint=$(dpkg-deb -f "$f" Maintainer)
    rec=$(dpkg-deb -f "$f" Recommends)
    desc="Travel NAT Router"
    cat >> Packages <<PKG
Package: ${name}
Priority: optional
Section: net
Installed-Size: 2400
Maintainer: ${maint}
Architecture: ${arch2}
Recommends: ${rec}
Filename: ${f}
Size: ${sz}
MD5Sum: ${md5}
SHA1: ${sha1}
SHA256: ${sha256}
Homepage: https://github.com/ragavellur/rust-travel-router
Description: ${desc}

PKG
  done
done
gzip -9c Packages > Packages.gz
echo "Generated $(grep -c '^Package:' Packages) stanzas"
