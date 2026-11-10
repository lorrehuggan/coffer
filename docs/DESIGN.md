# DESIGN.md — Visual design system

The visual language for Coffer is a dark, dense, professional-tool UI
with hairline structure, compact 28px rows, mono numerals, and two accent colours that
each carry one meaning.

Architecture and product decisions live in [`ARCHITECTURE.md`](ARCHITECTURE.md);
this file covers how the app looks and feels.

## Source of truth

| | |
|---|---|
| Tool | [Paper](https://paper.design), read and written via the Paper MCP server |
| Coffer file | **Coffer** — <https://app.paper.design/file/01M6YH440YAX6H5H322ZX4JYC9> (file ID `01M6YH440YAX6H5H322ZX4JYC9`, in the team's `Projects` folder). Created in D1.1; all Coffer design work happens here |
| Coffer page | `Page 1` (`p-1-0`) |
| Coffer artboards | Foundations (`1-0`), 1920 wide: colour, type scale, spacing, radii (D1.3). Components (`CJ-0`), 1920 wide: every component below in its states (D1.4). Transactions (`18E-0`) and Transactions · multi-select (`1NP-0`), 1440×900: the Transactions screen (D3.1). Overview (`22V-0`), 1440×900: the Overview screen (D4.1). D6.3, Transactions additions: Transactions · split and tags (`2Q3-0`), Transactions · excluded (`3FE-0`), Transactions · rule from an edit (`3WT-0`), Transactions · manual (cash account) (`4D7-0`), 1440×900, and Transactions · D6.3 states (`4X9-0`), 1440 wide |
| Coffer tokens | 124: 96 created in D1.2, 24 size tokens and 2 danger-state colours added in D1.4, `--container-search-field` added in D3.1, `--container-dialog` added in D6.3; content hash `b442a048` (see "Tokens") |
| Reference file | **Festive comet** — <https://app.paper.design/file/01M6GNHMB1RVVHY51G5W61H75N/p-1-0>. Not Coffer's file: a design-editor app this system was extracted from. Read it for detail; never edit it |
| Reference page | `Editor — v1` (`p-1-0`) |
| Reference artboards | Editor (`1-0`), Empty file (`JK-0`), 1920×1080 |
| Token snapshot | [`design/tokens.json`](../design/tokens.json), [`design/tokens.css`](../design/tokens.css) — exported from the **Coffer** file in D1.5 and re-exported in D3.1 and D6.3, content hash `b442a048`, extracted 2026-11-09. `tokens.json` keeps each token's Paper description |

The reference artboards show a design-editor app. What carries over to Coffer is the
**system** (tokens, chrome, components, interaction states, the AI-agent colour role), not
the editor's features. The coffee-shop mock-up drawn inside the editor canvas is sample
content; its serif (Fraunces) and cream palette are **not** part of the system.

**Component scope**

| | Components |
|---|---|
| Used as specified | Title bar, tab, command bar, kbd, segmented control, list row, section label row, inspector field row, select, toggle, buttons (neutral primary, outline, outline compact, icon, chip), badge, status dot, underline tabs, card, empty state, tooltip, status bar, agent card, progress bar |
| Repurposed | Floating toolbar → bulk-action bar for multi-selected transactions. Selection overlay → selection language for charts. Agent buttons → inbox actions ("Review 12 suggestions", "Approve all", "Reject all"). Avatar → agent avatar only |
| Not used | Tool dock; prompt composer (Coffer has no in-app chat: the AI lives in the external MCP client); collaborator avatars and "Share"; the editor's brand mark |

The editor's molten gem is that product's mark, not ours. Until Coffer has its own
mark, the title bar shows a wordmark: "Coffer", Inter 13/16 600, `--color-text`,
−0.01em.

**Rules**

1. Paper wins. If this file and the Coffer Paper file disagree, the Paper file is right;
   update this file and re-export the tokens.
2. Don't invent values. Use the values listed here. Every token is in the Coffer file.
   Values marked **Proposed** (the finance screens not drawn yet) are decided and
   may be used in code, but aren't drawn in Paper yet; when they are, drop the marker.
   Motion, shadows and border widths have no Paper token type, so this file is their
   source.
3. Code references tokens by name, never raw hex or px literals outside the theme module.
4. New or changed Paper tokens: re-run the Paper MCP `get_tokens` export (json + css) from
   the Coffer file into `design/`, and update the tables below and `src/theme/tokens.rs` in
   the same change. The drift test fails until the theme module matches the export.

## Character

- **Dark, chrome-first.** Near-black chrome around a slightly lighter content canvas.
  There is no light theme yet.
- **Hairlines, not boxes.** Panels and sections are separated by 1px `hairline` borders.
  Information sits directly on surfaces; cards are reserved for things that float or
  demand attention.
- **Dense and calm.** 12px body text, 28px rows, 4px spacing grid. Hierarchy comes from
  weight and colour steps, not size jumps.
- **Two accents, one job each.** `molten` (orange) means *brand, selection, focus,
  "you are here"*. `moldavite` (green) means *the AI agent*: presence, its proposals, its
  progress. Everything else is greyscale.
- **Numbers are mono.** Coordinates, counts, sizes, and shortcuts use Geist Mono. In
  Coffer this extends to every amount, date, and figure.

## Tokens

The Coffer file holds 124 tokens (content hash `b442a048`), exported to
[`design/tokens.json`](../design/tokens.json) and [`design/tokens.css`](../design/tokens.css)
and checked against the theme module by its drift test. Every node on the Foundations,
Components, Transactions (D3, D6) and Overview artboards is bound to them. Motion durations, shadows and border widths have
no Paper token type and stay in this file.

How the set was built: the first three tables are the 35 tokens of the reference file
(with the weights changed to 400/500/600, see "Type scale"), plus `--font-weight-bold` and
`--opacity-disabled`, which complete their groups. The reference file bound only
eight colour tokens to nodes and typed everything else as literals; D1.2 turned the repeated
literals into the tokens in "More colour" and named the type, layout and state values this
file already used ("Tokens named in D1.2"). D1.4 added the component sizes ("Tokens named
in D1.4") and the solid danger button's hover and pressed colours ("Status colours").

### Colour

| Token | Value | Role |
|---|---|---|
| `--color-chrome` | `#0F0F10` | Title bar, side panels, inspector — the app frame |
| `--color-canvas` | `#18181A` | Main content area (the "work" surface) |
| `--color-raised` | `#1A1A1C` | Raised surface on canvas |
| `--color-field` | `#1F1F22` | Inputs, selects, segmented-control tracks, command bar |
| `--color-hover` | `#26262A` | Hovered/current row or tab; kbd background |
| `--color-hairline` | `#232326` | 1px dividers and panel borders |
| `--color-text` | `#EDEDEF` | Primary text and icons |
| `--color-text-muted` | `#8E8E93` | Secondary text, labels, inactive tabs, default icons |
| `--color-text-faint` | `#5C5C61` | Tertiary text, counts, field prefixes (X/Y) |
| `--color-molten` | `#FF6B1A` | Brand mark, selection outline/handles, measurement labels, notification dot |
| `--color-molten-soft` | `#FF6B1A24` | Selected row background |
| `--color-moldavite` | `#7CC48A` | AI agent: primary agent buttons, agent name, live dots, progress |
| `--color-moldavite-soft` | `#7CC48A1F` | Agent count badges |

### Typography

| Token | Value |
|---|---|
| `--font-ui` | `"Inter"` |
| `--font-mono` | `"Geist Mono"` |
| `--text-xs` | `11px` |
| `--text-sm` | `12px` |
| `--text-md` | `13px` |
| `--text-lg` | `15px` |
| `--font-weight-regular` | `400` |
| `--font-weight-medium` | `500` |
| `--font-weight-strong` | `600` |
| `--font-weight-bold` | `700` |
| `--tracking-tight` | `-0.01em` |
| `--tracking-caps` | `0.06em` |
| `--leading-ui` | `16px` |

### Space, size, shape

| Token | Value |
|---|---|
| `--spacing-1` … `--spacing-4` | `4px`, `8px`, `12px`, `16px` |
| `--radius-sm` / `-md` / `-lg` | `4px` / `6px` / `10px` |
| `--container-panel` | `272px` (left panel width) |
| `--opacity-muted` | `60%` |
| `--opacity-disabled` | `40%` (disabled controls) |
| `--breakpoint-desktop` | `1920px` |

### More colour: values typed into the reference artboards

The reference artboards repeat these literal values. Near-duplicates are merged (the visual
difference is below 1% lightness), so each role has one value.

| Token | Value | Merges | Use |
|---|---|---|---|
| `--color-text-secondary` | `#B4B4B9` | `#A1A1A6` (tooltip body) | Text one step below primary: list labels, chips, kbd glyphs, tooltip body, secondary icons |
| `--color-text-strong` | `#D6D6DA` | | Text on outline buttons, empty-state titles, emphasized values |
| `--color-icon-faint` | `#6E6E73` | | Chevrons, sort indicators, income bars in charts |
| `--color-icon-disabled` | `#4A4A4F` | | Disabled icons |
| `--color-float` | `#141416` | | Floating surfaces (toolbar, cards, popovers), status bar |
| `--color-control-active` | `#2C2C30` | | Active segment, active icon button, toggle track (off), pressed state |
| `--color-border` | `#2A2A2E` | | Borders on floating surfaces and chips |
| `--color-border-strong` | `#2E2E33` | | Outline-button borders, dashed empty states, inline separators, chart baseline |
| `--color-tooltip-border` | `#333338` | | Tooltip border, kbd inside tooltips |
| `--color-avatar` | `#3A3A3F` | | Neutral avatar fill |
| `--color-on-light` | `var(--color-chrome)` | | Text on the neutral primary button |
| `--color-on-moldavite` | `#0E1710` | | Text on solid moldavite buttons |
| `--color-moldavite-text` | `#A9D9B2` | | Agent text on dark ("Approve all", agent labels) |
| `--color-moldavite-text-muted` | `#7E8A81` | | Secondary text inside agent surfaces |
| `--color-moldavite-text-faint` | `#5C6B60` | | Timestamps inside agent surfaces (non-essential text only; 3.2:1) |
| `--color-moldavite-surface` | `#111813` | `#121A14` | Agent card fill |
| `--color-moldavite-surface-raised` | `#162019` | | Agent ghost button, hover on agent card rows |
| `--color-moldavite-well` | `#1B241D` | `#14231A` | Icon wells and agent avatar fill |
| `--color-moldavite-border` | `#24352A` | `#2A3D2E` | Agent card border |
| `--color-moldavite-border-strong` | `#2E4535` | `#2A4031` | Agent outline button border |
| `--color-moldavite-track` | `#22332A` | | Agent progress-bar track |

The amber marker (`#E7C24A`) becomes `--color-warning` (see Status colours).

Contrast on `--color-canvas`: `text` 15.2:1, `text-secondary` 8.6:1, `text-muted` 5.4:1,
`text-faint` 2.7:1. **`text-faint` fails WCAG for text**, so use it only for decorative or
redundant information (counts that are also given elsewhere, field prefixes), never for
something the user needs to read.

### Tokens named in D1.2

These values were already in this file (type scale, layout, interaction states) and in the
theme module, but had no token name. D1.2 named them so the foundations and components
artboards (D1.3, D1.4) can be built without literal values.

| Token | Value | Use | Theme constant |
|---|---|---|---|
| `--color-white` | `#FFFFFF` | Hovered neutral primary button; selected-row labels, selection handles, text on molten labels | `color::WHITE` |
| `--color-moldavite-hover` | `#8FD09C` | Hovered agent primary button | `color::MOLDAVITE_HOVER` |
| `--color-moldavite-pressed` | `#6DB27B` | Pressed agent primary button | `color::MOLDAVITE_PRESSED` |
| `--text-2xs` | `10px` | Mono micro, badges | `text::MICRO` |
| `--text-xl` | `20px` | Figure | `text::FIGURE` |
| `--text-2xl` | `32px` | Display, display figure | `text::DISPLAY` |
| `--leading-micro` | `12px` | With `--text-2xs` | `leading::MICRO` |
| `--leading-caption` | `14px` | With `--text-xs` | `leading::CAPTION` |
| `--leading-reading` | `20px` | Reading body and headings | `leading::READING` |
| `--leading-figure` | `24px` | With `--text-xl` | `leading::FIGURE` |
| `--leading-display` | `38px` | With `--text-2xl` | `leading::DISPLAY` |
| `--tracking-display` | `-0.025em` | Display, display figure | `tracking::DISPLAY` |
| `--spacing-0_5` | `2px` | Separator and toolbar gaps | `spacing::S0_5` |
| `--spacing-1_5` | `6px` | Tight icon + label pairs; list-section inline padding | `spacing::S1_5` |
| `--spacing-2_5` | `10px` | Tight icon + label pairs; list-section bottom padding | `spacing::S2_5` |
| `--spacing-3_5` | `14px` | Inspector section padding | `spacing::S3_5` |
| `--spacing-5` | `20px` | Pot indent under the parent account | `spacing::S5` |
| `--spacing-6` | `24px` | Bulk-action bar offset | `spacing::S6` |
| `--radius-marker` | `2px` | Swatches, markers, chart bars, progress bars | `radius::MARKER` |
| `--radius-label` | `3px` | Molten measurement / selection labels | `radius::LABEL` |
| `--radius-button` | `7px` | Primary buttons, command bar | `radius::BUTTON` |
| `--radius-surface` | `8px` | Segmented track, tooltip, cards in panels, badges, toggle track | `radius::SURFACE` |
| `--radius-card` | `12px` | Cards | `radius::CARD` |
| `--radius-pill` | `14px` | Chips | `radius::PILL` |
| `--opacity-area-fill` | `8%` | Chart area fill | `opacity::AREA_FILL` |
| `--container-inspector` | `296px` | Right inspector width | `size::INSPECTOR` |

Spacing half-steps use `_` (`--spacing-1_5`) because token names can't contain `.`.

### Tokens named in D1.4

The control heights, component geometry and column widths the component specs use. The
theme already had most of them as `size` constants. D1.4 named them so every node on the
Components artboard binds to a token. Heights and geometry are `spacing` tokens (Paper's
type for widths and heights); layout widths are `container` tokens, like `--container-panel`.

| Token | Value | Use | Theme constant |
|---|---|---|---|
| `--spacing-progress` | `3px` | Progress bars, ranked-bar-list bars | `size::PROGRESS` |
| `--spacing-dot` | `6px` | Status dots, flagged-row dot, chart hover points | `size::DOT` |
| `--spacing-handle` | `7px` | Selection handles | `size::HANDLE` |
| `--spacing-marker` | `8px` | Colour markers, select chevron, sort chevron | `size::MARKER` |
| `--spacing-icon-xs` … `-lg` | `10px` / `12px` / `14px` / `16px` | Icon sizes | `size::ICON_XS` … `ICON_LG` |
| `--spacing-toggle-knob` | `12px` | Toggle knob | `size::TOGGLE_KNOB` |
| `--spacing-control-xs` | `16px` | Toggle, badges, kbd in tooltips, agent-card icon wells | `size::CONTROL_XS` |
| `--spacing-kbd` | `18px` | Kbd | `size::KBD` |
| `--spacing-control-sm` | `24px` | Outline buttons, section-label-row actions, agent-row approve/reject | `size::CONTROL_SM` |
| `--spacing-control-compact` | `26px` | Compact outline buttons, avatar, agent-card rows | `size::CONTROL_COMPACT` |
| `--spacing-control` | `28px` | Rows, tabs, fields, icon buttons, chips, card icon well | `size::CONTROL` |
| `--spacing-status-bar` | `28px` | Status bar | `size::STATUS_BAR` |
| `--spacing-toggle-width` | `28px` | Toggle track | `size::TOGGLE_WIDTH` |
| `--spacing-control-lg` | `30px` | Primary, agent and destructive buttons, panel segmented control | `size::CONTROL_LG` |
| `--spacing-control-xl` | `36px` | Bulk-action bar, agent ghost button | `size::CONTROL_XL` |
| `--spacing-title-bar` | `40px` | Title bar | `size::TITLE_BAR` |
| `--container-field-label` | `64px` | Inspector field-row label column | `size::FIELD_LABEL` |
| `--container-column-amount` | `96px` | Transactions: amount column (minimum) | `size::COLUMN_AMOUNT` |
| `--container-column-account` | `140px` | Transactions: account column | `size::COLUMN_ACCOUNT` |
| `--container-column-category` | `180px` | Transactions: category column | `size::COLUMN_CATEGORY` |
| `--container-command-bar` | `340px` | Command bar | `size::COMMAND_BAR` |
| `--container-search-field` | `240px` | Transactions toolbar search field (added in D3.1) | `size::SEARCH_FIELD` |
| `--container-dialog` | `440px` | Short dialogs: a confirmation or a form of a few fields (added in D6.3) | `size::DIALOG` |

Paper drops a token used as a border width or a shadow offset, so 1px and 1.5px borders,
rings and shadow geometry stay typed. `calc()` over tokens works and is used where a size
is derived, e.g. the agent-row hover bleed `calc(var(--spacing-1_5) * -1)`.

## Type scale

**Weights: 400 / 500 / 600 / 700.** The reference artboards apply 400 (unset), 500, 550,
600, 650 and 700, while the reference tokens say 450/550/650 and are bound to nothing. The
decision, now in the Coffer file:

| Token | Value | Reference value | Replaces in reference artboards |
|---|---|---|---|
| `--font-weight-regular` | `400` | `450` | 400 (what actually renders) |
| `--font-weight-medium` | `500` | `550` | 500 and 550 |
| `--font-weight-strong` | `600` | `650` | 600 and 650 |
| `--font-weight-bold` | `700` | — | 700 (badges, agent primary button) |

Why: 400 is what every unset text node already renders at. 550 and 650 sit only 50 units
from their neighbours, a difference that barely shows at 11–13px. And all four values exist
as static Inter instances, so the GPUI build doesn't depend on variable-font rendering.
Some component specs below still quote the reference artboards; implement 550 as 500 and
650 as 600.

Line heights snap to a 2px grid. Tracking: tight on large type, open on caps.

| Role | Size / line | Weight | Tracking | Colour | Example |
|---|---|---|---|---|---|
| Display | 32 / 38 | 600 | −0.025em | text | Hero heading |
| Display figure | 32 / 38, Geist Mono | 600 | −0.025em | text | Overview "Spent" total |
| Figure | 20 / 24, Geist Mono | 600 | −0.01em | text | Overview "Income" and "Net" |
| Reading heading | 15 / 20 | 600 | −0.01em | text | Headings in saved AI reviews (`--text-lg`) |
| Reading body | 13 / 20 | 400 | — | text | Body of saved AI reviews |
| Title | 13 / 16 | 600 | −0.01em | text | Panel header, inspector title |
| Card title | 13 / 16 | 600 | — | text | Card titles |
| Section title | 12 / 16 | 600 | — | text | Inspector sections |
| Body | 12 / 16 | 400–500 | — | text / secondary / muted | Rows, tabs, labels |
| Button | 12 / 16 | 600 (neutral), 700 (agent), 500 (outline) | — | per button | |
| Caption | 11 / 14 | 400–500 | — | muted | Subtitles, status bar, small buttons |
| Section label | 11 / 14 | 600 | +0.06em, UPPERCASE | muted | "ACCOUNTS", table headers |
| Mono value | 11 / 14 | 400 | — | text / muted | Dates, counts, balances in lists |
| Mono amount | 12 / 16 | 400 | — | text | Amounts in tables |
| Mono micro | 10 / 12 | 400–600 | — | secondary / white on molten | Kbd glyphs, chart axis labels, selection labels |
| Badge | 10 / 12 | 700 | — | moldavite | Count badges |

**For Coffer:** every monetary amount, date, percentage, and count is Geist Mono with
tabular figures, right-aligned in tables. Inter is for words.

## Layout and sizing

- **Grid:** 4px base (`spacing-1`…`4`). Observed gaps: 2, 4, 6, 8, 10, 12, 16. Prefer
  the token steps; 6px and 10px appear for tight icon+label pairs and section padding.
- **App frame:** title bar 40px · left panel 272px · right inspector 296px · status bar 28px.
  Panels use `chrome` with a 1px `hairline` border on the canvas side.
- **Control heights:** 16 (toggle, badges, kbd-in-tooltip) · 18 (kbd) · 24 (small button) ·
  26 (compact button, avatar) · **28 (default: rows, tabs, fields, icon buttons)** ·
  30 (primary buttons, segmented control) · 36 (dock tools, floating toolbar) · 48 (dock).
- **Section padding:** inspector sections 14px all round with 8px internal gap; list
  sections 6px inline / 4px top / 10px bottom; rows 8px inline.
- **Radius usage:** 2px colour swatches/markers · 3px measurement labels · 4px kbd, inner
  segments, swatches · **6px rows, fields, tabs, icon buttons** · 7px primary buttons and
  command bar · 8px segmented track, tooltip, cards in panels, dock tools · 10px floating
  toolbar, agent card · 12px starter cards · 13px dock · 14px chips (pill) and composer.

## Elevation

Flat by default. Only floating things get shadow.

| Level | Shadow | Used on |
|---|---|---|
| Inset highlight | `inset 0 1px 0 #FFFFFF0A` | Active segment in segmented control |
| Swatch ring | `inset 0 0 0 1px #FFFFFF1F` | Colour swatches |
| Tooltip | `0 6px 18px #00000066` | Tooltip |
| Toolbar | `0 8px 24px #00000073` | Contextual action bar |
| Dock | `0 10px 30px #00000080` | Bottom tool dock; dialogs (D6.3) |
| Composer | `0 0 0 4px #7CC48A0F, 0 16px 40px #00000059` | Agent prompt composer (moldavite halo) |

Floating surfaces: `#141416` fill, `#2A2A2E` 1px border, radius 10–13.

## Components

Values below started from the reference artboards and are now drawn on the Coffer
**Components** artboard (`CJ-0`). Names in parentheses are Coffer node IDs, for lookup with
the Paper MCP (`get_jsx` / `get_computed_styles`); `ref` marks a reference-file ID for a
component Coffer doesn't use.

**Settled on the Components artboard (D1.4).** Where the reference typed an off-grid value,
the artboard snaps it to the nearest token, as the weights snap 550 → 500:

- Padding and gaps: outline button 9 → 8px (`spacing-2`), chip 11 → 12px (`spacing-3`),
  kbd 5 → 4px (`spacing-1`), tab icon gap 7 → 6px (`spacing-1_5`). Tooltip body 11/15 →
  caption 11/14. Section-label-row actions 22px radius 5 → `control-sm` radius 6. Card icon
  well 32 → 28px (`control`). Agent card radius 8–10 → 10 (`radius-lg`).
- New state designs: agent outline hovers to `moldavite-surface-raised` and presses to
  `moldavite-well`; agent ghost hovers to `moldavite-well` and presses to `moldavite-border`.
  Grey `hover`/`control-active` would read as foreign on an agent surface. Destructive
  outline follows the generic rules (`hover` / `control-active`). A hovered agent-card row
  swaps its timestamp for approve ✓ / reject ✕ icon buttons (`control-sm`, `moldavite-text`).
- Destructive buttons are 30px (`control-lg`, `radius-button`) so they pair with the
  neutral primary in dialogs. The solid one hovers to `danger-hover` and presses to
  `danger-pressed`: `danger` shifted by moldavite's own hover/pressed steps in OKLCH, so
  the two solid accent buttons behave alike (see "Status colours").
- View tabs in the title bar have no close button: Coffer's views are fixed.
- Budget bars use a `border` track; the overage past 100% is drawn after a `spacing-0_5`
  gap that marks the budget's end.
- The "Uncategorised" marker is dashed 1px `text-muted`, not `border-strong`. `border-strong`
  measured 1.1–1.4:1 against `chrome`, `canvas`, `hover` and a selected row, so it
  disappeared; `text-muted` is 4.5–5.9:1 on all four. Dashed with no fill, it stays
  distinct from the solid-outlined "Other" marker.

**Title bar** (`OU-0`) — 40px, `chrome`, bottom hairline, 12px left / 8px right padding, 4px gap.
"Coffer" wordmark and the view tabs at left, command bar centred, window controls right.

**Tab** (`QE-0`, `QM-0`) — 28px, radius 6, 10px padding, 6px icon gap, 12/16 weight 500,
`text-muted`. Hover: `hover` background, label stays `text-muted`. Active: `hover` background,
`text`, weight 550 (implemented as 500). A closable tab adds a close button (18px, radius 4).
Optional 8×8 colour marker (radius 2). Tabs are separated by a 1×14 hairline.

**Command bar** (`O3-0`) — 340×28, `field` fill, hairline border, radius 7, 10px left / 6px right
padding, 8px gap, search icon, placeholder in `text-muted`, trailing kbd.

**Kbd** (`OB-0`) — 18px, `hover` fill, radius 4, 4px padding, Geist Mono 11/14, `#B4B4B9`.
In tooltips: 16px, `#333338`, mono 10/12.

**Segmented control** (`N7-0`, `NR-0`) — track `field`, 30px radius 8 (panel) or 28px radius 6
(inspector), 2px padding. Active segment `#2C2C30`, radius 6/4, inset highlight (panel only),
text `text` weight 500.
Inactive text `text-muted`.

**List row** (`S6-0` left panel, `VI-0` states) — 28px, radius 6, 8px inline padding, 6–8px gap, icon +
label + trailing mono count (`text-faint`, 11/14).
States: default transparent / `text-secondary` label · hover/current `hover` fill,
`text` label · **selected `molten-soft` fill, white label** · focus: inset ring. Nesting indents
the row (pots: 20px).

**Section label row** (`SE-0`) — 28px, 8px left / 4px right padding, caps label (11/14, 600,
+0.06em, `text-muted`), trailing 24px icon actions.

**Inspector field row** (`JY-0`, `KA-0`) — 28px, label column 64px (`text-muted` 12/16),
control fills the rest. Field: `field` fill, radius 6, 8px padding, optional faint prefix
(e.g. "X") and trailing mono unit.

**Select** (`KK-0`) — field style, 10px left / 8px right, value `text`, chevron 8px
`#8E8E93`.

**Toggle** (`MA-0`, `MG-0`) — 28×16 track, radius 8, 2px padding, 12px knob.
Off: track `#2C2C30`, knob `text-muted`. On: track `text` (`#EDEDEF`), knob `chrome`.
(Neutral, not coloured: switches don't use the accents.)

**Buttons** (matrix `D4-0`: every variant × default, hover, pressed, focus, disabled)

| Variant | Spec | Example |
|---|---|---|
| Primary (neutral) | 30px, `#EDEDEF` fill, radius 7, 14px padding, `#0F0F10` text 12/16 600 | "Share" |
| Agent primary | 30px, `moldavite` fill, radius 7, `#0E1710` text 12/16 650–700, optional trailing kbd | "Draft it", "Review 12 changes" |
| Agent outline | 30px, 1px `#2E4535` border, radius 7, 12px padding, `#A9D9B2` text 12/16 500 | "Undo all" |
| Agent ghost | 36px, `#162019` fill, radius 8, sparkle icon in moldavite, `#A9D9B2` text 550 | "Ask agent" |
| Outline | 24px, 1px `#2E2E33` border, radius 6, 8px padding, `#D6D6DA` 11/14 550 | "Connect" |
| Outline compact | 26px, 1px `hairline` border, radius 6, 8px padding, `#B4B4B9` 11/14 500 | "Main" |
| Icon button | 28×28, radius 6, icon 12–16px; dock: 36×36 radius 8, active `#2C2C30` with white icon | Panel toggle, dock tools |
| Chip | 28px, 1px `#2A2A2E` border, radius 14 (pill), 12px padding, `#B4B4B9` 12/16, optional mono count | "Uncategorised 14" |
| Destructive outline | 30px, 1px `danger` border, radius 7, 12px padding, `danger` text 12/16 500 | "Delete import" |
| Destructive solid | 30px, `danger` fill, radius 7, 14px padding, `on-danger` text 12/16 600; dialog confirmation only | "Delete 312 transactions" |

**Badge** (`14J-0`, neutral `14N-0`) — 16px min, radius 8, 4px padding, `moldavite-soft` fill, moldavite
10/12 700. Agent-only; a neutral badge would use `hover` + `text-muted`.

**Status dot** — 6×6, radius 3. Moldavite = agent connected/live. Molten = unread/new.

**Tabs (underline)** (`R8-0`) — inspector tabs: 12/16 500; active `text` with 1.5px
`text`-coloured bottom border; inactive `text-muted`.

**Card** (`132-0`) — `#141416`, 1px `hairline`, radius 12, 16px padding, 6px gap; icon
well (28px, `field`, radius 8), 13/16 600 title, 11/14 muted body, kbd hints.

**Empty state** (`13L-0`) — dashed 1px `#2E2E33`, radius 8, 16×14 padding, 10px gap,
title `#D6D6DA` 12/16 550, body muted 11/14, list of kbd + action rows.

**Prompt composer** (ref `126-0`, not used) — `#141416`, 1px `#2E2E33`, radius 14, 16px padding, moldavite
halo shadow; 14/22 input; footer row with attach, agent picker, context chip, agent primary
button.

**Tooltip** (`145-0`) — `#26262A`, 1px `#333338`, radius 8, 8×10 padding, tooltip shadow;
title 12/16 600 + kbd, body `text-secondary` 11/14.

**Floating toolbar** → bulk-action bar (`112-0`) / **dock** (ref `BF-0`, not used) — `#141416`, 1px `#2A2A2E`, radius 10/13,
toolbar/dock shadow, 2px gap, separators 1×16–20 `#2E2E33`.

**Status bar** (`RG-0`) — 28px, `#141416`, top hairline, 12px padding, 16px gap, 11/14 muted
text, mono values, 1×12 separators, status dots.

**Agent card** (`11R-0`) — `moldavite-surface` fill, 1px `moldavite-border`, radius 10, 12px
padding, 10px gap; header (sparkle + agent name in moldavite 600 + `moldavite-text-muted`
context + count badge); list of proposed changes in 26px rows (16px icon well
`moldavite-well` with a mono glyph, `text-strong` label with `moldavite-text-muted` detail,
mono timestamp `moldavite-text-faint`); actions row (agent primary + agent outline).

**Progress bar** (`15L-0`; neutral `15S-0`) — 3px, radius 2, track `#22332A`, fill moldavite.

**Avatar** (ref `1V-0`) — 26px circle, 2px `chrome` ring, `#3A3A3F` fill, initial 11/14 650.
Coffer uses only the agent avatar (`158-0`): `moldavite-well` fill, 1.5px moldavite ring,
sparkle.

**Selection overlay** (`17K-0`) — molten 1–1.5px outline, 7×7 white handles with molten border,
molten measurement labels (mono 10/12 600, white). In Coffer this is the
selection/focus language for charts and tables (e.g. a selected bar or row range).

## Iconography

Inline SVG, outline style: stroke 1.1–1.3px, round caps and joins, no fills except the
sparkle (agent) and the brand mark. Sizes 10, 12, 14, 16. Colours: `text-muted` default,
`#B4B4B9` secondary, `text`/white active, `#6E6E73` chevrons, `#4A4A4F` disabled.
The four-point **sparkle** (`M6 0.5L7.3 4.7L11.5 6L7.3 7.3L6 11.5L4.7 7.3L0.5 6L4.7 4.7Z`)
is reserved for the AI agent.

## Applying the system to Coffer

**Colour semantics**

| Meaning | Treatment |
|---|---|
| Brand, current view, selection, focus | `molten` |
| Anything from an AI client over MCP: inbox suggestions, saved reviews, "agent connected" | `moldavite` (+ sparkle icon) |
| Unread/new | molten dot |
| Problem: over budget, overdrawn, failed import, reconciliation mismatch, destructive action | `danger` (see Status colours) |
| Caution: budget at 85% or more, unconfirmed transfer pair, import with skipped rows | `warning` |
| Success: import done, reconciled | Neutral: `text` plus a check icon. No success colour (green belongs to the agent) |

**Money**

- Amounts are Geist Mono, tabular figures, right-aligned, `--color-text`. **Colour never
  carries the sign**: outflows use a true minus (`−£12.40`, U+2212) and inflows a plus
  (`+£1,250.00`). Neither is green or red, because green means "agent" and orange means
  "selected".
- Signs mark direction, so a total whose label already gives the direction is unsigned:
  the Overview "Spent" and "Income" figures, the category amounts in "Spending by
  category", and the Spending and Income rows of a chart tooltip (`£2,481.90`). Net is
  signed because it can go either way (`+£968.10`). Single transactions and payments
  (table rows, recurring payments) always carry their sign (decided in D4.1).
- Each amount shows its own currency symbol. Foreign-currency transactions show the
  original amount below in mono 10/12 `text-muted` (`€14.50`).
- Transfers, including pot moves, are `text-muted` with a ⇄ icon. They're excluded from
  totals, and the dimming says so.
- Colour appears on money only when it means something beyond the sign: `danger` for a
  negative balance or an over-budget figure, `warning` for a budget at 85% or more.
- Thousands separators and decimal marks follow the user's locale; the currency's minor
  units decide the decimals (JPY shows none).

**Shell mapping (suggested, to be confirmed in Paper)**

| Reference design | Coffer |
|---|---|
| Title bar tabs | Views (Overview, Transactions, Trends, Budgets, Net worth, Inbox, Reviews) |
| Command bar (⌘K) | Search transactions, run commands (import, jump to month) |
| Left panel: pages + layer tree | Accounts tree with pots nested under their parent; categories |
| Canvas | Charts and tables |
| Inspector | Detail for the selected transaction/category/account; rule editor |
| Agent card + "Review N changes" | Inbox: AI-proposed categories, rules, transfer pairs |
| Status bar | Import status, last sync, "1 agent connected" (MCP), home currency |

## Status colours

Chosen to stay clear of both accents: `danger` is a rose, not a red, so it reads as
distinct from `molten` (ΔE 14.8 in OKLab ×100, against 8.8 for the first red tried, `#F26D78`). `warning`
reuses the amber that already exists in the file.

| Token | Value | Contrast | Use |
|---|---|---|---|
| `--color-danger` | `#E5598E` | 5.2:1 on canvas | Danger text, icons, 6px dots, chart overage |
| `--color-danger-soft` | `#E5598E1F` | danger text on it 4.5:1 | Danger banner / row background |
| `--color-danger-hover` | `#ED6F9B` | on-danger text on it 6.7:1 | Hovered solid danger button |
| `--color-danger-pressed` | `#D04A7E` | on-danger text on it 4.5:1 | Pressed solid danger button |
| `--color-on-danger` | `#1A0B11` | 5.6:1 on danger | Text on a solid danger button ("Delete import") |
| `--color-warning` | `#E7C24A` | 10.3:1 on canvas | Warning text, icons, dots, budget bars at 85% or more |
| `--color-warning-soft` | `#E7C24A1F` | warning text on it 8.1:1 | Warning banner background |
| `--color-on-warning` | `#1A1405` | 10.7:1 on warning | Text on solid warning (rare) |

Destructive buttons are outline by default (1px `danger` border, `danger` text); a solid
danger fill appears only in the final confirmation of a dialog.

## Data visualisation

**Approach.** Charts are monochrome by default, and colour appears only when categories are
being compared. Selection uses the same molten language as everywhere else. The palette is
in Paper (Foundations artboard); the chart designs below were drawn on the Overview artboard
(`22V-0`) in D4.1, which settled the bar width and added the line halo.

**Categorical palette.** Six colours, each with its own lightness step, so they stay
distinguishable when hue is lost. They avoid the hues of `molten`, `moldavite`, `danger` and
`warning`. Measured as OKLab ΔE ×100, the closest pair among the six colours and the
four accent/status colours is **12.2** for normal vision, **7.0** for protanopia and
**8.2** for deuteranopia (Machado 2009 simulation). Every colour has at least 3:1
contrast on `canvas` (WCAG non-text contrast).

| Token | Value | OKLCH | Contrast on canvas |
|---|---|---|---|
| `--color-chart-1` | `#C1E3FC` | 0.90 0.05 240 | 13.2:1 |
| `--color-chart-2` | `#FBA0CC` | 0.81 0.12 350 | 9.3:1 |
| `--color-chart-3` | `#B396E8` | 0.73 0.12 300 | 7.1:1 |
| `--color-chart-4` | `#1AA0B9` | 0.65 0.11 215 | 5.7:1 |
| `--color-chart-5` | `#5777C1` | 0.58 0.12 265 | 4.1:1 |
| `--color-chart-6` | `#92508C` | 0.53 0.12 330 | 3.2:1 |
| "Other" | 1px `text-muted` outline, `field` fill | — | outline 5.4:1 |

Rules:

- At most six named series; everything else is grouped as "Other". "Other" is drawn as an
  outline, not a seventh colour: every mid-grey that passes contrast collides with one of
  the six (or with `danger`) under colour-blindness simulation, while the outline
  treatment reads as "the rest" for everyone.
- Colour never stands alone: every series has a direct label or a legend row with its
  value. Categories keep the same colour on every chart (assigned by spending rank over the
  last 12 months, then stored on the category).
- The same colour marks a category in tables: an 8×8 square, radius 2, before the
  category name.
- Prefer a ranked bar list (category, mono amount, 3px bar) to a donut. Use a donut only for
  six slices or fewer, 2px gaps between slices, total in the centre as a display figure.

**Ranked bar list** (Overview `2M5-0`, drawn in D4.1). One entry per category, 12px apart
(`spacing-3`): a label line (8×8 marker, name 12/16 `text-secondary`, share in mono 11/14
`text-muted` in a 24px lane, amount in mono 12/16 `text` in the 96px amount lane), then
6px below (`spacing-1_5`) a 3px bar (`spacing-progress`, radius 2) in the category's
colour. Bars have no track: the longest entry fills the row, the rest scale to it.
Ordered by amount, with "Other" and "Uncategorised" last whatever their size. Their bars
repeat their markers: "Other" is a `field` bar with a 1px `text-muted` outline,
"Uncategorised" a 1px dashed `text-muted` outline with no fill, and its name is
`text-muted` as in the table.

**Income vs spending (monthly overview).** Drawn in D4.1 (Overview `2HP-0`). Spending
bars `text-secondary`, income bars `icon-faint`, in that order within each month; net
savings a 1.5px `text` line through the month centres, over the bars. 240px plot
(`calc(var(--spacing-6) * 10)`), y axis £0 to the next round step above the largest
value, five intervals, labels right-aligned in a 32px column 8px from the plot. A legend
row sits right of the chart title: 8×8 markers for the bars, a 12×1.5px line for net.
Hovered month: tooltip with the month as title and one row per series, plus the net
point (6px dot, 2px `canvas` ring); the tooltip opens on the side away from the
selection label. Selected month: the selection overlay around the bar pair (1.5px molten
outline 2px out, white handles) with a molten mono label 12px above it, aligned to the
pair's inner edge so it stays inside the plot; the selected month's axis label is `text`.
The selected month is the month the switcher shows.

**Chart chrome.**

| Element | Spec |
|---|---|
| Axis labels | Geist Mono 10/12, `text-muted` (not `text-faint`: it fails contrast) |
| Gridlines | Horizontal only, 1px `hairline`; no vertical gridlines, no chart border |
| Baseline / zero line | 1px `border-strong` |
| Bars | Radius 2 on the value end. Fixed width, not a share of the band: 16px (`spacing-4`), 2px apart within a group (`spacing-0_5`), groups centred in equal bands. (The proposed "group gap 40% of bar width" gave 38px bars at 1440, too heavy for a 28px-row UI) |
| Lines | 1.5px, round joins and caps; points hidden until hover (6px dot, 2px `canvas` ring). A line drawn over bars gets a `canvas` halo: the same path at 4.5px in `canvas` underneath, so 1.5px of canvas shows each side. Without it the `text` line over `text-secondary` bars measures 1.8:1 and vanishes at each crossing |
| Area fill | Series colour at 8% opacity |
| Budget line | 1px dashed `text-muted`; the part over budget is `danger` |
| Tooltip | Tooltip component; values in mono; series marker 8×8 radius 2 |

## Data table (Transactions)

The table sits directly on `canvas`, not in a card, using the list-row language. Drawn on the
Components artboard: table `XJ-0` (header, day groups, every row kind below), bulk-action
bar `112-0`.

| Part | Spec |
|---|---|
| Header | 28px, sticky, `canvas` fill, bottom `hairline`; section-label type. Sortable columns show an 8px chevron (`icon-faint`) on hover; the active sort column label is `text` |
| Day group header | 28px, 8px top padding, no fill: "Tue 14 Oct" 11/14 600 `text-secondary`, daily net right-aligned in mono 11/14 `text-muted` |
| Row | 28px, radius 6, 8px inline padding, 12px column gap; no row dividers (hairlines only between day groups) |
| Columns | Merchant (12/16 `text`, truncates, flexible) · Category (8×8 marker + 12/16 `text-secondary`, 180px) · Account (12/16 `text-muted`, 140px; hidden when filtered to one account) · Amount (mono 12/16, right-aligned, min 96px). The date lives in the day header |
| Hover / selected | `hover` fill / `molten-soft` fill with `text` labels; multi-select with ⇧/⌘ |
| Bulk actions | Floating toolbar centred 24px above the bottom edge: "3 selected · Categorise · Mark as transfer · Create rule · ✕" |
| Uncategorised | Dashed 1px `text-muted` marker + "Uncategorised" in `text-muted` |
| AI-suggested category | 10px sparkle + label in `moldavite-text`, 6px gap (`spacing-1_5`) so the label stays in the lane of the 8px markers (10 + 6 = 8 + 8); approve/reject from the inspector or the inbox |
| Transfer | ⇄ icon, merchant and amount in `text-muted`, category "Transfer · Savings pot" |
| Flagged (reconciliation) | 6px `danger` dot before the amount; tooltip explains |
| Inline edit | Enter or double-click on Category opens a select popover in place; Esc cancels, Enter commits |
| Empty | Empty-state component: "No transactions yet" + kbd row "⌘I Import a statement" |

One density only (28px rows). The inspector shows the full detail: raw description in mono
11/14, notes, original currency, rule that categorised it.

## Interaction states

| State | Spec |
|---|---|
| Hover | Rows, tabs, icon buttons, outline buttons, chips: `hover` fill. Neutral primary: `#FFFFFF`. Agent primary: `#8FD09C` (text contrast 10.2:1). Agent outline: `moldavite-surface-raised`; agent ghost: `moldavite-well`. Solid danger: `danger-hover` (`#ED6F9B`, 6.7:1) |
| Pressed | Neutral primary: `text-strong` (`#D6D6DA`). Agent primary: `#6DB27B` (7.2:1). Agent outline: `moldavite-well`; agent ghost: `moldavite-border`. Solid danger: `danger-pressed` (`#D04A7E`, 4.5:1). Everything else: `control-active` |
| Focus (keyboard, focus-visible only) | 1.5px `molten` ring, 2px outside the element, radius = element radius + 2. Rows in scrolling lists draw the ring inset so it isn't clipped. Molten on canvas is 6.2:1 |
| Selected | `molten-soft` fill; can combine with focus |
| Disabled | 40% opacity (`--opacity-disabled`) of the whole control as one group, no hover or pressed, default cursor. In GPUI, controls with a fill under their content paint the group's composite colours instead of element opacity (see "Rendering findings (D2.6)" → "Closed") |
| Loading | Skeleton blocks in `field`, radius 4, opacity pulsing 60% ↔ 100% over 1.2s. Import progress uses a neutral progress bar (track `border`, fill `text`); the moldavite bar is for agent work only |

## Motion

Paper has no motion token type, so this file is the source for these values; the names are
the theme module's (`motion::FAST` …), not Paper tokens.

| Name | Duration | Use |
|---|---|---|
| `--motion-fast` | 90ms | Hover and pressed colour changes |
| `--motion-base` | 150ms | Toggles, segmented control, tooltip and popover in |
| `--motion-slow` | 220ms | Panels and inspector open/close, sheets, the import dialog |
| `--motion-chart` | 320ms | Chart data changes (month switch, filters) |

- Easing: `cubic-bezier(0.2, 0, 0, 1)` for entering and moving; exits use
  `cubic-bezier(0.4, 0, 1, 1)` at two-thirds of the duration. No bounce or spring.
- Tooltips appear after 500ms; moving between adjacent targets shows the next one
  immediately; hiding is instant.
- Charts: bars grow from the baseline on first render only; later changes animate values in
  place. Numbers never count up. No animation on window resize.
- Respect the OS reduced-motion setting: every animation goes to 0ms, opacity fades
  included, and repeating ones (the skeleton pulse) hold still. GPUI has no per-animation
  opt-out (`App::reduce_motion` stops every GPUI and Base animation), and an instant change
  is acceptable for reduced motion. Delays aren't motion: tooltips still wait 500ms.
  (Decided in the D2.6 closure while the user was away; review.)

## Proposed: finance screens

Window minimum 1280×800, designed at 1440×900. Shell: title bar with view tabs
(Overview, Transactions, Trends, Budgets, Net worth, Inbox, Reviews; Inbox carries a
moldavite count badge when suggestions are pending) and the ⌘K command bar; left panel
272px; inspector 296px (collapsible); status bar 28px.

**Left panel.** ACCOUNTS section: one row per account with its mono balance right-aligned;
pots nested 20px under their parent with a pot icon. Net worth total at the top as a
mono 13/16 600 figure. CATEGORIES section below, used as a filter.

**Overview** — drawn in D4.1 (no longer Proposed): artboard Overview (`22V-0`), October
2026 with "All accounts" (no account selected in the left panel). The shell is the
Transactions shell with the Overview tab active and no inspector: nothing on Overview
selects a single record, so the canvas takes the full 1168px. Canvas (`24V-0`): 16px top,
24px inline and bottom padding (`spacing-6`, roomier than the table views), 24px between
sections.
- Month switcher (`2H2-0`): 28px icon buttons with 12px chevrons either side of the month
  in title type (13/16 600, "October 2026"), pulled 8px left so the chevron's glyph lines
  up with the content edge.
- Figures (`2HB-0`): Spent (display figure), Income and Net (figure), 48px apart and
  top-aligned so their caption labels ("Spent", 11/14 500 `text-muted`) share a line. Each
  has a caption delta vs last month below, in `text-muted` ("+8% vs Sep", "Same as Sep",
  "−16% vs Sep"). Spent and Income are unsigned, Net is signed (see "Money").
- Agent card, compact (`2LQ-0`), at the right end of the figures row, centred on it.
  Moved from the right column, where the full card and the recurring list didn't fit
  together in the 326px left under the chart, while the figures row had about 500px
  free on the right. Header as the agent card's (sparkle, "Claude", "Claude Code
  · 2h ago", count badge), then the pending suggestions by kind ("9 categorisations · 2
  rules · 1 transfer pair", `text-strong` with `moldavite-text-muted` separators), aligned
  under the agent name; an agent primary "Review" on the right opens the Inbox. 12px
  padding (14px left), 24px gap. Only shown while suggestions are pending.
- Income and spending chart (`2HP-0`): see "Data visualisation". Title "Income and
  spending" (section title) with caption "Last 12 months", legend on the right. October is
  selected, July shows the hover state.
- Lower row (`2M4-0`), two equal columns 48px apart, each headed by a section title and
  caption: "Spending by category · October" as a ranked bar list (`2M5-0`, see "Data
  visualisation"), and "Recurring · Next 30 days" (`2NY-0`) with the period total in mono
  11/14 `text-muted` on the right. Recurring rows are 28px list rows inset 8px like the
  table body: marker + merchant (`text`), cadence (`text-muted`, 96px), next date (mono
  11/14 `text-muted`, 48px, right), amount (mono 12/16, the 96px amount lane, signed). A
  price rise replaces the cadence with "Up from £11.99" in `warning` and puts a 6px
  `warning` dot before the amount, as the flagged-row dot does with `danger`. Recurring
  looks forward from today, whatever month the switcher shows.
- Fixture data: Spent £2,481.90 across Groceries £612.40, Eating out £438.15, Bills
  £402.60, Shopping £356.98, Transport £214.30, Entertainment £98.67, Other £300.27 and
  Uncategorised £58.53; income £3,450 a month (£4,280 in March); monthly spending from
  £1,980.10 (Jan) to £3,120.75 (Dec). Category colours as on Transactions.

**Transactions** — drawn in D3.1 (no longer Proposed): artboards Transactions (`18E-0`,
one transaction selected) and Transactions · multi-select (`1NP-0`). Fixture data: five day
groups (Mon 9 – Thu 5 Nov 2026; the canvas clips the last one, as in a scrolled list),
across Current account and Joint account, with every row kind: long merchant names
truncating, pot transfers both ways ("To Rainy day", "From Holiday"), a euro payment with
its original amount, a refund (ASOS, `+£34.00` in Shopping), uncategorised, AI-suggested
and flagged rows.
- Left panel: no account row is selected, which means "All accounts", so the table shows
  the Account column. Selecting an account filters the table and hides the column.
- Toolbar (`1DB-0`): 12px block / 16px inline padding, 8px gap. Search field
  (`--container-search-field`, field fill, hairline border, radius 6, search icon). Filter
  chips: Account, Category and Date open a menu, so they end in an 8px `icon-faint` chevron;
  "Uncategorised 14" is a toggle, so it ends in a mono count. A spacer, then the outline
  "Import" button with a 16px kbd "⌘I" inside (mono 10/12, 4px right padding).
- Table header (`1E9-0`) spans the canvas edge to edge, 16px inline padding. No column shows
  a sort chevron: the default order is by date, which the day groups already show. The
  body (`1EJ-0`) is inset 8px, so row text lines up with the header at 16px.
- Inspector, one transaction (`1A0-0`), in 14px sections divided by hairlines. Header:
  merchant as title (13/16 600), amount as display figure (mono 32/38 600), caption
  "Card payment · 14:32" (time in mono; the date is a field below). Field rows: Date,
  Account (select), Category (select; a suggested category shows the sparkle and
  `moldavite-text`), Note. For a suggestion, an agent card follows the fields: header as
  the agent card's without the badge, "Categorise as Shopping" (`text-strong`, category in
  `moldavite-text`), the agent's reason in caption `moldavite-text-muted`, then "Approve"
  (agent primary, fills) and "Reject" (agent outline). Then "Raw description" (mono 11/14
  `text-secondary`, wraps) and "Rule" ("No rule matched" + outline "Create rule from this").
  Not drawn yet: transfer pairing (a transfer row's inspector) and the original-currency
  detail (the euro row's inspector).
- Multi-selection (`1NP-0`): three uncategorised rows selected, the bulk-action bar centred
  24px above the canvas bottom, over the rows. The inspector summarises the selection:
  "3 transactions" title, total as display figure, caption with what they share, one
  Category select ("Choose a category", dashed marker) that sets all of them, and a
  "Selected" list (mono date, merchant truncating, mono amount; 28px rows).
- Title bar at 1440: the seven view tabs take about 640px, so the command bar can't be
  centred. It follows the tabs, and the window controls stay right.

**Transactions additions** — drawn in D6.3. Four 1440×900 artboards duplicated from
Transactions (`18E-0`), plus a states board. Same shell, fixture data and inspector
sections as D3. Every value is a bound token; the typed values are as on D3. One new
token, `--container-dialog`.
- **Split and tags** (`2Q3-0`). "Waitrose & Partners, Clapham Junction", −£52.16 on Joint
  account, is split and selected. Inspector sections: Header, Details, Split, Source, Rule.
  - **Details** shows the imported source fields (Date, Account) read-only: plain value
    text with a trailing 10px `lock` icon in `icon-faint`, and no field fill. The value
    text lines up with field text (8px inset). The editable user fields are field rows:
    Merchant (the display merchant; it starts empty and falls back to the imported
    merchant), Tags and Note. "Bank note" is the imported note, shown read-only like Date.
    It sits directly above Note (the user's own), so the two are never confused. The Bank
    note row only appears when the import carried a note.
  - **Tags** sit in a wrapping box at field width, 4px apart. Each tag is a 24px pill
    (`radius-pill`, `field` fill, 8px inline padding, caption `text-secondary`). A private
    tag ("Mum's birthday") leads with a 10px `lock` in `text-muted`: private tags are never
    sent over MCP. A dashed "+ Tag" pill (1px dashed `border-strong`, `text-muted`) opens
    a tag menu that also creates new tags.
  - **Split section.** Title row: "Split" plus an outline "Remove split". One line per
    category: a category select (marker + label, fills), a 96px mono amount field
    (`column-amount`), and a 24px remove icon button. Footer: "+ Add line" (ghost) on
    the left; on the right, "Remaining", the mono remainder, and a 12px status icon in a
    24px lane that lines up with the remove buttons. Each line is 28px tall, its parts 6px
    apart. The transaction's amount is locked: it's the header figure, read-only, and the
    lines must sum to it exactly (ARCHITECTURE "Splits").
    "Add line" prefills the new line with the remainder, so the common two-way split is
    one action. That new line is drawn with the focus ring on its amount.
  - **Split states** (states board `4XF-0`). Not balanced: "Remaining" and the signed
    remainder in `warning`, with an `alert` icon, and a caption explains the difference
    ("The lines are £3.60 more than the transaction."). Edits save only once the remainder
    is £0.00; an unbalanced split isn't saved, and leaving the row keeps the last balanced
    split. Balanced: "Remaining" in `text-muted`, `£0.00` in `text`, a `check` icon in
    `text-muted`.
  - **Split row in the table.** The 8px category marker is split into a left and a right
    half in the two largest lines' colours (`chart-1`/`chart-3`). The label lists every
    line's category ("Groceries, Shopping, Other") and truncates.
  - **Source section.** Raw description (mono 11/14 `text-secondary`), then the batch
    ("monzo-joint-november.csv · imported 9 Nov", caption `text-muted`), then outline
    "Exclude…" and "Undo import…". Undo import opens the existing batch-undo confirmation.
- **Excluded** (`3FE-0`). The toolbar gains an "Excluded" toggle chip after
  "Uncategorised". It's off by default, and excluded rows are then hidden. With it on (as
  drawn), excluded rows appear in place in their day group. The chip shows no count: with
  a count it overflowed the toolbar at 1440 by about 6px. Instead the status bar reads
  "328 transactions · 2 excluded".
  - **Excluded rows.** Merchant and account in `text-muted`. The category cell reads
    "Excluded · {reason}" with a 12px `excluded` (circle-slash) icon in the marker lane.
    The amount is struck through in `text-muted`. Excluded rows don't count in the day
    total (Sat 7 Nov reads −£160.54 without the Odeon row). Selected: the usual
    `molten-soft` fill, with the muted text lifted to `text-secondary`, not white, so the
    row still reads as excluded.
  - **Inspector.** The display figure is struck through in `text-muted`. Below the header
    is a card (`float`, 1px `hairline`, `radius-surface`, 12px padding). Its first row has
    the 12px `excluded` icon, "Excluded from totals" (section title) and the date it was
    excluded (mono caption, "8 Nov", right). Below that: the reason ("Bank error", body
    `text-strong`), the user's details (caption `text-muted`, wraps), then outline
    "Include again" and outline "Edit reason…". Details still lets the user change
    category, tags and note, since those don't affect totals. Source keeps only "Undo
    import…".
  - **Exclude dialog** (states board `51L-0`): "Exclude “Odeon Luxe Leicester Square”?",
    a body explaining what changes and that it can be undone, Reason (select:
    Duplicate, Bank error, Disputed, Other) and Details (optional text, required for
    Other). Cancel (secondary) and Exclude (neutral primary, not destructive: excluding
    is reversible and drops nothing).
  - **Duplicate exclusions also leave the account balance.** The other reasons are money
    that really moved. So for a duplicate the card title reads "Excluded from totals and
    balance", and the dialog body says so when Duplicate is picked. The drawn copy is
    the Bank error case.
- **Rule from an edit** (`3WT-0`). The user has just set VINTED *KX8F2Q's category to
  Shopping. The edit is already saved (Uncategorised 14 → 13, Shopping 18 → 19). A
  neutral card under Details asks "Create a rule from this edit?". Contents:
  - the proposed rule, "Merchant contains `VINTED` → Shopping", with the match text in mono;
  - the match count, "6 earlier transactions match" (`text-strong`);
  - a caption that splits the matches and names the risk: "4 are uncategorised; 2 have a
    category you set, which the rule would change";
  - outline "Create rule…", which opens the rules editor (D6.7) prefilled with the
    apply-to-history preview, and ghost "Not now".

  The card is the app's own prompt, not an agent suggestion, so it is neutral, not
  moldavite. It appears after a category edit when no rule matched the row and the
  proposed pattern matches at least one earlier transaction. It never appears for
  transfers or split rows. It goes away on "Not now", which is remembered for that
  pattern, or when the selection changes.
- **Manual (cash account)** (`4D7-0`). The left panel has a "Cash" account (£23.00,
  `cash` kind) selected, so there is no Account column.
  - **Toolbar.** On a manual account the Import button becomes outline "New transaction"
    with kbd "⌘N". ⌘N works on every account; it opens the dialog below with the
    selected account preset.
  - **Rows.** Manual rows look like imported rows. The account already says where they
    came from, and a separate marker would compete with the category marker. The rows
    include a transfer from Current account (a cash withdrawal, paired as a transfer).
  - **Inspector.** Every field is editable: Date and Amount (mono, with a `GBP` unit
    suffix) as fields, Account as a select. The Split section shows only an outline
    "Split" button and a caption until the user splits. Source reads "Added by you on 7
    Nov" and has a small destructive outline "Delete transaction…": 24px, radius 6, 8px
    padding, 1px `danger` border, caption 500 `danger`. That's the outline button's size,
    so it doesn't outweigh the section. Only manual rows can be deleted: imported rows are
    excluded instead.
  - **New-transaction dialog** (states board `529-0`, `--container-dialog` wide). Field
    rows: Account, Date, Amount, Merchant, Category, Note. Amount is an Out/In segmented
    control (Out first, the common case) beside an unsigned field with a `£` prefix and a
    currency unit, so the sign is never typed. Cancel (secondary) and "Add transaction"
    (neutral primary).
- **Every unsplit transaction** has the Split section, as on `3WT-0` and `4D7-0`: title
  "Split", outline "Split" button, caption "Divide this amount across categories." It sits
  between Details and Source.
- **Undo notification** (states board `53O-0`). After a change to many existing rows
  (bulk categorise, apply a rule, merge duplicates), a floating bar replaces the
  bulk-action bar in its place. It is drawn with `float`, a `border` line, `radius-lg` and
  the Toolbar shadow. Contents: a 14px `check`, the change in body text ("Categorised 3
  transactions as Shopping"), outline "Undo", and a 24px dismiss icon button. Undo
  reverts the `change_batch`. The bar stays until dismissed or until the next change;
  undo stays available afterwards from the batch history (D6.7).
- **Dialogs** (`51L-0`, `529-0`) use `--container-dialog` (440px) for a confirmation or a
  short form, `float` fill, `border`, `radius-lg` and the Dock shadow. Layout: 16px padding,
  16px gaps, title in title type, footer actions right-aligned with 8px gap. Field rows as
  in the inspector (64px label column). The backdrop is still provisional ("Implementing
  in GPUI"). The Import dialog keeps its own 760px width.
- **States board** (`4X9-0`, 1440 wide): split editor states (`4XF-0`), dialogs and
  notification (`51I-0`), and the new table rows (`542-0`): split, split selected,
  excluded, excluded selected, manual.
- **Data-model questions: decided** (2026-11-10, `ARCHITECTURE.md` → "Decision record:
  Transactions additions"):
  1. *Exclusion:* reason key (`duplicate`, `bank_error`, `disputed`, `other`) plus
     optional note (required for Other) plus `excluded_at`. Only Duplicate also leaves
     the account balance; the other reasons are money that really moved.
  2. *Every transaction has `created_at`.*
  3. *The rule prompt needs at least one earlier match.*

**Import (dialog, 760px).** Steps in a segmented control: File → Map → Review.
- Map: left column of field rows (detected preset "Monzo" as a select at the top, then date
  column + format, amount, merchant, description, ...); right column, a live preview of the
  first 10 parsed rows. Rows that fail to parse get the flagged style.
- Review: "312 new · 16 already imported · 0 flagged" in mono, reconciliation result, and a
  neutral primary button "Import 312 transactions".

**Trends.** A category or merchant picker at the top (select + search). Below it: the
pick's monthly spending over 12 months (bars, `text-secondary`, average as a dashed
`text-muted` line); top merchants as a ranked bar list; and recurring payments as list
rows (merchant, cadence, mono amount, next expected date), where an increase since the
last payment is flagged with `warning`.

**Inbox.** Agent cards grouped by kind (Categorisations, Rules, Transfer pairs), each with
its source ("Claude Code · 2h ago"). Each item has approve ✓ / reject ✕ icon buttons; each
group has "Approve all (12)" (agent primary) and "Reject all" (agent outline).

**Budgets.** One row per category: name, "£212 of £300" in mono, a 3px bar (fill
`text-secondary`; `warning` at 85% or more; `danger` over 100%, with the overage drawn
past the end), remaining amount right-aligned.

**Net worth.** Line chart (`text` line, 8% area) over 12 or 24 months, then account
balances with their change for the period. Accounts with no balance data (e.g. Monzo
exports) show a neutral card: "Enter today's balance to include this account".

**Reviews.** List of saved AI reviews (period, title, source client) on the left; the
selected review in reading type (13/20 body, 15/20 headings) on the right.

**Light theme.** Not in v1.

## Setting up the Coffer Paper file

D1 in [`ROADMAP.md`](ROADMAP.md). The file starts empty; nothing is copied from the
reference file except the values recorded here.

1. Create the file and record its URL under "Source of truth". ✅ Done (D1.1).
2. Create every token in this file: the reference tokens, with the three weight tokens at
   400/500/600 plus 700, and the colour and opacity tokens proposed for the artboards.
   (Motion durations aren't a Paper token type, so they stay in this file.) ✅ Done (D1.2),
   plus the "tokens named in D1.2".
3. Build a foundations artboard and a components artboard from those tokens only, and
   replace the reference node IDs in "Components" with the Coffer file's node IDs.
   Foundations ✅ done (D1.3, artboard `1-0`). Every colour, size, weight, tracking, line
   height, gap and radius on it is a bound token. The only typed values are the 1px swatch
   ring (`inset 0 0 0 1px #FFFFFF1F`, from "Elevation") and the 1px outline on the chart
   "Other" swatch, because Paper has no shadow or border-width token type.
   Components ✅ done (D1.4, artboard `CJ-0`), with the size tokens it needed ("Tokens
   named in D1.4"). Every colour, size, weight, gap, radius and opacity is a bound
   token. Typed values: 1px/1.5px border, ring and outline widths, shadow geometry, SVG
   icon paths, and data-driven bar lengths (percentages). The "Components" section above
   now quotes the Coffer node IDs.
4. Design the screens above (D3, D4, D6), then replace the "Proposed" text here with the
   Paper node IDs. Transactions ✅ done (D3.1, artboards `18E-0` and `1NP-0`): composed
   from clones of the Components artboard, every value a bound token. Typed values as on
   Components: 1px borders, the bulk-action bar's shadow, SVG icon paths. Overview ✅ done
   (D4.1, artboard `22V-0`): the shell cloned from Transactions, the canvas written from
   the specs here, every colour, size, gap, radius and type value a bound token, with no new
   tokens. Typed values: 1px/1.5px borders and outlines, the 4.5px halo stroke, the
   tooltip shadow and swatch ring, the net line's SVG points, data-driven positions (bar
   heights and widths as percentages, the hover point's −5px centring offset) and the
   hover point's 50% radius (a 10px circle has no radius token). Transactions additions
   ✅ done (D6.3, artboards `2Q3-0`, `3FE-0`, `3WT-0`, `4D7-0` and the states board
   `4X9-0`): duplicated from Transactions, additions built from the specs here, every
   value a bound token, one new token (`--container-dialog`). Typed values: 1px borders,
   the 1.5px focus ring, the Toolbar and Dock shadows, SVG icon paths.
5. Export `design/tokens.json` and `design/tokens.css` from the Coffer file. ✅ Done (D1.5):
   122 tokens, content hash `572f3e11`. Re-exported in D3.1: 123 tokens, content hash
   `86e4aeb1`. Re-exported in D6.3: 124 tokens, content hash `b442a048`. Re-export
   whenever a token changes (rule 4).

## Implementing in GPUI

- **Fonts:** bundle Inter and Geist Mono with the app (both SIL OFL 1.1, compatible with
  `MIT OR Apache-2.0` distribution; include their licence files). Fraunces is not used.
  Implemented: the files (Inter 4.1, Geist 1.9.2) and their `OFL.txt` are in
  `assets/fonts/`; `src/theme/fonts.rs` compiles them into the binary, registers them at
  startup, and sets them as `gpui-component`'s UI and mono families. Startup fails if a
  face doesn't resolve to itself, so a missing weight can't silently fall back.
- **Weights:** 400/500/600/700, so the app bundles the static Inter instances for those
  four weights (and Geist Mono 400/600) and doesn't depend on variable-font rendering.
- **Rendering findings (D2.6).** How GPUI draws the values that are hard to carry over from
  Paper. Read from the `gpui-pre` 0.3.11 and `gpui-pre-wgpu` 0.3.11 source (the Linux
  renderer; macOS's Metal renderer isn't checked) and the bundled font files, not measured on
  screen. The two defects found are fixed and tested. The three questions left open are closed
  at the end (decided while the user was away; review).
  - **Alpha colours.** The translucent tokens (`molten-soft`, `moldavite-soft`,
    `danger-soft`, `warning-soft`, the swatch ring, shadow colours) are `Rgba` with alpha.
    The renderer draws to a non-sRGB surface (`Bgra8Unorm` or `Rgba8Unorm`) and writes colours
    without a gamma conversion, so blending works on sRGB-encoded values, as a browser's does:
    `molten-soft` over `canvas` gives the same pixel as in Paper. A border is composited over
    the element's own fill (CSS `background-clip: border-box`), so a translucent border looks
    like an inset ring; the token board's swatches draw their ring that way.
    **Difference:** GPUI's `opacity` isn't group opacity. It multiplies into the alpha of each
    thing the element draws, so a label over a fill blends with the already-faded fill, where
    CSS fades the finished group. It shows only where a label sits on a fill. At
    `opacity-disabled` on `canvas`, element `opacity` gave the neutral primary's label
    `#474749` on `#6D6D6F` (1.8:1) instead of Paper's `#141416` (3.6:1); solid danger dropped
    from 1.9:1 to 1.4:1, agent primary from 2.5:1 to 1.5:1, and the agent ghost's label
    shifted slightly. Outline, toolbar, icon and text buttons, chips and rows have nothing
    under their labels and match (agent outline included: measured label `#526557`, border
    `#212A25`, Paper the same within P3 rounding). **Fixed:** see "Closed" below.
  - **1px hairlines.** GPUI snaps every box edge to device pixels (halves round towards zero)
    and draws a border a whole number of device pixels wide, at least one. A `hairline` is
    always crisp: 1 device pixel at 1x, 2 at 2x, 1 at 1.25x and 1.5x (so a little under 1
    logical pixel there, as in a browser). 1.5px lines (`size::STROKE`, `size::FOCUS_RING`)
    draw 1px wide at 1x and 1.5px at 2x, the same as a browser draws a 1.5px border (CSS
    Values 4, "snap as a border width": anything over 1 device pixel is floored). At
    fractional scales GPUI rounds where CSS floors: 1.5px at 1.25x is 2 device pixels in GPUI
    and 1 in a browser. Edges at half-pixel positions move by half a pixel, so keep geometry
    on whole pixels, or snap it for the window's scale factor as the focus ring does. Dashed
    borders use a dash twice the width and a gap equal to it (the uncategorised marker: 2px
    dashes, 1px gaps).
  - **Shadows.** GPUI's `BoxShadow` has offset, blur, spread, colour and inset, and an element
    takes a list of them, so every row of "Elevation" can be expressed, including the
    composer's halo-plus-drop pair. Inset shadows paint over the fill and follow the corner
    radii. **Fixed:** GPUI's `blur_radius` is the Gaussian's σ, but a CSS blur radius is 2σ.
    `tokens::shadow::Shadow::box_shadow` passed the CSS value straight through, which would
    have drawn every shadow twice as soft; it now halves it (test
    `shadows_convert_the_css_blur_to_gpui_sigma`). The shader approximates the Gaussian (an
    `erf` along one axis, four samples across the other). A drop shadow is clipped by any
    ancestor that clips its content, so floating surfaces render through `deferred`, which
    draws without the ancestors' clip, as stock popovers do. No Coffer component draws a
    shadow yet (tooltip, bulk-action bar and composer aren't built), so none has been
    compared on screen; compare the first one against Paper. **D5:** the bulk-action bar's
    `toolbar` shadow matches Paper's export within the diff threshold at 1x and 2x.
  - **Outside focus ring.** See "Focus ring" below. **Fixed:** at 1x GPUI drew the 1.5px ring
    1px wide and snapped its 3.5px outset towards zero, so the gap came out 3px on the top and
    left and 2px on the bottom and right. The ring now snaps its width and gap to device
    pixels for the window's scale factor before building the outset: a 1px ring 2px out at
    1x, 1.5px and 2px at 2x, corners still the element's radius + 2. Tests:
    `outside_ring_lands_on_whole_pixels_at_1x`,
    `ring_snaps_to_device_pixels_at_fractional_scales`, and the headless
    `outside_ring_keeps_an_even_gap_at_1x`. The headless test window runs at 2x, where the
    old geometry was already exact, so the earlier bounds test couldn't catch it.
  - **Tabular mono figures.** Every glyph in both bundled Geist Mono weights is 600 units
    wide (checked: digits, `, . + % £ € $`, U+2212 minus, no-break space), so its figures
    are tabular without a `tnum` feature, and it has none. Right-aligned amount columns line
    up. Not in Geist Mono: ⇄ (U+21C4, not in Inter either), ⌘ (U+2318), thin space (U+2009)
    and narrow no-break space (U+202F). For a missing glyph GPUI uses a system font chosen
    by the platform (DejaVu Sans for ⇄ on the dev machine), so its shape and width vary by
    OS. So: the Kbd wrapper sets ⌘ in Inter, which has it, or draws it as an icon; the agent
    card's transfer-pair well and the table's transfer rows draw ⇄ as `Glyph::Transfer`; amount
    formatting uses the plain or no-break space, never U+2009 or U+202F. Inter's figures are
    proportional by default. It has `tnum`, and GPUI passes `Styled::font_features` to the
    shaper (cosmic-text on Linux). The only Inter figures today are badge counts, which never
    line up in a column, so nothing sets it yet.
  - **Animation easing.** `gpui_kit::base::Easing::cubic_bezier` evaluates a CSS
    cubic-bézier, so `motion::EASE_ENTER` and `EASE_EXIT` carry over exactly.
    `component.rs` puts them on the stock theme with the durations and critically damped
    springs, and the stock components that animate read them from there. Coffer's own
    animations use `Animation::new(duration).with_easing(...)`, which takes any
    `Fn(f32) -> f32`, so pass a closure over `Easing::sample`; exits run for
    `motion::exit(duration)`. Reduced motion: `gpui_kit::init` reads the OS setting (macOS,
    Windows, the XDG desktop portal on Linux) into `App::reduce_motion`, and every GPUI and
    Base animation then jumps to its end, opacity fades included, as "Motion" now specifies;
    repeating animations hold still. Coffer's buttons still change colour instantly (no
    `motion-fast` transition; see "Stock components").
  - **Closed** (decided while the user was away; review). The three questions D2.6 left open:
    1. *Disabled solid buttons: match Paper.* No new tokens, so Paper is unchanged: the
       disabled colours are derived from the existing tokens and `opacity-disabled`. A
       disabled button with a fill under its content (neutral primary, destructive solid,
       agent primary, agent ghost, or any button with a shortcut's key cap) skips element
       `opacity` and paints each colour opaque, as its composite at 40% over the surface the
       button sits on (`ui::faded`; `Button::with_surface`, `canvas` by default,
       `moldavite-surface` on the agent card and agent suggestion). That's the pixel CSS
       paints for the faded group. The stock button replays Coffer's instance style inside
       its disabled state, so the composites go in as ordinary fill and text colours.
       Buttons without a fill keep element `opacity`, which is already exact for them; that
       includes agent outline, which measured as matching. Measured on the showcase's
       disabled column against Paper's `D4-0` export (colour-managed, over `canvas`): neutral
       primary `#6D6D6F`/`#141416` (Paper `#6D6D70`/`#141416`, 3.6:1), agent primary
       `#405D47`/`#141816` (`#405D46`/`#141716`), solid danger `#6A3248`/`#191316` (the same),
       agent ghost label `#526557` (`#536556`). Every difference is 1 level, from P3
       rounding. Tests: `disabled_buttons_keep_papers_label_contrast`,
       `only_content_on_a_fill_is_composited`,
       `stock_focus_style_is_off_and_disabled_fades_as_a_group` (`src/ui/button.rs`) and
       `faded_colours_are_the_css_group_composite` (`src/ui.rs`).
    2. *Reduced motion: 0ms for everything.* "Motion" now says every animation, opacity
       fades included, is 0ms under reduced motion. GPUI has no per-animation opt-out, and an
       instant change is acceptable for reduced motion. That is what `App::reduce_motion`
       already does, and `gpui_kit::init` (through `gpui_base::init`) sets it from the OS, so
       no code was needed. Only the `motion` module's doc comment changed.
    3. *Agent card ⇄: `Glyph::Transfer`.* The transfer-pair well draws the table's transfer
       icon in place of the mono ⇄, in the wells' glyph colour (`moldavite`) at the wells'
       glyph size: the 12px icon scaled to the 11px mono size (1em, as the glyph it replaces;
       1.1px strokes, inside "Iconography"). It sits centred where the → glyphs sit; its ink
       is 9×8px against 8×8 for Paper's fallback ⇄. The showcase's two agent cards show it.
       Tests: `kinds_use_the_paper_glyphs`, `a_mark_icon_matches_the_well_glyphs`.
       Remaining: Paper's agent card (`11R-0`; the Overview and Transactions artboards have no
       ⇄ well) still sets ⇄ as text in the well, so it renders from a browser fallback font.
       Swap in the Transfer icon there when convenient. A ⇄ inside text (the showcase's
       "Savings ⇄ Current" detail, which is Paper's copy, or anything an agent writes) still
       comes from a fallback font.
- **Fidelity findings (D5).** The Transactions screen (`src/transactions.rs`, fixture data)
  rendered headlessly by `examples/snapshot.rs` (GPUI's headless platform with the real
  text system and the wgpu renderer, 1440×900 at 1x and 2x) and diffed pixel by pixel
  against Paper's PNG exports of `18E-0` and `1NP-0`. Linux only; macOS's Metal renderer
  and Core Text aren't checked.
  - **Colour: exact.** Paper exports PNGs tagged Display P3 (an `iCCP` profile from
    Chrome/Skia), so its saturated pixels read as different numbers: `moldavite` is
    `#8CC28F` in the file, `chart-4` `#4B9EB6`. Converted to sRGB, the file matches
    the tokens to within 2 levels, and GPUI paints the token values exactly. Greys are the
    same in both spaces. Colour-manage Paper exports before comparing them.
  - **Layout: exact.** Every box edge (bars, panels, rows, hairlines, fields, chips,
    buttons, the agent card, the bulk-action bar) lands on the same pixel at 2x. 0.16% of
    pixels at 1x have no match within 1px, all of them on glyph edges.
  - **Text: within half a pixel.** Glyph shapes, widths and positions along the line
    match. Vertically, baselines differ by at most 1 device pixel at 2x (2 for the
    wordmark), and so by 0–1px at 1x. They go both ways and land on plain and tracked text
    alike. GPUI centres the font's unrounded ascent and descent in the line height; Chrome
    rounds them first. Boxes centred on a half pixel (a 28px control in the 39px inside a
    40px bar with a bottom hairline) round down in GPUI and up in Chrome at 1x. Not
    visible in use; not worth correcting.
  - **GPUI pitfalls found, fixed in Coffer's code:** (1) a `list` lays each item out alone
    at the list's width and ignores the list's own padding, so items need a full-width
    wrapper that carries the inline padding; (2) a virtual list measures an item by its
    border box, so a margin on an item overlaps the next one. The day header's 4px above
    its divider is now padding on an outer box; (3) an `svg` takes its colour from its
    own style, not its parent's text colour (the left panel's account icons weren't drawn);
    (4) a div whose only extent is its own 1px border paints nothing, so `ui::separator`
    is a filled 1px box (the view-tab and status-bar rules were missing).
  - **Not covered by the snapshots:** behaviour on a real display (hover, the focus
    ring under keyboard use, scrolling, window drag and resize), and scrolling performance
    beyond the fixture's 25 rows. Selection by click, ⇧-click, ⌘-click and the arrow keys,
    the bulk-action bar and the agent suggestion's approve and reject have headless tests.
- **Theme module:** one Rust module in `crates/app` maps every token above to a constant
  or theme field, generated from or checked against `design/tokens.json`. Components
  never contain raw colour or size literals.
  Implemented in `src/theme/tokens.rs` (moves to `crates/app` in M1). Its drift test maps
  every token in `design/tokens.json` to a constant (following `var()` aliases such as
  `--color-on-light`), so the build fails when the export adds, removes or changes a token
  until the theme module follows. A second test checks the snapshot came from the Coffer
  file.
- **Letter spacing:** GPUI's text style has no letter-spacing property, so a role's
  tracking (−0.01em, +0.06em caps, −0.025em display) is lost on a plain text child.
  `ui::tracked::Tracked` applies it (D5): it shapes the line with GPUI's text system and
  paints each glyph itself, moved right by the tracking once per character before it, and
  is `tracking × characters` wider than the shaped text (CSS also spaces after the last
  character). Measured against Paper, "MERCHANT" and the other caps labels come out the
  same width to the pixel. Used for the tracked roles: section labels, column headers, the
  wordmark, the inspector title and display figure. Limits: one line, no wrapping,
  truncation or selection, and no accessibility node, so long text in a tracked role is
  clipped (the inspector title for a long merchant). Plain-text roles (tracking 0) stay
  ordinary text.
- **gpui-component theme:** mapped onto the tokens by `src/theme/component.rs` (D2.4);
  `coffer::theme::install` runs it at startup, then loads the fonts. It switches the
  stock theme to dark and assigns every one of its colour fields a token, plus radius
  (`radius-md`; `radius-lg` for dialogs and other floating surfaces), fonts, shadows (off:
  Coffer is flat) and motion (`motion::FAST`/`BASE`/`SLOW`, the enter and exit curves,
  critically damped springs). A test fills the palette with a sentinel, applies the map and
  fails if any field is unmapped or isn't a token, so a field added by a `gpui-component`
  upgrade fails the build until it's mapped. A headless `#[gpui_kit::test]` runs the
  startup path (`gpui_kit::init`, then `install`) and checks the global theme, its
  `tokens` (no stale stock gradient) and the Base layer's copy; the fonts can't be checked
  there, since the test platform's text system resolves every font to one id. The map
  follows meaning, not names: stock `primary` is the neutral primary button (`text` fill,
  `on-light` label); success and info are neutral too (Coffer has no success colour);
  `molten` is only selection, focus and
  drop targets; `moldavite` only the literal "green" hue. Nothing may call
  `Theme::change` or `Theme::sync_system_appearance` afterwards: either reloads the stock
  palette.
- **Rem base:** stock controls are sized in rems (medium buttons, inputs and selects are
  `h_8` = 2rem, text `text_sm` = 0.875rem), so the theme's base font, which is GPUI's rem,
  is `size::CONTROL / 2` = 14px. Stock medium controls are then exactly 28px. Their text is
  12.25px against `--text-sm` 12px; accepted for stock controls (D2.5 visual review). Coffer's
  own components take sizes from the tokens and don't depend on the rem. Zoom (changing
  the base font) would scale only stock components; out of scope until there's a zoom
  feature.
- **Not yet designed in Paper, mapped provisionally:** dialog/sheet backdrop (`chrome` at
  `opacity-muted`), scrollbar thumb (`border-strong`, hover `tooltip-border`), text caret
  (`text`). Warning has no hover/pressed states (Coffer has no warning button).
- **Focus ring:** Coffer's own components draw the spec ring with
  `FocusRingExt::focus_visible_ring` (`src/theme/focus.rs`): a `size::FOCUS_RING` (1.5px)
  `molten` line, drawn only while the element has keyboard focus (focused, and the last
  input was a key press, as GPUI's `focus_visible`). `Outside` puts it
  `size::FOCUS_RING_OFFSET` (2px) out, concentric with the element's corners (radius there
  = element radius + 2, so its outer edge is + 3.5); `Inset` puts it on the inside of the
  edge, following the corners, for rows in scrolling lists. "Inset ring" has no gap in the
  spec, so it's read as flush (CSS `ring-inset`); accepted in the D2.5 visual review. It's an absolutely
  positioned last child, so it takes no layout room, and an `Outside` ring needs 3.5px clear
  of any clipping ancestor. Unit tests check the geometry; headless tests check it appears
  only on keyboard focus, and its bounds.
  Stock controls run with `focus_ring = false`. The stock ring (`true`) is a fixed 3px band
  of `ring` at 50% alpha, flush outside a `ring`-tinted border: wider than the spec,
  translucent, no gap, and clipped by any `overflow_hidden` ancestor. Its width, gap and
  alpha are `gpui-component` constants, not theme values. With `false`, bordered controls
  (input, select, outline button, input group, combobox) turn their border `molten`;
  ghost buttons and other borderless controls draw a 1px `molten` line on their edge;
  link and text buttons draw it 0.125rem (1.75px) outside. That's solid `molten`, 0.5px
  thinner than the spec with less or no gap, and nothing clips it, so it's the closer of
  the two. Coffer's buttons (`src/ui/button.rs`, see "Stock components") close the stock
  button's gaps: they set the stock `.focus_ring(false)` and draw the spec ring from a
  tracked handle, on a wrapper with the button's radius, so it shows on keyboard focus only;
  headless tests check that, and that a click leaves no ring. Remaining gaps: stock buttons
  used directly or inside other stock components (dialog footers, dropdown buttons, button
  groups) still draw the stock style: filled ones (primary, secondary, danger, warning) a
  line inside their fill in the label colour at 60% instead of `molten`, on any focus, not
  only keyboard focus; stock rows have no focus ring of their own (the data table marks
  the whole table, the list marks nothing), so focusable rows need the `Inset` ring. Read
  from the `gpui-component` 0.9 source only; the GUI hasn't been run. Check every stock
  control's focus in the D2.5 showcase.
- **Stock components** (D2.4 audit of `gpui-component` 0.9). Stock styling must not leak:
  every component is either themed through the map above, wrapped with per-instance
  styling, or built in Coffer from `div` and the tokens. "On the showcase's stock
  board, unwrapped" means the colours map and the control is shown as-is beside its spec;
  wrap it to the spec before product UI uses it.

  | Component | Treatment | Why |
  |---|---|---|
  | Button: neutral primary, outline, outline compact, destructive outline, destructive solid, icon (28px; 36px dock) | Stock, wrapped: **built**, `coffer::ui::button` (`Button`, `IconButton`) | The stock button keeps pointer and Enter/Space activation, Tab order, disabled handling and the accessible name (the label; an icon button takes one). Height, padding, radius, border, fill and label colour are set per instance from the tokens. The label is a child with its type role, because the stock label's size follows the stock `Size` (12.25px at the rem base). Hover and pressed go through `ButtonVariant::Custom` with token colours, because the stock variants also set the border on hover and pressed (the outline's `border-strong` would turn `hairline`, the destructive outline's `danger` a 40% tint). Disabled is the resting look faded to `opacity-disabled` as one group: element opacity where nothing sits under the label, opaque composites over `Button::with_surface` where a fill does (see "Rendering findings (D2.6)" → "Closed"). Focus: see "Focus ring". Not built: the active icon button (`control-active`; white icon in the dock), tooltips (Coffer's tooltip is custom and not built yet), and hover/pressed transitions (`motion-fast`; the stock button changes instantly). Read, not specified, and accepted in the D2.5 visual review: icon colour `text-muted` (the token for default icons) and icon size `icon-md` (14px, from the table's 12–16px) |
  | Agent outline | Stock, wrapped: **built**, `ButtonVariant::AgentOutline` | As above: `moldavite-border-strong` border, `moldavite-text` label, hover `moldavite-surface-raised`, pressed `moldavite-well`. It confirms the `Custom` path: its resting fill (20% of its colour) is replaced per instance, and the hover and pressed fills still apply over that |
  | Agent primary, agent ghost | Stock, wrapped; not built | Same `Custom` path; the solid agent primary sets `.bg(moldavite)` per instance. Needs Paper first: the table gives no horizontal padding for either. Agent primary's optional trailing kbd needs the Kbd wrapper; agent ghost needs a leading icon (sparkle) |
  | Input, field, select, command bar | Stock, wrapped | In dark mode the stock fill is the *border* colour (`input`) at 30% alpha, so no theme value gives `field` fill with a `hairline` border; the wrapper sets `.bg(field)`. Height is 28px from the rem base |
  | Data table | Not used for Transactions (D5): a GPUI `list` with Coffer's rows, which interleaves day headers of their own height | Colours map (no stripes, no row dividers, `hover`/`molten-soft` rows, `canvas` sticky header) and row height takes `Size::Size(size::CONTROL)`. These colours apply if a stock data table is used elsewhere. The in-place category select isn't built yet |
  | Popover, menu, dialog, sheet, notification | Stock; on the showcase's stock board, unwrapped | Fill and text map (`float`, `text`). Popovers use a hard-coded shadcn ring and shadow at `radius`; Coffer's floating spec is a 1px `border`, `radius-lg` and the toolbar shadow, set per instance |
  | Title bar | Stock; on the showcase's stock board, unwrapped | Keeps platform window controls and dragging. Height is a constant 34px; Coffer's is 40px |
  | List row | Stock, wrapped: **built**, `coffer::ui::list_row` (`ListRow`) | Wraps the stock `ListItem` (pointer, accessible row). 28px, radius 6, 8px inline padding, 16px icon slot, `text-secondary` label lifting to `text` on hover, `molten-soft` fill with white label when selected, trailing mono count. The owning list holds focus and passes `focus_visible`; the row draws the `Inset` ring on a wrapper, because `ListItem` lays children out in an inner box and scrolling lists clip an outside ring. Nesting is a 20px left indent on the wrapper. No pressed look (rows select on click) |
  | Transaction row, table header, day header | **Custom: built**, `coffer::ui::transaction_row` | Columns from `column-category`/`column-account`/`column-amount`; amounts in `mono-amount`, sign never carried by colour. Category cell: assigned (marker), uncategorised (dashed marker), suggested (sparkle, `moldavite-text`), transfer (row dimmed, transfer glyph). Selection and focus as the list row. D5 kept these rows, in a virtualised GPUI `list` (`src/transactions.rs`) |
  | Status bar, tree row | Stock; not yet compared | Colours map (`float` status bar with `hairline`; `hover` and `molten-soft` rows) |
  | Scrollbar | Stock | Colours provisional (above) |
  | Kbd | Stock, wrapped | Fill and font differ (`hover`, Geist Mono 11/14, 18px); set per instance |
  | Toggle | **Custom** | Stock `Switch` (small) has the right geometry (28×16, 12px knob, 2px inset, pill), but one knob colour for both states; Coffer's knob is `text-muted` off and `chrome` on |
  | Segmented control | **Custom** | Stock segmented tabs are 24 or 32px (segments 18/24); Coffer needs 30px panel / 28px inspector with 2px inset, and an inset highlight on the active segment |
  | View tabs (title bar) | **Custom** | Stock tab heights are 20/24/32/36px; Coffer's tab is 28px with a `hover` active fill and hairline separators |
  | Underline tabs | Stock; on the showcase's stock board, unwrapped | Stock underline tab heights are 26/30/36/44px; the spec gives no height |
  | Badge | **Custom: built**, `coffer::ui::badge` | Stock `Badge` is a dot or count overlaid on another element; Coffer's badge is an inline 16px count pill |
  | Tooltip | **Custom** | Stock tooltip uses the popover fill (`float`) and `text_sm`; Coffer's is `hover` fill, `tooltip-border`, tooltip shadow, title + kbd + body. Rendered through GPUI's `.tooltip(...)` |
  | Progress bar | **Custom** | Stock heights are 4/6/8/10px; Coffer's is 3px, radius 2 |
  | Agent card | **Custom: built**, `coffer::ui::agent_card` | Change rows are 26px tab stops with a `moldavite-well` glyph well and the detail in `moldavite-text-muted`; on hover or keyboard focus the row fills `moldavite-surface-raised` and swaps its age for approve/reject icon buttons, so both are reachable by keyboard. "Approve all" is a full-width agent outline (`Button::with_full_width`) |
  | Chip | **Custom: built**, `coffer::ui::button::Chip` | Pill with optional mono count |
  | Card, status dot, empty state, bulk-action bar, section label row, inspector field row | **Custom** | No stock component with the same structure; built from `div` and the tokens. Not built yet |
  | Charts | **Custom** (decided in D4.1), on the `gpui_component::plot` primitives | Stock `BarChart` plots one value per band, so grouped spending/income bars would need two charts stacked; `LineChart` hard-codes a 2px stroke (spec 1.5px) and has no halo option; neither has a selected-band state for the molten selection overlay. Reuse the plot scales and tooltip; draw bars, the haloed line and the selection with the tokens |
- **Showcase** (`coffer --showcase`, `src/showcase.rs`): three boards. *Design tokens*
  (every colour token once, checked by a test against `design/tokens.json`; the type
  scale), *Components* (each Coffer component as a static state matrix next to a live,
  interactive row; the matrix forces states with `ui::PreviewState`, which is for the
  showcase only), and *Stock components* (input, select, popover, menu, dialog, title bar,
  underline tabs and a stock-focus row, unwrapped, each captioned with its spec). The
  stock board shows what still needs a wrapper; it isn't product styling. Compared by eye
  against the Paper artboards (`D4-0`, `VI-0`, `XJ-0`, `11R-0`, `9-0`, `6S-0`, `14I-0`) in
  D2.5, with no discrepancies found.
- **Fidelity check:** compare implemented screens against Paper screenshots
  (`get_screenshot`) at 1x and 2x before marking UI work done.
