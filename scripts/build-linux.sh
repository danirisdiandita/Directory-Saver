#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
arch="$(uname -m)"
name="directory-saver-$version-$arch-linux"
stage="dist/$name"

cargo build --release --locked

rm -rf "$stage"
install -Dm755 target/release/directory-saver "$stage/bin/directory-saver"
install -Dm644 packaging/directory-saver.desktop "$stage/share/applications/directory-saver.desktop"
cp -r assets/icons "$stage/share/icons"
install -m755 packaging/install.sh packaging/uninstall.sh "$stage/"
install -m644 README.md "$stage/"

tar -C dist -czf "dist/$name.tar.gz" "$name"
echo "dist/$name.tar.gz"
