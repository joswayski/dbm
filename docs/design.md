# DBM design system

DBM is a local-first database workbench. The interface has one job: get out of the
way while you read and change data, and be unambiguous the moment you need to make
a decision. This document is the reference for the visual language, the component
inventory, and the cross-platform rules every surface must follow.

## Principles

1. **Content first, chrome last.** Grids and the SQL editor are the product.
   Chrome is hairline-thin, monochrome, and quiet. Panels are separated by 1px
   lines and whitespace instead of nested boxes, borders, and shadows.
2. **One primary action per surface.** Each surface has exactly one solid accent
   button, always in the same position: the right end of a toolbar, the bottom
   right of a sheet. Everything else is a ghost or subtle control.
3. **Progressive disclosure, never hidden.** Secondary actions collapse into
   labelled overflow menus and toggles that show their state (filter counts,
   selection counts, pending counts). Nothing important is discoverable only by
   hover.
4. **Keyboard first, with visible affordances.** The command palette
   (`Ctrl`/`⌘`+`K`) can reach every navigation action, and shortcuts are printed
   next to the controls they trigger instead of being folklore.
5. **Color means something.** Neutral surfaces carry the layout. Cyan means
   "interactive": primary action, focus, active state. Per-connection color means
   identity — which database am I about to change. Amber means staged/pending,
   red means destructive, green means saved/safe.
