# Graphite — DBM design system

Graphite is DBM's visual language: neutral graphite surfaces, a system-blue
accent, dense but readable data, and native desktop idioms (source-list
sidebar, segmented controls, a row inspector). It is the reference for the
current Tauri UI (`apps/desktop/ui/src/styles.css`) and for future native
clients. When a token changes, update both this file and `styles.css`.

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
- **Quiet motion.** 90–180 ms fades; honor `prefers-reduced-motion`.

## Color tokens

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
| `--success` | `#5ad394` | Success, inserted rows (future) |
| `--danger` | `#ff8a80` | Staged delete, destructive actions, errors |

Connection palette offered in the profile editor: `#4c9aff`, `#ff9f43`,
`#3dd6c6`, `#b48cff`, `#ff6b8a`, `#7ed957`, `#f0b14c`, `#8e8e93` (plus custom).

## Typography

- UI: **Geist** (bundled via `@fontsource-variable/geist`, OFL 1.1), falling back
  to the platform system font.
- Data and code: **Geist Mono** (`@fontsource-variable/geist-mono`), tabular
  numerals in grids.
- Fonts ship inside the app bundle; nothing is fetched at runtime.

| Role | Size / weight |
| --- | --- |
| Dialog title | 17 / 600 |
| Query title | 15 / 600 |
| Body, buttons, tree | 13 or 12.5 / 400–500 |
| Section label | 11 / 600, sentence case |
| Grid cells, inspector values | 12 mono |
| Column type, metadata | 11 mono |

Native clients may substitute SF Pro / SF Mono (macOS) and Segoe UI Variable /
Cascadia Mono (Windows) if matching Geist is impractical; keep the size scale.

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
| Modified cell | `--modified-soft` fill, 5 px `--modified` dot top-right; row gets a 2 px `--modified` edge |
| Deleted row | `--danger-soft` wash, `--danger` text with strikethrough, 2 px `--danger` edge |
| NULL | `NULL` in `--faint` italic |
| Primary key | key glyph in `--modified` in the header; read-only in the inspector |

## Components

- **Buttons:** primary (`--accent-strong`, white, 600), secondary (`--control`
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
- **Icons:** 24-px-grid stroke glyphs at 1.6 px (`Icon.tsx`). Native clients map
  them to SF Symbols / Segoe Fluent Icons.

## Designed but not implemented yet

The Graphite mockups also explored these ideas. They are not in the app yet:

- Inserting new rows (the green "inserted" state is reserved for it).
- A typed picker for enum columns in the grid.
- A SQL preview of staged changes before Save.
- A unified macOS title bar with window controls in the sidebar. The app still
  uses each platform's native title bar.
