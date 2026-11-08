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
| Coffer artboards | Foundations (`1-0`), 1920 wide: colour, type scale, spacing, radii (D1.3). Components (`CJ-0`), 1920 wide: every component below in its states (D1.4) |
| Coffer tokens | 122: 96 created in D1.2, 24 size tokens and 2 danger-state colours added in D1.4; content hash `572f3e11` (see "Tokens") |
| Reference file | **Festive comet** — <https://app.paper.design/file/01M6GNHMB1RVVHY51G5W61H75N/p-1-0>. Not Coffer's file: a design-editor app this system was extracted from. Read it for detail; never edit it |
| Reference page | `Editor — v1` (`p-1-0`) |
| Reference artboards | Editor (`1-0`), Empty file (`JK-0`), 1920×1080 |
| Token snapshot | [`design/tokens.json`](../design/tokens.json), [`design/tokens.css`](../design/tokens.css) — exported from the **Coffer** file in D1.5, content hash `572f3e11`, extracted 2026-11-08. `tokens.json` keeps each token's Paper description |

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
   Values marked **Proposed** (the finance screens and the chart designs) are decided and
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

The Coffer file holds 122 tokens (content hash `572f3e11`), exported to
[`design/tokens.json`](../design/tokens.json) and [`design/tokens.css`](../design/tokens.css)
and checked against the theme module by its drift test. Every node on the Foundations and
Components artboards is bound to them. Motion durations, shadows and border widths have
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
| Dock | `0 10px 30px #00000080` | Bottom tool dock |
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
in Paper (Foundations artboard); the chart designs ("Income vs spending" and "Chart
chrome" below) are **Proposed** until the Overview screen is drawn (D4).

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

**Income vs spending (monthly overview).** Spending bars `text-secondary`, income bars
`icon-faint`, net savings a 1.5px `text` line. Hovered month: tooltip. Selected month:
1.5px molten outline around the bar pair plus a molten mono label (selection language).

**Chart chrome.**

| Element | Spec |
|---|---|
| Axis labels | Geist Mono 10/12, `text-muted` (not `text-faint`: it fails contrast) |
| Gridlines | Horizontal only, 1px `hairline`; no vertical gridlines, no chart border |
| Baseline / zero line | 1px `border-strong` |
| Bars | Radius 2 on the value end; group gap 40% of bar width |
| Lines | 1.5px, round joins; points hidden until hover (6px dot, 2px `canvas` ring) |
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
| AI-suggested category | 10px sparkle + label in `moldavite-text`; approve/reject from the inspector or the inbox |
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
| Disabled | 40% opacity (`--opacity-disabled`), no hover or pressed, default cursor |
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
- Respect the OS reduced-motion setting: everything goes to 0ms except opacity fades, which
  stay at 90ms or less.

## Proposed: finance screens

Window minimum 1280×800, designed at 1440×900. Shell: title bar with view tabs
(Overview, Transactions, Trends, Budgets, Net worth, Inbox, Reviews; Inbox carries a
moldavite count badge when suggestions are pending) and the ⌘K command bar; left panel
272px; inspector 296px (collapsible); status bar 28px.

**Left panel.** ACCOUNTS section: one row per account with its mono balance right-aligned;
pots nested 20px under their parent with a pot icon. Net worth total at the top as a
mono 13/16 600 figure. CATEGORIES section below, used as a filter.

**Overview.** Month switcher (‹ Oct 2026 ›) top-left. Three figures in a row: Spent
(display figure), Income and Net (figure), each with a caption delta vs last month
("+8% vs Sep", `text-muted`). Then the 12-month income vs spending chart (240px high).
Below that, two columns: category breakdown as a ranked bar list (left), and recurring
payments plus, when suggestions are pending, the agent card "Claude · 12 suggestions" with
"Review" (right).

**Transactions.** Toolbar: search field (240px), filter chips (Account, Category, Date,
"Uncategorised 14"), and "Import ⌘I" outline button on the right. Then the data table.
The inspector shows the selected transaction: merchant as title, amount as display figure,
field rows (date, account, category select, note), raw description, "Create rule from this"
and transfer pairing.

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
   Paper node IDs.
5. Export `design/tokens.json` and `design/tokens.css` from the Coffer file. ✅ Done (D1.5):
   122 tokens, content hash `572f3e11`. Re-export whenever a token changes (rule 4).

## Implementing in GPUI

- **Fonts:** bundle Inter and Geist Mono with the app (both SIL OFL 1.1, compatible with
  `MIT OR Apache-2.0` distribution; include their licence files). Fraunces is not used.
  Implemented: the files (Inter 4.1, Geist 1.9.2) and their `OFL.txt` are in
  `assets/fonts/`; `src/theme/fonts.rs` compiles them into the binary, registers them at
  startup, and sets them as `gpui-component`'s UI and mono families. Startup fails if a
  face doesn't resolve to itself, so a missing weight can't silently fall back.
- **Weights:** 400/500/600/700, so the app bundles the static Inter instances for those
  four weights (and Geist Mono 400/600) and doesn't depend on variable-font rendering.
- **Motion:** check that GPUI's animation API can express the durations and easing curves
  above, during the fidelity spike.
- **Theme module:** one Rust module in `crates/app` maps every token above to a constant
  or theme field, generated from or checked against `design/tokens.json`. Components
  never contain raw colour or size literals.
  Implemented in `src/theme/tokens.rs` (moves to `crates/app` in M1). Its drift test maps
  every token in `design/tokens.json` to a constant (following `var()` aliases such as
  `--color-on-light`), so the build fails when the export adds, removes or changes a token
  until the theme module follows. A second test checks the snapshot came from the Coffer
  file.
- **Letter spacing:** GPUI's text style has no letter-spacing property, so the tracking
  tokens (−0.01em, +0.06em caps, −0.025em display) are stored but can't be applied yet.
  Decide in the fidelity spike whether that's acceptable or needs a workaround.
- **gpui-component:** override its theme with the tokens in this file; replace stock components
  where their structure doesn't match the specs above (e.g. toggle, segmented control,
  agent card). Stock styling must not leak into the UI.
- **Fidelity check:** compare implemented screens against Paper screenshots
  (`get_screenshot`) at 1x and 2x before marking UI work done.