6. **Same app on every OS.** No platform-only materials, glyphs, or metrics. See
   [Cross-platform rules](#cross-platform-rules).

## Color

Tokens live in `apps/desktop/ui/src/styles/tokens.css`. Components never hardcode
palette values; they consume semantic tokens so the light theme is a token swap.

| Token group | Purpose |
| --- | --- |
| `--surface-0` … `--surface-4` | Background ramp: window, rails, panels, raised controls, overlays |
| `--line`, `--line-strong`, `--line-accent` | Hairlines, emphasized dividers, focus outlines |
| `--text`, `--text-2`, `--text-3`, `--text-disabled` | Type ramp from primary copy to disabled |
| `--accent`, `--accent-hover`, `--accent-press`, `--accent-fg`, `--accent-soft` | The single interactive accent |
| `--success`, `--warning`, `--danger` (+ `-soft`, `-line`, `-text`) | State colors with stable meanings |
| `--connection-color` | Per-profile identity color, set inline by React |

Rules:

- Dark is the default appearance. Light is a full peer, not an afterthought, and
  both are reachable from the appearance menu (System / Dark / Light).
- The accent is used sparingly: primary buttons, focus rings, active tab and tree
  rows, running indicators. A screen with two accent-colored blocks is a bug.
- Connection color appears as a 6px dot, a 2px tab underline, and a 2px sidebar
  rail — never as a large background wash, which fights the data.
- Status colors keep their meaning across every surface: amber for staged edits,
  red for deletes and errors, green for saved and for "local only".

## Type and space

- Font stack resolves to the best native UI face on each OS without shipping a
  webfont: `Inter`, `SF Pro Text`, `Segoe UI Variable Text`, `Segoe UI`, `Roboto`,
  `Ubuntu`, `Cantarell`, `Noto Sans`.
- Monospace stack for SQL, values, and connection strings: `ui-monospace`,
  `SF Mono`, `Cascadia Mono`, `JetBrains Mono`, `Menlo`, `Consolas`,
  `DejaVu Sans Mono`.
- Scale: 11 / 12 / 13 / 15 / 19 / 26px. Nothing below 11px, because Linux and
  Windows rasterize small text more heavily than macOS.
- Uppercase micro-labels are 11px with `0.06em` tracking, used only for section
  headers in the sidebar and panel titles.
- Numbers in grids, counts, and durations use `font-variant-numeric: tabular-nums`
  so columns of digits stop jittering.
- Space scale is 4px based (`--space-1` = 4 … `--space-8` = 32). Control height is
  28px (`--control-h`), compact controls 24px, rows in grids 32px.
- Radii: 6px controls, 8px panels, 12px sheets, pill for badges.

## Layout

```
┌────────────────────────────────────────────────────────────────────────┐
│ top bar   DBM · connection identity · database        ⌘K  update  ⚙    │ 44
├──────────────┬─────────────────────────────────────────────────────────┤
│ sidebar      │ tab strip: query/table tabs, +                          │ 36
│  connections ├─────────────────────────────────────────────────────────┤
│  ─────────── │ view toolbar: title · one primary action                │ 44
│  database    ├─────────────────────────────────────────────────────────┤
│  schema tree │ content: editor + results, or grid                      │
│  filter      │                                                         │
├──────────────┴─────────────────────────────────────────────────────────┤
│ status bar   ● connected · engine · rows · pending · Local only   ⌘K   │ 26
└────────────────────────────────────────────────────────────────────────┘
```

- **Top bar** carries identity and global state only: the app mark, the active
  connection with its color, the database path in monospace, then update state,
  appearance, and the command palette affordance.
- **Sidebar** is one scroll region: connections, then the active connection's
  database picker and schema tree with a filter field. Resizable (240–460px) and
  collapsible to a 52px rail of connection dots.
- **Tab strip** is monochrome; the connection color is a 2px underline on the
  active tab plus a dot on each tab, so a tab from the wrong database is obvious.
- **View toolbar** is the single place a view exposes actions. Left: what you are
  looking at. Right: refresh, overflow, primary action.
- **Status bar** absorbs the ambient facts that used to clutter the content area:
  connection state, engine, the database path, read-only mode, open tab count,
  the local-only guarantee, and the command palette shortcut. Row, column, and
  selection counts stay with the grid they describe.

## Component inventory

Primitives (`ui.tsx`): `Button` (primary / secondary / ghost / danger),
`IconButton`, `Segmented`, `Field`, `TextField`, `SelectField`, `CheckboxField`,
`Menu` + `MenuItem` / `MenuLabel` / `MenuSeparator`, `Badge`, `Count`, `Kbd`,
`Sheet`, `EmptyState`, `Notice`. Supporting modules: `icons.tsx` (SVG icon set),
`useDismiss.ts` (outside-pointer and Escape dismissal), `platform.ts` (shortcut
glyphs), `appearance.ts` (appearance store), `editorTheme.ts` (CodeMirror theme
built from the tokens). Tooltips are CSS-driven through `data-tooltip`, and the
spinner is a CSS class so it can sit inside any button.

Surfaces:

| Surface | File | Notes |
| --- | --- | --- |
| App shell | `App.tsx` | Grid layout, keyboard map, toasts, error banner |
| Top bar | `TopBar.tsx` | Identity, database path, palette, updates, appearance |
| Sidebar | `Sidebar.tsx` | Connection rows, actions menu, database select, schema tree + filter |
| Tab strip | `TabStrip.tsx` | Tabs, rename, collapse, close, new query |
| Status bar | `StatusBar.tsx` | Ambient state, pending counts, shortcut hint |
| Welcome / empty | `Welcome.tsx` | First run, no selection, selected-not-connected |
| Connection sheet | `ConnectionSheet.tsx` | Sectioned form, URL import, test, delete |
| Command palette | `CommandPalette.tsx` | Connections, tables, queries, appearance, actions |
| Query workbench | `QueryView.tsx` | Editor, run/refresh, history drawer, results |
| Table workbench | `TableView.tsx` | Toolbar, query bar (filters/sort/limit), grid, staged changes |

## Interaction rules

- Every interactive element has a visible `:focus-visible` ring
  (`--line-accent`, 2px offset), including grid headers and tree rows.
- Destructive actions are red, are never the default focus target, and confirm
  before they touch data (delete profile, drop/truncate SQL, large exports).
- Staged table edits are amber, reversible, and summarized in a persistent bar
  with an explicit **Discard** and a primary **Save changes (n)**.
- Loading uses one pattern per situation: skeleton for first paint, a small
  overlay pill for refreshes, and inline spinners inside buttons that are
  waiting on a round trip.
- Motion is 120–180ms `ease-out`, only on enter/exit and state changes, and is
  fully disabled under `prefers-reduced-motion: reduce`.

## Cross-platform rules

DBM ships on macOS, Windows, and Linux from one React tree, so the design cannot
lean on Apple's system materials.

- **No platform-only materials.** No vibrancy, no `backdrop-filter` as a
  legibility requirement (WebKitGTK support is inconsistent); overlays use solid
  tokens with shadows.
- **SVG icons, never Unicode glyphs.** `×`, `⋯`, `⌄`, `▧`, `↤` render at
  different weights, baselines, and sometimes as tofu across Segoe UI Symbol,
  Noto, and Apple Symbols. Every glyph in the UI is an inline stroke icon from
  `icons.tsx` at a 24px grid, 1.75 stroke.
- **System font stack per OS,** with `font-synthesis: none` so a missing weight
  is never faked into a smear.
- **Native window decorations.** DBM does not draw its own traffic lights or
  caption buttons; the top bar sits inside the OS frame on all three platforms.
- **Scrollbars are styled for Chromium and WebKit** (`::-webkit-scrollbar` plus
  `scrollbar-color`/`scrollbar-width`), thin and overlay-like, because GTK and
  Windows scrollbars otherwise punch a light-gray gutter into a dark app.
- **Shortcut glyphs adapt:** `⌘↵` on Apple platforms, `Ctrl+↵` elsewhere, from a
  single `platform.ts` helper.
- **Layout is fluid down to the 960×640 minimum window** in `tauri.conf.json`:
  the history drawer, filter rows, and toolbar actions wrap or collapse instead
  of clipping.
- **Pointer parity:** 28px minimum hit target, hover states are never the only
  way to reach an action, and context menus are mirrored by visible buttons.

## What this redesign changed

- Replaced the navy chrome and glowing accents with a neutral surface ramp,
  hairline dividers, and a single cyan accent.
- Introduced the top bar, status bar, and command palette; moved ambient counts
  and the local-only badge out of the content area.
- Rebuilt the table view's filter card into a single dense query bar
  (filters + sort + limit + apply) that wraps instead of stacking.
- Replaced 20+ Unicode glyph buttons with an inline SVG icon set.
- Added a light appearance and a System option that follows the OS.
- Gave every empty, loading, and error state a designed treatment instead of
  centered gray text.
