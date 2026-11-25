#!/usr/bin/env sh
set -eu

prefix="${PREFIX:-$HOME/.local}"
here="$(cd "$(dirname "$0")" && pwd)"

install -Dm755 "$here/bin/directory-saver" "$prefix/bin/directory-saver"
install -Dm644 "$here/share/applications/directory-saver.desktop" \
  "$prefix/share/applications/directory-saver.desktop"
for icon in "$here"/share/icons/hicolor/*/apps/directory-saver.*; do
  rel="${icon#"$here"/}"
  install -Dm644 "$icon" "$prefix/$rel"
done

command -v update-desktop-database >/dev/null && update-desktop-database "$prefix/share/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$prefix/share/icons/hicolor" || true

echo "Installed Directory Saver to $prefix"
case ":$PATH:" in
  *":$prefix/bin:"*) ;;
  *) echo "Note: $prefix/bin is not on your PATH" ;;
esac
