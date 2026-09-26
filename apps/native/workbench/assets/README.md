# Bundled Geist fonts

These TTF files are the existing frontend's Latin variable Geist/Geist Mono
fonts, decompressed from WOFF2 without changing glyphs or names. OFL licenses
are included alongside them. No fonts are fetched at runtime.

Regenerate from the repository root after `npm ci`:

```sh
uv run --with fonttools --with brotli python -c '
from fontTools.ttLib import TTFont
for name in ("geist", "geist-mono"):
    font = TTFont(f"node_modules/@fontsource-variable/{name}/files/{name}-latin-wght-normal.woff2")
    font.flavor = None
    font.save(f"apps/native/workbench/assets/{name}.ttf")
'
```

`geist-medium.ttf` and `geist-semibold.ttf` are static weight 500 and 600
instances of `geist.ttf`; egui only draws a variable font's default instance.

```sh
uv run --with fonttools python -c '
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont
for weight, name in ((500, "medium"), (600, "semibold")):
    font = instantiateVariableFont(TTFont("geist.ttf"), {"wght": weight}, updateFontNames=False)
    font.save(f"geist-{name}.ttf")
'
```
