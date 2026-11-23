#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = ["dmgbuild==1.6.7"]
# ///
"""Build Anybase's macOS drag-to-Applications disk image without Finder GUI."""

from __future__ import annotations

import argparse
from pathlib import Path
import plistlib
import subprocess
import sys

REPO_ROOT = Path(__file__).resolve().parents[1]
VOLUME_NAME = "Anybase"
WINDOW_ORIGIN = (160, 140)
WINDOW_SIZE = (660, 432)  # 660x400 background art plus Finder's 32 pt title bar.
BACKGROUND_SIZE = (660, 400)
ICON_SIZE = 128
TEXT_SIZE = 13
APP_POSITION = (170, 200)
APPLICATIONS_POSITION = (490, 200)


def background_paths(repo_root: Path = REPO_ROOT) -> tuple[Path, Path]:
    dmg_dir = repo_root / "apps" / "native" / "macos" / "dmg"
    return dmg_dir / "background.png", dmg_dir / "background@2x.png"


def volume_icon_path(repo_root: Path = REPO_ROOT) -> Path:
    return repo_root / "apps" / "native" / "icons" / "icon.icns"


def build_settings(app_path: Path, repo_root: Path = REPO_ROOT) -> dict:
    """Return dmgbuild settings matching apps/native/macos/dmg/background.svg."""
    bg_1x, bg_2x = background_paths(repo_root)
    icon = volume_icon_path(repo_root)
    for asset in (bg_1x, bg_2x, icon):
        if not asset.is_file():
            raise FileNotFoundError(f"Missing DMG asset: {asset}")
    if not app_path.is_dir() or app_path.suffix != ".app":
        raise ValueError(f"Expected an .app bundle directory, got: {app_path}")
    return {
        "format": "UDZO",
        "files": [str(app_path)],
        "symlinks": {"Applications": "/Applications"},
        "hide_extensions": [app_path.name],
        "icon": str(icon),
        "background": str(bg_1x),
        "window_rect": (WINDOW_ORIGIN, WINDOW_SIZE),
        "default_view": "icon-view",
        "icon_locations": {
            app_path.name: APP_POSITION,
            "Applications": APPLICATIONS_POSITION,
        },
        "icon_size": ICON_SIZE,
        "text_size": TEXT_SIZE,
        "label_pos": "bottom",
        "show_status_bar": False,
        "show_tab_view": False,
        "show_toolbar": False,
        "show_pathbar": False,
        "show_sidebar": False,
    }


def verify_dmg(dmg_path: Path, app_name: str = "Anybase.app") -> None:
    """Mount the finished DMG read-only and verify its Finder layout records."""
    from ds_store import DSStore

    attached = subprocess.run(
        ["hdiutil", "attach", "-nobrowse", "-readonly", "-plist", str(dmg_path)],
        check=True,
        capture_output=True,
    )
    info = plistlib.loads(attached.stdout)
    device = None
    mount_point = None
    for entity in info.get("system-entities", []):
        if entity.get("mount-point"):
            device = entity["dev-entry"]
            mount_point = Path(entity["mount-point"])
            break
    if not device or not mount_point:
        raise RuntimeError(f"Could not determine mount point for {dmg_path}")
    try:
        app_entry = mount_point / app_name
        applications = mount_point / "Applications"
        background = mount_point / ".background.tiff"
        volume_icon = mount_point / ".VolumeIcon.icns"
        ds_store_path = mount_point / ".DS_Store"

        if not app_entry.is_dir():
            raise AssertionError(f"Missing {app_name} in DMG")
        if not applications.is_symlink() or applications.readlink() != Path("/Applications"):
            raise AssertionError("Applications symlink does not point to /Applications")
        if not background.is_file() or background.stat().st_size == 0:
            raise AssertionError("Missing Retina background TIFF (.background.tiff) in DMG")
        if not volume_icon.is_file() or volume_icon.stat().st_size == 0:
            raise AssertionError("Missing volume icon (.VolumeIcon.icns) in DMG")
        if not ds_store_path.is_file() or ds_store_path.stat().st_size == 0:
            raise AssertionError("Missing .DS_Store in DMG")

        with DSStore.open(str(ds_store_path), "r") as store:
            bwsp = store["."]["bwsp"]
            icvp = store["."]["icvp"]
            if (
                bwsp.get("ShowToolbar")
                or bwsp.get("ShowStatusBar")
                or bwsp.get("ShowPathbar")
                or bwsp.get("ShowSidebar")
            ):
                raise AssertionError(f"Unexpected Finder chrome enabled in bwsp: {bwsp}")
            if icvp.get("backgroundType") != 2 or not icvp.get("backgroundImageAlias"):
                raise AssertionError(f"Finder picture background missing in icvp: {icvp}")
            if float(icvp.get("iconSize", 0)) != float(ICON_SIZE):
                raise AssertionError(f"Expected iconSize {ICON_SIZE}, got {icvp.get('iconSize')}")
            if store[app_name]["Iloc"] != APP_POSITION:
                raise AssertionError(f"Unexpected {app_name} position: {store[app_name]['Iloc']}")
            if store["Applications"]["Iloc"] != APPLICATIONS_POSITION:
                raise AssertionError(f"Unexpected Applications position: {store['Applications']['Iloc']}")
    finally:
        subprocess.run(["hdiutil", "detach", device], check=False, capture_output=True)


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build the Anybase macOS installer DMG.")
    parser.add_argument("app", type=Path, help="Path to the signed Anybase.app bundle")
    parser.add_argument("output", type=Path, help="Path to the output .dmg file")
    parser.add_argument(
        "--volume-name",
        default=VOLUME_NAME,
        help=f"Mounted volume name (default: {VOLUME_NAME})",
    )
    parser.add_argument(
        "--verify",
        action="store_true",
        help="Mount the built DMG read-only and verify its Finder layout records",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    if sys.platform != "darwin":
        print("macOS DMG creation requires macOS (hdiutil and tiffutil).", file=sys.stderr)
        return 1

    import dmgbuild

    app_path = args.app.resolve()
    output_path = args.output.resolve()
    output_path.parent.mkdir(parents=True, exist_ok=True)
    if output_path.exists():
        output_path.unlink()

    settings = build_settings(app_path)
    dmgbuild.build_dmg(
        str(output_path),
        args.volume_name,
        settings=settings,
        lookForHiDPI=True,
    )
    if args.verify:
        verify_dmg(output_path, app_name=app_path.name)
    print(f"Built {output_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
