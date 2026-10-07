# Graphite — Anybase design system

Graphite is Anybase's visual language: neutral graphite surfaces, a system-blue
accent, dense but readable data, and native desktop idioms (source-list
sidebar, segmented controls, a row inspector). It is the reference for both
native apps. The tokens are defined in `enum Graphite` in
`apps/native/macos/Sources/Theme.swift` (AppKit) and in
`apps/native/workbench/src/theme.rs` (egui). When a token changes, update this
file and both theme files.

## Principles

- **Data first.** Chrome recedes; the grid and its values carry the contrast.
  Hierarchy comes from type weight, spacing, and hairlines before color.
- **Color means something.** The accent marks focus, selection, and the one
  primary action on screen. State colors (modified, inserted, deleted) are
  reserved for staged changes and never used decoratively.
- **Connection identity is per profile.** Each connection keeps its own color
  on its sidebar dot, tab edge, and table icon. Do not replace it with the
  accent.
- **Safe by default.** Writes are staged, visible in the grid, summarized in the
  pending-changes bar, and applied only on an explicit Save.
- **Quiet motion.** 90–180 ms fades; honor the system's reduce-motion setting.

## Color tokens

Token names below are the design names; the theme files use the same names in
each language's case (`--accent-strong` is `accentStrong` in Swift and
`ACCENT_STRONG` in Rust).

| Token | Value | Use |
| --- | --- | --- |
| `--bg` | `#161618` | Canvas: grid, editor, active tab |
| `--chrome` | `#1c1c1e` | Top bar, tab strip, toolbars, inspector, status bar |
| `--sidebar` | `#1f1f21` | Sidebar (source list) |
| `--grid-header` | `#1a1a1c` | Column headers |
| `--control` | `#2a2a2d` | Inputs, secondary buttons, segmented tracks |
| `--control-hover` / `--control-active` | `#323235` / `#3a3a3d` | Hover and selected segment |
| `--popover` | `#262629` | Menus, popovers, toasts |
| `--hairline` | `#202023` | Row separators |
| `--border` / `--border-strong` | `#2a2a2d` / `#353538` | Panel edges / control outlines |
| `--text-strong` | `#f5f5f7` | Titles, active tab |
| `--text` | `#e8e8ea` | Body and cell text |
| `--secondary` | `#c7c7cc` | Tree items, toolbar labels |
| `--muted` | `#a0a0a6` | Secondary copy |
| `--faint` | `#808087` | Metadata, NULL, row numbers (≥4.5:1 on `--bg`) |
| `--accent` | `#4c9aff` | Focus rings, selection edge, sort indicator |
| `--accent-strong` | `#3b78c7` | Primary button fill (white text passes 4.5:1) |
| `--accent-text` | `#7db6ff` | Accent-colored text and links |
| `--accent-soft` | `rgba(76,154,255,.14)` | Selected row, active toggles |
| `--edit-surface` | `#10192a` | Cell or field being edited |
| `--modified` | `#f0b14c` | Staged edit (dot, row edge, chip) |
| `--modified-soft` | `rgba(240,177,76,.12)` | Staged cell or inspector field fill |
| `--modified-row-soft` | `rgba(240,177,76,.05)` | Subtle full-row wash for staged edits |
| `--success` | `#5ad394` | Success, inserted rows (future) |
| `--danger` | `#ff8a80` | Staged delete, destructive actions, errors |

Connection palette offered in the profile editor: `#4c9aff`, `#ff9f43`,
`#3dd6c6`, `#b48cff`, `#ff6b8a`, `#7ed957`, `#f0b14c`, `#8e8e93` (plus custom).

## Typography

- UI, data, and code: **Space Mono** (OFL 1.1), with monospaced numerals in grids.
- Use the real regular (400) and bold (700) faces: former medium roles use
  regular, and semibold roles use bold. Keep the existing size scale.
- Desktop font files live in `apps/native/workbench/assets`; macOS `build.sh`
  copies them into its bundle. Android and iPhone bundle the same TTFs in their
  resources, and the website bundles WOFF2 faces from `@fontsource/space-mono`.
  No client fetches fonts from an external service at runtime.
