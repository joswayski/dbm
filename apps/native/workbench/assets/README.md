# Bundled Geist fonts

These TTF files are the Latin variable Geist/Geist Mono fonts from
`@fontsource-variable/geist` and `@fontsource-variable/geist-mono`, decompressed
from WOFF2 without changing glyphs or names. OFL licenses are included
alongside them. No fonts are fetched at runtime.

To regenerate, download those packages' `files/<name>-latin-wght-normal.woff2`
into this folder, then:

```sh
uv run --with fonttools --with brotli python -c '
from fontTools.ttLib import TTFont
for name in ("geist", "geist-mono"):
    font = TTFont(f"{name}-latin-wght-normal.woff2")
    font.flavor = None
    font.save(f"{name}.ttf")
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
