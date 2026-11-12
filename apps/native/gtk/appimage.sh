#!/usr/bin/env bash
# Packages a release dbm-gtk binary as an AppImage.
#   appimage.sh <dbm-gtk binary> <output .AppImage>
# Needs appimagetool on PATH (or APPIMAGETOOL); CI downloads it.
# The binary keeps the egui app's name, dbm-workbench, inside the AppImage
# so existing installs and their desktop entries update onto it. GTK 4
# (4.12 or newer) comes from the host, as glibc does; it is not bundled.
set -euo pipefail
binary="$1"
output="$2"
root="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
tool="${APPIMAGETOOL:-appimagetool}"
appdir="$(mktemp -d)/Anybase.AppDir"
mkdir -p "$appdir/usr/bin"
install -m 0755 "$binary" "$appdir/usr/bin/dbm-workbench"
ln -s usr/bin/dbm-workbench "$appdir/AppRun"
cp "$root/apps/native/icons/icon.png" "$appdir/dbm.png"
cat > "$appdir/dbm.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Anybase
Comment=Database client for PostgreSQL, MySQL, and Redis
Exec=dbm-workbench
Icon=dbm
Categories=Development;Database;
Terminal=false
StartupWMClass=io.github.joswayski.dbm
DESKTOP
# Extract-and-run avoids needing FUSE on the build machine. The squashfs is
# uncompressed: compressed blocks change throughout with every new binary, so
# patch updates between images were nearly full size; uncompressed, they are
# about as small as the bare binary's (docs/releases.md#patch-updates).
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$tool" --no-appstream \
  --mksquashfs-opt -noI --mksquashfs-opt -noD --mksquashfs-opt -noF --mksquashfs-opt -noX \
  "$appdir" "$output"
chmod 0755 "$output"
