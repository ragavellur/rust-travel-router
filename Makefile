.PHONY: all build release deb clean armhf arm64

all: release

build:
	cargo build

release:
	cargo build --release

deb:
	dpkg-buildpackage -b -us -uc

# Cross-compilation for NanoPi (armhf) and Cubie (arm64)
armhf:
	cargo build --release --target armv7-unknown-linux-gnueabihf

arm64:
	cargo build --release --target aarch64-unknown-linux-gnu

# Build deb for both architectures (requires cross toolchains)
deb-all: armhf arm64
	mkdir -p deb-packages
	# armhf deb
	dpkg-deb --build --root-owner-group \
		-DPkg:Version="0.1.0" \
		-DPkg:Architecture="armhf" \
		debian deb-packages/travel-net_0.1.0_armhf.deb
	# arm64 deb
	dpkg-deb --build --root-owner-group \
		-DPkg:Version="0.1.0" \
		-DPkg:Architecture="arm64" \
		debian deb-packages/travel-net_0.1.0_arm64.deb

clean:
	cargo clean
	rm -rf deb-packages
	rm -f ../travel-net_*.deb
	rm -f ../travel-net_*.dsc ../travel-net_*.tar.xz
	rm -f ../travel-net_*.buildinfo ../travel-net_*.changes

install: release
	install -D -m 755 target/release/travel-net /usr/sbin/travel-net
	install -D -m 644 debian/travel-net.service /lib/systemd/system/travel-net.service
	install -D -m 644 config/etc/travel-net/config.json /etc/travel-net/config.json
	install -D -m 644 nftables/travel-net.nft /etc/travel-net/travel-net.nft
	systemctl daemon-reload
	systemctl enable travel-net.service
	systemctl restart travel-net.service

# Quick test on local machine (won't work without hostapd/hw, but tests config + web server)
test:
	cargo test
	@echo "Run 'make install' as root to install on this device"