- Native system dialogs, menus, and SF Symbol icons retain platform rendering.

| Role | Size / weight |
| --- | --- |
| Dialog title | 17 / 700 |
| Query title | 15 / 700 |
| Body, buttons, tree | 13 or 12.5 / 400 |
| Section label | 11 / 700, sentence case |
| Grid cells, inspector values | 12 mono |
| Column type, metadata | 11 mono |

System monospace fonts are failure fallbacks only; missing glyphs may use
platform or egui fallback fonts.

## Spacing, radius, elevation

- 4-pt spacing grid. Grid rows 32 px, column headers 34 px, toolbars 44 px,
  pending-changes bar 48 px, status bar 28 px.
- Radii: 5 px (chips, menu items), 6–7 px (controls, buttons), 10 px (panels,
  popovers), 14 px (dialogs).
- Elevation only for floating surfaces: popovers and toasts use
  `0 16px 40px rgba(0,0,0,.55)` plus a 1 px inner top highlight.

## Layout

```
┌ sidebar ─────┬ top bar: connection identity · updates ──────────────┐
│ Connections  ├ tabs (connection-colored top edge on active) ────────┤
│ ● Production │ toolbar: schema.table · counts · Refresh Copy Export ⊟│
│   database   ├ filters / sort / limit ─────────────────┬ inspector ─┤
│   schema tree│ grid (sticky header, 32 px rows)        │ row fields  │
│              │                                         │ Delete row  │
│ + New conn.  ├ pending changes: ● N · chips · Discard · Save ───────┤
└──────────────┴ status: Rows 1–50 of N · per page · ‹ Page › ───────┘
```

## Cell and row states

| State | Treatment |
| --- | --- |
| Default | `--text` on `--bg`, hairline below |
| Hover | `rgba(255,255,255,.025)` row wash |
| Selected row | `--accent-soft` wash, 2 px `--accent` left edge |
| Editing cell | `--edit-surface` fill, 1.5 px inset `--accent` ring |
| Modified row | `--modified-row-soft` wash, 2 px `--modified` edge; wash also overlays selected rows |
| Modified cell | Additional `--modified-soft` fill, 5 px `--modified` dot top-right |
| Deleted row | `--danger-soft` wash, `--danger` text with strikethrough, 2 px `--danger` edge |
| NULL | `NULL` in `--faint` italic |
| Primary key | key glyph in `--modified` in the header; read-only in the inspector |

## Components

- **Buttons:** primary (`--accent-strong`, white, 700), secondary (`--control`
  with `--border-strong`), toolbar (transparent, icon + label), icon (26 px),
  danger text. One primary per surface.
- **Segmented control:** `--control` track, `--control-active` selected segment
  with a 1 px shadow (engine picker).
- **Inputs:** 30 px (28 px in dense toolbars), `--control` fill; focus is an
  `--accent` border plus a 3 px 18% accent ring.
- **Menus:** `--popover`, 4 px padding, 28 px items; hover fills
  `--accent-strong` with white text; destructive items in `--danger`.
- **Row inspector:** 300 px panel on the right of the grid showing the single
  selected row. Each field shows name, type, a note ("Primary key" or
  "was …"), and a value field. Enter stages the change, Escape reverts it.
  Toggle it from the toolbar.
- **Pending-changes bar:** amber dot, count, per-kind chips, Discard (secondary)
  and Save changes (primary).
- **Icons:** stroke glyphs on a 16-pt grid at a 1.5 pt line, drawn by
  `apps/native/macos/Sources/Icons.swift` and
  `apps/native/workbench/src/icons.rs`.

## Designed but not implemented yet

The Graphite mockups also explored these ideas. They are not in the app yet:

- Inserting new rows (the green "inserted" state is reserved for it).
- A typed picker for enum columns in the grid.
- A SQL preview of staged changes before Save.
- A unified macOS title bar with window controls in the sidebar. The app still
  uses each platform's native title bar.
