"""Unit tests for the macOS drag-to-Applications DMG builder and artwork."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import struct
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


def load_macos_dmg():
    spec = importlib.util.spec_from_file_location("macos_dmg", ROOT / "scripts" / "macos_dmg.py")
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def png_size(path: Path) -> tuple[int, int]:
    header = path.read_bytes()[:24]
    if header[:8] != b"\x89PNG\r\n\x1a\n" or header[12:16] != b"IHDR":
        raise AssertionError(f"{path} is not a valid PNG")
    return struct.unpack(">II", header[16:24])


class MacosDmgTests(unittest.TestCase):
    def test_retina_background_assets_match_layout_dimensions(self):
        macos_dmg = load_macos_dmg()
        bg_1x, bg_2x = macos_dmg.background_paths(ROOT)
        self.assertTrue(bg_1x.is_file(), f"Missing {bg_1x}")
        self.assertTrue(bg_2x.is_file(), f"Missing {bg_2x}")
        width, height = macos_dmg.BACKGROUND_SIZE
        self.assertEqual(png_size(bg_1x), (width, height))
        self.assertEqual(png_size(bg_2x), (width * 2, height * 2))
        svg = (bg_1x.parent / "background.svg").read_text(encoding="utf-8")
        self.assertIn(f'viewBox="0 0 {width} {height}"', svg)
        self.assertIn("Install Anybase", svg)
        self.assertIn("Drag Anybase onto the Applications folder.", svg)

    def test_build_settings_configures_drag_to_applications_layout(self):
        macos_dmg = load_macos_dmg()
        with tempfile.TemporaryDirectory() as tmp:
            app = Path(tmp) / "Anybase.app"
            app.mkdir()
            settings = macos_dmg.build_settings(app, repo_root=ROOT)
            self.assertEqual(settings["format"], "UDZO")
            self.assertEqual(settings["files"], [str(app)])
            self.assertEqual(settings["symlinks"], {"Applications": "/Applications"})
            self.assertEqual(settings["hide_extensions"], ["Anybase.app"])
            self.assertEqual(settings["default_view"], "icon-view")
            self.assertEqual(settings["window_rect"], (macos_dmg.WINDOW_ORIGIN, macos_dmg.WINDOW_SIZE))
            self.assertEqual(
                settings["icon_locations"],
                {
                    "Anybase.app": macos_dmg.APP_POSITION,
                    "Applications": macos_dmg.APPLICATIONS_POSITION,
                },
            )
            self.assertEqual(settings["icon_size"], macos_dmg.ICON_SIZE)
            self.assertFalse(settings["show_toolbar"])
            self.assertFalse(settings["show_status_bar"])
            self.assertFalse(settings["show_pathbar"])
            self.assertFalse(settings["show_sidebar"])
            self.assertFalse(settings["show_tab_view"])
            self.assertTrue(Path(settings["background"]).is_file())
            self.assertTrue(Path(settings["icon"]).is_file())

    def test_build_settings_rejects_non_app_bundle(self):
        macos_dmg = load_macos_dmg()
        with tempfile.TemporaryDirectory() as tmp:
            not_app = Path(tmp) / "Anybase"
            not_app.mkdir()
            with self.assertRaises(ValueError):
                macos_dmg.build_settings(not_app, repo_root=ROOT)

    def test_workflows_use_locked_macos_dmg_script(self):
        self.assertTrue((ROOT / "scripts" / "macos_dmg.py.lock").is_file())
        release = (ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
        ci = (ROOT / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")
        self.assertIn("uv run --locked --python 3.12 --script scripts/macos_dmg.py", release)
        self.assertIn("uv run --locked --python 3.12 --script scripts/macos_dmg.py", ci)


if __name__ == "__main__":
    unittest.main()
