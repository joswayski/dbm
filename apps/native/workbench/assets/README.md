# Bundled Space Mono fonts

These are unmodified static Space Mono TTFs from
[Google Fonts](https://github.com/google/fonts/tree/9710da1eacb3be272583c3224dcb70f9da6eadbb/ofl/spacemono),
licensed under SIL OFL 1.1 (`SpaceMono-LICENSE`). Regular (400) and bold (700)
serve both UI and code; medium roles use regular, semibold roles use bold.
macOS also registers the italic and bold italic faces. No fonts are fetched
at runtime.

To refresh the assets from the repository root:

```sh
source=https://raw.githubusercontent.com/google/fonts/9710da1eacb3be272583c3224dcb70f9da6eadbb/ofl/spacemono
assets=apps/native/workbench/assets
for face in Regular Bold Italic BoldItalic; do
  curl -fsSL "$source/SpaceMono-$face.ttf" -o "$assets/SpaceMono-$face.ttf"
  cp "$assets/SpaceMono-$face.ttf" "apps/native/ios/Resources/Fonts/SpaceMono-$face.ttf"
  case "$face" in
    Regular) resource=regular ;; Bold) resource=bold ;;
    Italic) resource=italic ;; BoldItalic) resource=bold_italic ;;
  esac
  cp "$assets/SpaceMono-$face.ttf" "apps/native/android/app/src/main/res/font/space_mono_$resource.ttf"
done
curl -fsSL "$source/OFL.txt" -o "$assets/SpaceMono-LICENSE"
cp "$assets/SpaceMono-LICENSE" apps/native/ios/Resources/SpaceMono-LICENSE.txt
cp "$assets/SpaceMono-LICENSE" apps/native/android/app/src/main/assets/SpaceMono-LICENSE.txt
```

Android and iPhone bundle identical copies. The website uses locally bundled
WOFF2 faces from `@fontsource/space-mono` instead of a font CDN.
