#!/usr/bin/env bash
# Packages a release dbm-workbench binary as an AppImage.
#   appimage.sh <dbm-workbench binary> <output .AppImage>
# Needs appimagetool on PATH (or APPIMAGETOOL); CI downloads it.
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
DESKTOP
# Extract-and-run avoids needing FUSE on the build machine.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$tool" --no-appstream "$appdir" "$output"
chmod 0755 "$output"
