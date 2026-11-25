#!/usr/bin/env sh
set -eu

prefix="${PREFIX:-$HOME/.local}"
rm -f "$prefix/bin/directory-saver" "$prefix/share/applications/directory-saver.desktop"
rm -f "$prefix"/share/icons/hicolor/*/apps/directory-saver.*
command -v update-desktop-database >/dev/null && update-desktop-database "$prefix/share/applications" || true
echo "Removed Directory Saver from $prefix (your saved directories are kept)"
