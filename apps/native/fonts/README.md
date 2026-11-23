# Bundled Red Hat Mono fonts

These are static Red Hat Mono TTFs instantiated with `fontTools` from
[Google Fonts](https://github.com/google/fonts/tree/06bfe0d62a98375cb09c942fa18e68bd32cff057/ofl/redhatmono),
licensed under SIL OFL 1.1 (`RedHatMono-LICENSE`). Regular (400) and bold (700)
serve both UI and code; medium roles use regular, semibold roles use bold.
Regular, bold, italic, and bold italic faces are bundled. No fonts are fetched
at runtime.

Android and iPhone bundle identical copies under `apps/native/android/app/src/main/res/font`
(`red_hat_mono_*.ttf`) and `apps/native/ios/Resources/Fonts` (`RedHatMono-*.ttf`),
with the license at `apps/native/android/app/src/main/assets/RedHatMono-LICENSE.txt`
and `apps/native/ios/Resources/RedHatMono-LICENSE.txt`. The website uses locally
bundled WOFF2 faces from `@fontsource/red-hat-mono` instead of a font CDN.
