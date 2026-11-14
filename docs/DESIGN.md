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
| Coffer artboards | Foundations (`1-0`), 1920 wide: colour, type scale, spacing, radii (D1.3). Components (`CJ-0`), 1920 wide: every component below in its states (D1.4). Transactions (`18E-0`) and Transactions · multi-select (`1NP-0`), 1440×900: the Transactions screen (D3.1). Overview (`22V-0`), 1440×900: the Overview screen (D4.1). D6.3, Transactions additions: Transactions · split and tags (`2Q3-0`), Transactions · excluded (`3FE-0`), Transactions · rule from an edit (`3WT-0`), Transactions · manual (cash account) (`4D7-0`), 1440×900, and Transactions · D6.3 states (`4X9-0`), 1440 wide. D6.1, remaining screens: Inbox (`564-0`), Import · map columns (`5ZQ-0`), Budgets (`6OE-0`), Trends (`781-0`), Net worth (`7LH-0`), Reviews (`7YM-0`), 1440×900; Import · steps (`6J4-0`) and Accounts panel · states (`8D8-0`, with M3.2's Add balance dialog `I7C-0`), 1440 wide. D6.2: Logo (`8MT-0`), 1440 wide: the mark, its construction, lockups and app icon; Logo · concepts (`8JS-0`): the three directions explored (record only). D6.4, review queues: Review · possible duplicates (`8SE-0`) and Review · balance checks (`98K-0`), 1440×900; Review queues · states (`9MQ-0`), 1440 wide. D6.5, refunds and shared costs: Transactions · refund link (`9WS-0`), Transactions · shared cost (`BAT-0`) and Review · owed to you (`CCF-0`), 1440×900; Links and shares · states (`EB0-0`), 1440 wide. D6.11, safe report rendering: Reviews · safe rendering (`C6D-0`), 1440×900; Reviews · D6.11 states (`E43-0`), 1440 wide. D6.9, accounts you keep yourself: Accounts · asset (valuations) (`BSI-0`), 1440×900; Accounts · D6.9 states (`EPF-0`), 1440 wide. D6.6, Inbox impact preview: Inbox · rule impact (`ABK-0`) and Inbox · merchant name impact (`DL8-0`), 1440×900; Inbox · D6.6 states (`EWZ-0`), 1440 wide. D6.8, Settings: Settings · AI clients (`AQ2-0`), Settings · access log (`DC6-0`) and Settings · backups (`F63-0`), 1440×900; Settings · D6.8 states (`GQA-0`), 1440 wide. D6.7, Rules: Rules (`B1J-0`), Rules · apply to past transactions (`EFT-0`), Rules · merchant names (`G2O-0`) and Rules · change history (`GZT-0`), 1440×900; Rules · states (`HFH-0`), 1440 wide. D6.10, budgets and upcoming: Budgets · groups, rollover and pace (`CXU-0`) and Budgets · upcoming (`GD5-0`), 1440×900; Budgets and upcoming · states (`HNB-0`), 1440 wide |
| Coffer tokens | 126: 96 created in D1.2, 24 size tokens and 2 danger-state colours added in D1.4, `--container-search-field` added in D3.1, `--container-dialog` added in D6.3, `--container-dialog-wide` and `--container-reading` added in D6.1; content hash `6a126241` (see "Tokens") |
| Reference file | **Festive comet** — <https://app.paper.design/file/01M6GNHMB1RVVHY51G5W61H75N/p-1-0>. Not Coffer's file: a design-editor app this system was extracted from. Read it for detail; never edit it |
| Reference page | `Editor — v1` (`p-1-0`) |
| Reference artboards | Editor (`1-0`), Empty file (`JK-0`), 1920×1080 |
| Token snapshot | [`design/tokens.json`](../design/tokens.json), [`design/tokens.css`](../design/tokens.css) — exported from the **Coffer** file in D1.5 and re-exported in D3.1, D6.3 and D6.1, content hash `6a126241`, extracted 2026-11-10. `tokens.json` keeps each token's Paper description |

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

The editor's molten gem is that product's mark, not ours. Coffer's own mark, the
strongbox, sits before the wordmark in the title bar (see "Logo").

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
   the Coffer file into `design/`, and update the tables below and `crates/app/src/theme/tokens.rs` in
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

The Coffer file holds 126 tokens (content hash `6a126241`), exported to
[`design/tokens.json`](../design/tokens.json) and [`design/tokens.css`](../design/tokens.css)
and checked against the theme module by its drift test. Every node on the Foundations,
Components, Transactions (D3, D6), Overview and D6.1 artboards is bound to them. Motion durations, shadows and border widths have
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
| `--container-dialog-wide` | `760px` | Wide dialogs that pair a form with a preview: Import (added in D6.1) | `size::DIALOG_WIDE` |
| `--container-reading` | `560px` | Measure of reading text: a saved review, about 75 characters of 13px body (added in D6.1) | `size::READING_MEASURE` |

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
Brand group (the 16px mark and the "Coffer" wordmark, see "Logo") and the view tabs at
left, command bar centred, window controls right.

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
is reserved for the AI agent. The other filled icon is Coffer's mark (see "Logo").

## Logo

Designed in D6.2 on the **Logo** artboard (`8MT-0`); the three directions explored
(strongbox, coin-stack C, keyhole tile) stay on **Logo · concepts** (`8JS-0`) as a record.

**Mark: the strongbox.** A coffer seen front-on: a lid, a 1-unit seam where the lid lifts
off, a body, and a hasp dropping from the seam. Filled, no strokes. Drawn on a 16-unit
grid so the title-bar size (16px) lands on whole pixels:

| Part | Geometry (16-unit grid) |
|---|---|
| Lid | 14 × 4, top corners r1.5 |
| Seam | 1 unit, full width |
| Body | 14 × 7, bottom corners r1.5 |
| Hasp | notch 2 wide, 3 deep from the seam, round end r1 |
| Bounds | 14 × 12, inset 1 unit at the sides and 2 at the top and bottom |

Path data (`viewBox="0 0 16 16"`):
`M2.5 2H13.5A1.5 1.5 0 0 1 15 3.5V6H1V3.5A1.5 1.5 0 0 1 2.5 2Z` (lid),
`M1 7H7V9A1 1 0 0 0 9 9V7H15V12.5A1.5 1.5 0 0 1 13.5 14H2.5A1.5 1.5 0 0 1 1 12.5Z` (body).

**Colour.** `molten`: the mark is the one place molten means *brand* rather than
selection or focus. `text` where a surface allows one colour only. Never `moldavite`
(green belongs to the agent). On a molten surface only as the app icon, where it turns
`chrome`.

**Lockups**

| Use | Mark | Gap | Wordmark |
|---|---|---|---|
| Title bar | 16px (`icon-lg`) | 6px (`spacing-1_5`) | "Coffer", Inter 13/16 600, `text`, −0.01em (`tracking-tight`) |
| Display (README, about) | 32px | 12px (`spacing-3`) | "Coffer", Inter 32/38 600, `text`, −0.025em (`tracking-display`) |

At display size the mark's 24px ink matches the wordmark's cap height. The mark alone is
for icons; the wordmark never appears without it. Clear space: one lid height (4 units)
on every side.

**App icon.** A `molten` tile, radius 28 on 128, with the mark in `chrome` at 4.5× (72px),
centred. Checked at 128, 64 and 32.

**Files.** `assets/brand/coffer-mark.svg` (the mark, 16px, molten) and
`assets/brand/coffer-icon.svg` (the app icon, 128px) are exports for use outside the app,
so their colours are literal hex. In the app the mark is `Glyph::Mark` (`crates/app/src/ui/icon.rs`),
tinted by `text_color`. A test checks that both files draw the same paths as the glyph.

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
  "Negative balance" means an overdrawn account: a current, savings, cash or pot account
  below zero. A liability is negative by nature and keeps the normal colour (decided in
  D6.9, for review). Credit cards follow the same reasoning, but no credit-card balance
  is drawn yet.
- Thousands separators and decimal marks follow the user's locale; the currency's minor
  units decide the decimals (JPY shows none).

**Shell mapping (suggested, to be confirmed in Paper)**

| Reference design | Coffer |
|---|---|
| Title bar tabs | Views (Overview, Transactions, Trends, Budgets, Net worth, Inbox, Reviews, Rules) |
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

## Finance screens

Window minimum 1280×800, designed at 1440×900. Shell: title bar with view tabs
(Overview, Transactions, Trends, Budgets, Net worth, Inbox, Reviews, Rules; Inbox carries a
moldavite count badge when suggestions are pending) and the ⌘K command bar; left panel
272px; inspector 296px (collapsible); status bar 28px.

**Left panel.** Drawn in D6.1: see "Accounts panel" below.

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
  centred. It follows the tabs, and the window controls stay right. D6.7 adds an eighth
  tab, Rules (about 60px more); the command bar moves right with it and still fits
  beside D6.8's Settings button. The artboards drawn before D6.7 still show seven tabs.

- **Built in M3.3** (`crates/app/src/transactions.rs`; `ARCHITECTURE.md` → "Decision
  record: the Transactions view"): the table reads the database and is filtered by the
  accounts panel; search, the Account, Category and Date chips, the Uncategorised chip
  and sorting work; categories are set in place. The inspector's Date and Account are
  read-only with the lock (D6.3's Details). AI suggestions, flagged rows, Note and
  "Create rule from this" wait for M3.6, a per-row problem record, M3.9 and M3.13.
  **Composed from existing specs, not drawn** (user, 2026-11-14; review):
  - *Category select*: the Kind menu (`FHZ-0`) with the category's 8×8 marker centred
    in the 16px icon slot; each top-level group under its caps label, its own category
    first, then its subcategories; a filter field at the top (the toolbar's search
    field at the menu's width, placeholder "Find a category") narrows the options as
    you type, a group's label matching keeps the whole group, and "No matches" (a 28px
    row in body `text-muted`) says when nothing is left; scrolls after 8 rows. In
    place, it opens 4px below the row, left-aligned with the category marker; from
    the bulk-action bar, 4px above it.
  - *Filter chip menus*: the same menu below the chip. Account: "All accounts", then
    the panel's accounts (each pot after its parent). Category: "All categories", then
    the category menu. Date: This month, Last month, Last 3 months, This year, All
    time (the default). A chip whose filter is set, and the Uncategorised chip while
    on, take the Excluded chip's on look (`3TU-0`: `hover` fill, `text` label) and
    name the choice ("Joint account", "Last month").
  - *Sorting*: the chevron sits on the column's inner side so no title moves: before
    the right-aligned Amount (as `XL-0`), after the others. It points down for Z–A or
    largest first, up (the same glyph turned) otherwise; on hover a sortable title
    shows it in `icon-faint` before any sort.
  - *No match*: the Empty state centred in the table: "No transactions match",
    "Nothing matches the search and filters." and "Esc Clear filters". A database that
    can't be read says so in the same place.
  - *Split marker*: the two halves only when both largest lines have one of the six
    colours; otherwise the largest line's marker.

  The undo bar (`53O-0`) is built as drawn (`ui::undo_bar`), after a category change to
  several rows; its ✕ is the 24px icon button with the app's 10px Close glyph, where
  Paper draws a 12px one.

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

**Remaining screens** — drawn in D6.1 (no longer Proposed). Six 1440×900 artboards and two
states boards, built from the existing shells (Transactions with an inspector, Overview
without) and Components clones. Every colour, size, gap, radius and type value is a bound
token. Typed values as on D3/D4: 1px borders, the Dock shadow on dialogs, SVG icon paths,
data-driven bar and point positions, the 50% radius of a chart point. Two new tokens,
`--container-dialog-wide` and `--container-reading`. Fixture data continues the
Transactions and Overview fixtures (November 2026, the same accounts, categories and
merchants).

**Accounts panel** (states board `8D8-0`). The left panel is the same on every view that
filters by account (Overview, Transactions, Trends, Budgets, Net worth); Inbox and Reviews
replace it with their own lists.
- Net worth figure at the top (mono 13/16 600), then ACCOUNTS (section label row with a `+`
  that opens "New account"), one list row per account with its mono balance; pots nested
  20px under their parent with the pot icon. CATEGORIES below, used as a filter.
- Row states: selected (`molten-soft`, white label), hover, **flagged** (a 6px `danger`
  dot before the balance: a balance assertion failed; the tooltip names the window), **no
  balance** (the value reads "No balance" in UI type, `text-muted`; the account is left out
  of net worth until a balance is entered). A negative balance is `danger` (see "Money").
- **Account inspector** (an account selected, no transaction selected). Header: name,
  balance as display figure, caption "Balance on 9 Nov · pots not included". Details: Name,
  Bank (fields), Kind (select), Currency (read-only with a 10px lock once the account has
  transactions), Preset (select, used by the next import). Balances: the anchors the user
  entered, each "date · Entered by you · amount", outline "Add balance…", and a caption
  that the balances are checked against the transactions. Pots: each pot with its
  balance, total in the title row. Imports: file name and "count · date" per batch. Footer:
  outline "Close account…" (hides the account and keeps its history; there's no delete for
  an account with transactions).
- **New account dialog** (`--container-dialog`): Name, Bank, Kind (select), Currency
  (select), Balance (`£` prefix, optional, with its date beside it in a 96px field). Caption:
  a credit card owes money as a negative balance; skip the balance when the export has
  one; pots come from imports. Cancel / "Add account" (neutral primary).
- Cash, asset and liability accounts: drawn in D6.9, see "Accounts you keep yourself".
- **Add balance dialog** (`I7C-0`, under New account on the states board; drawn for M3.2
  and approved by the user, 2026-11-14), from "Add balance…" on a bank or cash account.
  D6.9's "Add a balance for Mortgage" with the bank's wording: title "Add a balance for
  {account}"; Balance (the currency's symbol in mono `text-faint`, the amount in mono,
  signed: typed with a minus when overdrawn) with its date in a 96px field; Note; caption
  "Your balance at the end of that day, as your bank shows it. Type a minus if the
  account is overdrawn. Coffer checks it against the transactions since the last
  balance."; Cancel / "Add balance" (neutral primary). No new tokens.
- **Built in M3.2** (`crates/app/src/accounts/`): the panel from the database, the
  account inspector on Transactions, New account and Add balance. "Close account…" waits
  for M3.17 (user), so the inspector has no footer yet. The Preset select shows the
  preset's own name ("Monzo"), where the artboard wrote "Monzo (current account)".
  **Composed from existing specs, not drawn** (user, 2026-11-14; review):
  - *Empty panel*: with no accounts, the Accounts section shows one 28px row "No
    accounts yet" in body type `text-muted`, and net worth reads "—". A database that
    can't be read puts the reason there in caption type, `danger`.
  - *Several currencies*: one net worth line per currency, nothing converted (M4.6).
  - *Assets and liabilities* made with the CLI are listed under ASSETS AND LIABILITIES
    (D6.9), without that section's `+` until M3.17.
  - *Menus*: the Currency and Preset menus are the Kind menu (`FHZ-0`): Currency has two
    groups, "Common" (GBP, EUR, USD) and "Every currency" (by code), and scrolls after 8
    rows (`control` × 8); Preset lists "No preset · detect it at import" and the
    presets. The option the arrow keys reach takes the `hover` fill, like the chosen one.
    An option that can't be chosen is faded to `opacity-disabled` with the reason as its
    hint ("Asset · has transactions"); in New account, Asset and Liability read "added in
    a later version" until M3.17.
  - *Read-only kind*: a pot's Kind is shown like the locked Currency ("Pot" and the
    lock); a pot has no Bank or Preset, and nor does a cash account (D6.9's cash
    inspector). A pot's header caption reads "Balance on 9 Nov · in Current account".
  - *No balance yet*: the header has no figure, and its caption reads "No balance yet:
    add one below, or import a file that has balances."
  - *Balances caption*: the artboard's Monzo sentence when the preset is Monzo, otherwise
    "Coffer counts forward from the balances you enter and the ones in your imports.
    Each one is checked against the transactions." A row shows the balance's note when
    it has one, else where it came from ("Entered by you", "Statement", "Opening
    balance"). Dates from another year use D6.9's 56px lane ("2 Oct 25").
  - *Errors*: a field that can't be read, or a change core refuses, is said under the
    form in caption type, `danger` (a sentence: "An account needs a name."), and nothing
    is saved; in the inspector the field gets its stored value back.
  - *Dates* are typed into the mono date field ("9 Nov 2026"; also "9 Nov", ISO and
    day-first numbers): no date picker is drawn.
  - *Keyboard*: the panel is a Tab stop; the arrow keys move the selected account in
    panel order and Escape goes back to all accounts, with the inset focus ring on the
    selected row.

**Inbox** (`564-0`). Transactions shell with the Inbox tab active and the left panel
replaced by filters:
- Left panel: INBOX (Pending 12 selected, Approved 148, Rejected 23, with check / cross
  icons), KIND (Categorisations 9, Rules 2, Transfer pairs 1), SOURCE (Claude Code 12 with a
  moldavite connected dot, Codex 0, "Coffer from your edits" 0: the app's own `rule`
  suggestions). D6.4 adds a CHECKS section between INBOX and KIND: see "Review queues".
- Toolbar (52px, bottom hairline): "Pending" in title type, caption "12 suggestions ·
  nothing changes until you approve", and kbd hints on the right: A Approve, R Reject, ↑↓
  Move.
- **Groups are table sections, not agent cards** (changed from the proposal). With a dozen
  rows a moldavite card per group was heavy, and the table language lines up with
  Transactions. Each group: a 28px header with the group name (11/14 600
  `text-secondary`, like a day header) and mono count, plus the lane labels for that kind;
  28px rows; a footer with "Approve all (9)" (agent primary), "Reject all" (agent outline),
  and the source right-aligned ("✦ Claude Code · 2h ago", the client name in
  `moldavite-text`). A group of one says "Approve" / "Reject". Groups are divided by a
  hairline.
- Lanes (row content 840px): item (flex) · NOW (140, `column-account`) · SUGGESTED (180,
  `column-category`) · DATE (48, mono 11/14 right) · AMOUNT (96, mono 12/16 right) · 52px
  action lane (two 24px icon buttons, 4px apart). Suggested is the table's AI-suggested
  style (10px sparkle, `moldavite-text`). Now shows the current category (marker +
  label; dashed "Uncategorised"); a category set by a rule is shown plainly and the
  inspector says which rule. Rules use NOW for the rule that decides today ("No rule",
  "Rule UBER") and MATCHES (mono count) in the amount lane. Transfer pairs show "Savings →
  Current account", "Income and spending" under NOW, and the unsigned amount (a pair has
  no direction).
- Approve ✓ (`moldavite-text`) and reject ✕ (`text-secondary`) appear on hover and on the
  selected row only; at rest the lane is empty, so twelve rows don't repeat 24 icons.
- Inspector (a categorisation selected, UBER *EATS, which a rule filed as Transport): the
  transaction header; the agent suggestion card ("Change Transport to Eating out", the
  agent's reason, Approve / Reject); Details read-only (Category, Set by "Rule Merchant
  contains UBER", Account, Date) with a caption that approving changes only this
  transaction; Raw description; "One transaction" + outline "Show in Transactions". D6.6
  replaces the caption with an "If you approve" section: see "Inbox impact preview".
- **Built in M3.6/M3.14** (`crates/app/src/inbox/`; `ARCHITECTURE.md` → "Decision
  record: Inbox"): the panel, groups, open preview and inspector of `564-0`, `ABK-0`,
  `DL8-0` and `EWZ-0`, from the database. Groups are per kind *and* source, so each
  footer names one source. Left out until their items (user, 2026-11-14): CHECKS (M3.12),
  "Show N in Transactions" (M3.3), "Edit in Rules…" (M3.7). Client names are the
  `--client` names until display names (M3.15), so the card header reads
  "claude-code · 2h ago". **Composed from existing specs, not drawn** (user, 2026-11-14):
  - *After approving*: D6.3's undo bar with no Undo button, and the D6.6 captions drop
    "One change you can undo" and "open it in Rules"; no approval can be undone yet.
  - *Approved / Rejected*: the same groups, read-only: no NOW values, footers or action
    buttons; the action lane shows the decision date (mono 11/14 `text-muted`). The
    inspector shows a Decision section (date, the user's reason) instead of the buttons.
  - *Several items in one categorisation*: one row, "N transactions" in the item lane,
    "Several" where the items differ; selected, it opens like a rule with each
    transaction (BANK TEXT · NOW · AFTER · DATE · AMOUNT).
  - *Empty*: the Empty state component (`13L-0`) centred on the canvas ("Nothing to
    review", "No approved suggestions", "Nothing matches these filters"); nothing
    selected: one caption in the inspector.
  - *Coffer's own rows*: the suggested value in `text-secondary` with the sparkle's slot
    empty, and ✓ in `text`, so no moldavite shows.

**Import** (`5ZQ-0` map step over Transactions; states board `6J4-0`). A dialog
(`--container-dialog-wide`, 760px) over the window, backdrop `chrome` at `opacity-muted`
(still provisional, see "Implementing in GPUI"). Header: title, caption (format,
encoding, row count), the panel segmented control (File / Map / Review, 272px) and a 28px
close button. Footer: a caption on the left, actions right-aligned, above a hairline.
- **File**: steps not yet reachable are at `opacity-disabled`. A dashed drop zone
  (`border-strong`, `radius-surface`, 168px: `spacing-6` × 7) with a download icon well,
  "Drop a statement here" and outline "Choose file…"; RECENT IMPORTS below (file, account,
  preset, mono date). Caption: importing the same file twice duplicates nothing.
- **Map**: left column `container-panel` wide: Preset and Account selects, a caption
  ("Detected from the header row…"), then COLUMNS: Date, Format (mono `DD/MM/YYYY`), Time,
  Amount ("Amount · signed"), Merchant, Details, ID, Balance ("None in this file", muted).
  Right column, 24px away: "Preview · First 10 of 312 rows", a table (DATE 48, MERCHANT,
  AMOUNT 96), pot transfers muted with the transfer icon. A row that can't be parsed gets
  `danger-soft` fill, the 6px `danger` dot and its raw value in `danger`; a note under the
  table explains it ("Row 8: “8,99” isn't an amount with a decimal point. It will be
  imported as flagged, not skipped.") and says pot transfers pair automatically. Footer:
  "Nothing is saved until you import. A backup is taken first." Back / "Review 312 rows".
- **Review**: figures (New 295, Already imported 16 in `text-muted`, Flagged 1 with a
  `danger` dot; mono 20/24), AFTER IMPORT rows (categorised by rules, from Monzo's
  categories, left uncategorised, pot transfers paired, possible duplicates with a
  `warning` dot), then the reconciliation block, then Back / "Import 296 transactions".
  Flagged rows are imported (and flagged), so they're in the count.
- **Reconciliation block**, a 12px-padded box (`radius-surface`): *not reconciled*
  (hairline border, info icon, `text-strong` title: the export has no balances; neutral,
  because that's expected for Monzo), *reconciled* (hairline, check icon in `text`, the
  closing balance it matched), *mismatch* (`danger-soft` fill, `danger` icon and title,
  both balances, the gap and the window since the last passing balance; the primary
  button becomes "Import and flag the mismatch"), *locked period* (`warning-soft`, lock
  icon, the rows dated on or before `reconciled_through` stay out unless the user unlocks,
  outline "Unlock…").

**Budgets** (`6OE-0`). Transactions shell (left panel, inspector) with the Overview canvas
padding (16px top, 24px inline).
- Month switcher ("November 2026"), then figures: Left to spend (display figure) with
  "£16.02 a day for 21 days", Spent with "78% of budget, 30% of the month", Budgeted with
  the category count.
- A table, not a ranked list (the bars need the budget's end to line up): CATEGORY (180,
  marker + name) · bar (flex) · SPENT · BUDGET (`text-muted`) · LEFT (96 each, mono). The
  bar is the component's budget bar: 3px, `border` track, fill `text-secondary`, `warning`
  at 85% or more, `danger` over 100% with the overage after a 2px gap. LEFT takes the same
  colour (`warning` / `danger`, negative signed). A Total row (600) under a hairline.
- "Spending without a budget": Uncategorised with its amount and outline "Categorise 6".
- Inspector for the selected budget (Eating out): name, spent as display figure, "of
  £300.00 · 89% used with 21 days to go"; the Budget field (`£` prefix, "/ month" unit) with
  a caption; "Last 6 months" as a small bar list scaled to the largest month, with a 1px
  dashed `text-muted` budget line through it and the part of a bar over budget in
  `danger`; "Largest in November" (date, merchant, amount) and outline "Show all in
  Transactions". Rollover, groups, the pace line and Upcoming: drawn in D6.10, see
  "Budgets: groups, rollover, pace and upcoming".

**Trends** (`781-0`). Overview shell.
- Picker row: a category select (`container-search-field`, marker + name), "or", and a
  "Search merchants" field. Picking a category also selects its row in the left panel.
- Figures: Average a month (display figure), Last 12 months with the change on the year
  before, the selected month with its change and "highest this year".
- "{Category} by month": the Overview chart with one bar per month (`text-secondary`,
  monochrome: one series), y axis to the next round step, an average line (1px dashed
  `text-muted`, labelled "Avg £312.67" in mono micro on a `canvas` patch at its left end),
  legend "Spending" / "--- Average", the selected month with the selection overlay and
  molten label.
- Lower row: "Top merchants · {category} · last 12 months" as a ranked bar list without
  markers (bars `text-secondary`; "64 other merchants" last, drawn as Other) and
  "Recurring · {category}" with the monthly total; a price rise is flagged as on Overview.

**Net worth** (`7LH-0`). Overview shell.
- Figures: Net worth (display figure, "On 31 Oct, across 6 accounts"), Change (12 months,
  signed), the latest month's change.
- Chart: month-end net worth as a 1.5px `text` line with an `opacity-area-fill` area to
  the £0 baseline (the area must start at zero), the y axis £0 to the next round step, the
  last point selected (6px dot, 2px `canvas` ring, molten label). The range is an
  inspector-size segmented control in the chart header: 12 months, 24 months, All.
- Accounts table: ACCOUNT (icon, pots indented 20px) · BALANCE FROM (180: "Your balance on 1
  Sep", "Pot transfers", "Statement (OFX)", "Your valuation on 31 Oct") · CHANGE (signed,
  `text-secondary`) · BALANCE (96). Changes sum to the figure.
- An account without balances gets a neutral card under the table (`float`, hairline,
  `radius-surface`, icon well): "Amex isn't in your net worth yet", why, and outline
  "Enter balance…". Its left-panel row reads "No balance".

**Reviews** (`7YM-0`). Transactions shell; the left panel lists the reviews.
- Left panel: SAVED REVIEWS with a count, one row per review (10px moldavite sparkle,
  title, mono save date); the selected one `molten-soft`. A caption under the list: reviews
  are written by the AI client with `monthly_review`; Coffer never runs a model.
- Canvas: one column `--container-reading` (560px) wide, 24px top and 48px inline padding.
  Byline as the agent card header (sparkle, "Claude", "Claude Code · saved 1 Nov, 09:12"),
  the title in display type (32/38 600), a caption with the kind and period, then the body
  in reading type: 13/20 paragraphs, 15/20 600 headings 8px above their text, "–" lists in
  a 12px lane; figures inside text in mono.
- Inspector "About this review": a caption that it was written by an AI client (read it
  as advice); Period, Client ("Claude Code · standard access"), Prompt (mono), Saved;
  "What it read" — the tool calls from the access log with counts, "11 tool calls returned
  214 transaction rows", outline "Open access log" (D6.8); destructive outline "Delete
  review…". Safe rendering, the `ai_authored` marker and withheld text: see "Safe report
  rendering" below (D6.11).

**Safe report rendering** — drawn in D6.11 (M4.10, M2.13). A 1440×900 artboard,
Reviews · safe rendering (`C6D-0`), duplicated from Reviews with "Subscription check"
(24 Oct) selected, plus a states board, Reviews · D6.11 states (`E43-0`, 1440 wide). Every
value is a bound token; typed values as on D6.1 (1px borders, SVG icon paths). No new
tokens (hash still `6a126241`). A review is AI text, so the renderer treats it as
untrusted: a small markdown subset renders, everything else shows as the text it is, and
nothing in a review can load, open or run anything. The stored `body_markdown` is never
changed; all of this happens at render time.
- **The subset** (states board, "What renders"). `#` and `##` are reading headings
  (15/20 600, `tracking-tight`, 8px above); `###` and deeper are 13/20 600. The review's
  title comes from the report's `title`, not the body. `**strong**` is 600;
  `*emphasis*` is 500 in `text-strong`, because Coffer bundles no italic face (open
  question 1). `` `code` `` is mono 12px `text-secondary`; a fenced block sits on `field`,
  `radius-md`, 8×12 padding, mono 12/20 `text-strong`, no highlighting. Lists: "–" in a
  12px lane, numbers in mono 12px `text-muted` in a 16px lane, both 10px from the text;
  one nested level (indented by the lane and gap, its "–" `text-faint`), deeper items join
  it. A quote: 12px inset behind a 2px `border-strong` rule, `text-secondary`. `---` is a
  hairline with 4px above and below. GFM tables use the data-table language: caps header
  (11/14 600 +0.06em `text-muted`), 28px rows, hairlines, 12px column gaps, 12/16 cells;
  a cell holding only a figure is mono; alignment comes from the `--:` row; a table wider
  than the measure scrolls inside itself. Figures in running text are mono, as in every
  review (D6.1).
- **Links** are inert. `[text](url)` renders the text, a space, then the address in mono
  12px `text-muted`: no underline, no pointer, no hover, not clickable, but selectable
  and copyable. A link whose text looks like an address shows both, so a mismatch
  (`spotify.com` → `spotify-billing.example`) can't hide. A bare URL or `<autolink>`
  shows once, the same way. Long addresses wrap anywhere.
- **Images** are never fetched. `![alt](url)` becomes a block: 1px dashed `border-strong`,
  `radius-surface`, 10×12 padding; a 14px `image` icon, "Image not loaded" (12/16 500
  `text-secondary`) and the alt text in quotes (`text-muted`), then the address in mono
  11/14 `text-muted` aligned under the label. The address is shown because an image URL
  is the usual way to carry data out (`…&d=38.00,30.00,…`).
- **HTML** is never parsed as HTML. Inline HTML shows as written, tags and all, in mono
  12px `text-muted`. An HTML block (`<details>`, `<script>`, `<iframe>`, `<style>`, …)
  shows in a code block headed by a 24px row: a 12px `code` icon (`‹ ›`) and "HTML, shown
  as text" (caption `text-muted`), above a hairline. Entities (`&amp;`) are decoded;
  nothing else is.
- **Anything outside the subset** (task lists, strikethrough, footnotes, maths) keeps its
  markdown characters and reads as plain text: `[ ] ~~Cancel the free trial~~ done[^1]`.
- **Hidden characters.** Control, zero-width and bidi characters are removed at render
  time (they can hide or reorder words) and counted in the inspector.
- **The `ai_authored` marker.** In the app: an agent badge, "AI-written", at the right
  end of the byline (`moldavite-soft`, `moldavite` 10/12 700, radius 8, 16px high; 6px
  inline padding where count badges have 4px, because it holds a word). In the
  inspector's details, a "Read back" row with `ai_authored` in mono and a caption: an AI
  client reading the review back gets it marked as written by AI, an opinion and not the
  user's data. Over MCP: `list_reports` returns `"ai_authored": true` on every report
  (states board, "What the AI client gets").
- **Inspector "Shown as text"**, a section between the details and "What it read", only
  when the review has something to show as text. One 24px row per kind, in this order,
  each a 12px icon, a mono count and the kind (12/16 `text-secondary`) and a caption on
  the right (`text-muted`): `link` "3 links · Never opened", `image` "1 image · Never
  loaded", `code` "2 HTML snippets · Never run", `hidden` (an eye, struck through)
  "3 hidden characters · Removed". A kind with nothing is left out. Caption: links, images
  and HTML appear as text, so opening a review never reaches the internet.
- **Withheld text** (M2.13). When the review's tool calls withheld any descriptions, "What
  it read" gains a block after the call summary: a 12px `shield` in `warning` and "2
  descriptions withheld" (12/16 500 `text-strong`, mono count), then a caption: their bank
  text read like instructions to an AI, so the client got a marker instead; check where
  they came from. Outline "Show in Transactions" sits beside "Open access log". Warning,
  not danger: nothing went wrong, but text like that in a bank export is worth a look.
  - **Transactions**: "Show in Transactions" turns on a **"Withheld from AI"** filter chip
    (the Excluded chip's on state, with the `warning` `shield` before the label and a mono
    count). It lists every transaction whose text Coffer withholds, not only the ones
    this review's calls returned.
  - **Transactions inspector**: under "Raw description" (the full text, unchanged, mono),
    a 24px marker row, `warning` `shield` + "Withheld from AI clients" (12/16 500
    `text-strong`), and a caption: AI clients get a marker in its place and still see the
    merchant, amount and date; nothing here changes the transaction.
  - **Over MCP** the field keeps its type: `"description_untrusted": "[withheld: 74
    characters that read like instructions]"`.
- **New icons** (12px, 1.2 stroke, round caps and joins; paths on the states board):
  `link`, `image`, `code`, `shield`, `hidden`.
- **Decided** (2026-11-10, `ARCHITECTURE.md` → "Decision record: safe report rendering
  and withheld text"), all as the designs assume:
  1. `*emphasis*` renders as 500 `text-strong`; no italic face is bundled.
  2. The withheld count is a `withheld_count` column on `mcp_access_log`, summed per
     review over its calls. The "Withheld from AI" filter re-runs the same check over raw
     descriptions, so the log never stores which transactions were withheld.
  3. The marker is `[withheld: N characters that read like instructions]`.
  4. `list_reports` returns `body_markdown` with control, zero-width and bidi characters
     stripped and `ai_authored: true`; nothing inside a report is withheld.

**Review queues** — drawn in D6.4. Two 1440×900 artboards built from the Inbox shell
(`564-0`), plus a states board. Every value is a bound token; typed values as on D6.1
(1px borders, the Dock shadow on dialogs, SVG icon paths). No new tokens (hash still
`6a126241`). Fixture data continues D6.1: the Joint account's balance check on 9 Nov fails
by −£42.50, and a possible duplicate of −£42.50 on the same account explains it.
- **Where they live.** The view tabs have room for only one more at 1440 (it went to
  Rules in D6.7), and the Inbox is already where decisions wait. So the Inbox's left panel gains a CHECKS section between
  INBOX and KIND: "Possible duplicates" (a two-rectangle `duplicate` icon, mono count) and
  "Balance checks" (a `balance` scales icon; a 6px `danger` dot before the count while
  one fails). These are Coffer's own checks, not agent suggestions, so they're neutral:
  no moldavite, and they don't add to the Inbox tab's moldavite badge, which still
  counts pending suggestions only. KIND and SOURCE keep filtering suggestions. A failing
  check also replaces "Reconciled" in the status bar with a `danger` dot and "1 balance
  check failing", and the flagged account's dot in the Accounts panel opens Balance
  checks with that check selected. D6.5 adds a third row, "Owed to you" (see "Refunds,
  reimbursements and shared costs").
- **Possible duplicates** (`8SE-0`). Toolbar: "Possible duplicates", caption "3 pairs ·
  nothing changes until you merge", kbd hints M Merge, N Not duplicates, ←→ Keep, ↑↓
  Move. A table (header and 28px rows like the Inbox's, no action lane): PAIR (flex: the
  two merchant names joined by a `text-muted` "and") · ACCOUNT (140) · WHY FLAGGED (180:
  "1 day apart, text differs") · DATES (72, mono, `text-muted`) · AMOUNT (96, mono).
  - **The selected pair opens in place** into a side-by-side comparison below its row.
    The label lane is `column-amount` (96px) wide, then two equal columns 24px apart. Each
    column is headed by what happens to that row: `check` + "Keep" (600, `text`) with a
    caption saying where the row came from ("Imported, with the bank's ID"), and
    `excluded` + "Exclude as duplicate" (600, `text-secondary`, "Added by you on 5 Nov")
    with an outline compact "Keep this instead" (←→ does the same). Fields in 24px rows,
    4px apart: Date, Merchant, Amount, Category, Bank text (raw description, mono), Bank
    ID (mono). **Values that differ are `text`; values both rows share are
    `text-muted`**, so the eye goes to the differences. A missing value reads "None" in
    `text-muted`. Footer, aligned with the value columns: "Merge" (neutral primary:
    merging is undoable and deletes nothing), outline "Not duplicates", and the caption
    "The excluded row stays under the Excluded filter. Undo any time." A hairline closes
    the pair.
  - **Which row is kept by default:** the one with the bank's ID; if both or neither have
    one, the one imported first. A manual row loses to an imported one.
  - A caption under the table explains the rule: exact duplicates are skipped at import;
    a pair is flagged when one account has the same amount twice within 3 days under
    different text.
  - **Inspector.** Header: merchant, the amount as display figure, "Joint account ·
    possible duplicate". "Why it was flagged": one 24px row per criterion, a 12px icon
    then the criterion, with its value as a caption on the right. Met criteria take a
    `text-muted` `check` (Same account, Same amount, Within 3 days). The two that stopped
    exact dedupe take a `warning` `differs` (≠) icon (Different text, Bank ID on one
    only). A caption follows: an exact match is skipped at import; these two differ, so
    Coffer asks. "If you merge" (read-only field rows): Excludes "OCADO.COM · 5 Nov",
    Spending "Groceries £42.50 less", Balance "−£24.10 → £18.40", Check "`check` 9 Nov
    passes". A caption names the balance check the pair explains. Then "Both
    transactions" with outline "Show in Transactions".
  - **After a merge** the pair leaves the queue, the next pair opens, and the D6.3 undo
    bar reads "Merged Ocado: OCADO.COM excluded as a duplicate" with Undo (states board).
    The excluded row gets reason Duplicate, so it also leaves the balance (ARCHITECTURE
    "Decision record: Transactions additions"). Several merges in a row are separate
    `change_batch`es, one per pair.
  - **Not duplicates** dismisses the pair (`status = dismissed`). It changes no
    transaction, and the same two rows are never flagged again.
- **Balance checks** (`98K-0`). Toolbar: "Balance checks", caption "Every balance you
  enter or import is checked against the transactions · 1 failing". One table section
  per account that has balances, failing accounts first, divided by hairlines.
  - **Group header** (28px): a 16px status lane (empty), the account name (11/14 600
    `text-secondary`), then the lock state: a 10px `lock` and "Locked through 30 Sep"
    (caption `text-muted`, date in mono). No lock, no text. Lane labels: DATE (72) ·
    BALANCE · COFFER · DIFFERENCE (96 each, right-aligned).
  - **Rows**, newest first: the status icon (`check` `text-muted` passed, `alert`
    `danger` failed), the source ("Entered by you", "Statement (OFX)"), mono date, the
    balance given (`text`), Coffer's computed balance and the difference (mono). Passed:
    difference `£0.00` in `text-muted`. Failed: the difference is `danger`, 600; Coffer's
    balance follows the Money rule (a negative balance is `danger`). The oldest balance
    is the **starting point**, not a check: its source reads "… · starting point" in
    `text-muted`, no icon, and "—" for Coffer and Difference.
  - **Failing window**, under a failed row, indented to the source lane: a caption naming
    the last balance that matched and the transaction count since ("The gap opened after
    30 Sep, the last balance that matched: 47 transactions to look at"), and, when one
    exists, the possible duplicate that explains the gap. Outline compact "Review pair"
    and "Show 47 transactions" (Transactions filtered to the account and the window).
  - **Lock prompt**, at the foot of an account whose checks all pass beyond its lock: a
    caption and an outline compact "`lock` Lock through 31 Oct…", which opens the lock
    dialog. **One balance only** (Current account, a Monzo export with no balances): the
    starting point and a caption that there's nothing to check yet, with outline "Add
    balance…" (the D6.1 balance dialog).
  - A closing caption lists what isn't checked: accounts with no balance (Amex),
    valuation accounts (a valuation sets the balance; D6.9), and pots (they follow their
    transfers).
  - **Inspector** (a failed check selected). Header: account name, the difference as
    display figure, "Check on 9 Nov failed · Coffer is short". "This check" (read-only
    field rows): Balance "£18.40 entered by you", Coffer "−£24.10 from transactions",
    Matched "£512.40 on 30 Sep", Window "1 Oct – 9 Nov", and a caption: Coffer starts
    from the last balance that matched and adds every transaction since. "What could
    explain it": a card (`float`, 1px `hairline`, `radius-surface`, 12px padding) for the
    possible duplicate, with its amount, a caption ("Merging it closes the gap exactly.")
    and outline "Review pair". Then a caption with the other usual causes (a transaction
    missing from an import, one dated on the wrong side of the check, a balance typed
    wrong) and outline "Edit balance…". "Lock": a 12px `lock`, "Locked through 30 Sep"
    (600), outline "Unlock…", and a caption saying what the lock does and that the user
    can lock through 9 Nov once this check passes.
- **States board** (`9MQ-0`, 1440 wide).
  - **Lock dialog** (`--container-dialog`): "Lock Savings through 31 Oct?", a body saying
    what gets flagged and that nothing changes now, a Through select (passing check dates
    only, latest first: "31 Oct 2026 · latest passing check"), Cancel / "Lock" (neutral
    primary).
  - **Unlock dialog**: "Unlock Joint account?", a body (changes dated on or before the lock
    stop being flagged; the checks keep running; lock again once they pass), Cancel /
    "Unlock" (neutral primary: unlocking is reversible and changes no data). Unlock clears
    the lock; locking again picks a date.
  - **The merge undo bar** (above).
  - **Merging into a locked period**: the pair's footer gains the Import dialog's
    `warning-soft` locked-period block ("The excluded row is in a locked period", the
    lock date and the row's date, outline "Unlock…"). Merge is at `opacity-disabled`
    until the account is unlocked, or until "Keep this instead" makes the excluded row
    one outside the lock. "Not duplicates" stays enabled: it changes no transaction.
  - **Empty queue**: centred in the canvas, a 36px `field` icon well with the 16px
    `duplicate` icon, "No possible duplicates" (title type) and a caption: Coffer checks
    every import, and a pair marked "Not duplicates" isn't flagged again.
- **Built in M3.12** (`crates/app/src/checks/`; `ARCHITECTURE.md` → "Decision record:
  review queues (built)"): both queues, the states board's dialogs, undo bar, locked
  block and empty queue, the status bar's failing count and the flagged dot. Left out
  until their items (user, 2026-11-14): "Show in Transactions", "Show N transactions",
  "Edit balance…". **Composed from existing specs, not drawn** (user, 2026-11-14):
  - *Selection*: opening a queue opens its first pair or first failing check; nothing
    selected: one caption in the inspector.
  - *A passed check or a starting point selected*: the same header and "This check"
    (Balance, Coffer, Matched, Window) without "What could explain it"; a starting
    point's section is "This balance" with a caption that Coffer counts from it.
  - *Lock section* on every check's inspector: "Locked through …" with Unlock…, or
    "Not locked" with the `lock` "Lock through …" outline when a date is lockable.
  - *Moves*: a read-only field row in "If you merge" naming the values that move to
    the kept row ("Category to Ocado"), only when something moves.
  - *Dismissal undo*: the merge's undo bar, "Ocado and OCADO.COM marked as not
    duplicates", whose Undo reopens the pair.
  - *A merge that breaks a check*: Check "`alert` 30 Sep fails" in `danger`, and a
    caption that the balance counts both rows.
  - *Refusals*: under the toolbar, a 12px `alert` and the reason in caption type,
    `danger`.
  - *Empty Balance checks*: the empty Possible duplicates layout with the 16px scales,
    "No balances to check".
  - Two 16px icons drawn from the 12px ones at a 1.2px stroke (`DuplicateLarge`,
    `BalanceLarge`), and the locked block's 14px lock (`LockedPeriod`), all from Paper's
    paths.
- **Data-model questions: decided** (user, 2026-11-10; `ARCHITECTURE.md` → "Decision
  record: review queues"). Each went with the recommendation below, which the designs
  already assume.
  1. *Is the oldest balance a check?* Recommended: no. The first `balance_snapshot` of an
     account is the starting point that Coffer counts from; every later one is a check.
     A `valuation` snapshot sets the balance and is never a check.
  2. *What can be locked?* Recommended: only through the date of a passing check, and only
     when every check up to that date passes. Unlock clears `reconciled_through`; there's
     no partial unlock.
  3. *Which row does a merge keep by default?* Recommended: the row with a source ID, then
     an imported row over a manual one, then the one imported first. The user can swap
     before merging.
  4. *Near-duplicate window.* The designs use 3 days. A default, not an invariant (see
     "Defaults, not invariants").
  5. *Does a dismissed pair stay dismissed?* Recommended: yes, for that pair of
     transactions, for good. A new import that brings a third match is a new pair.

**Refunds, reimbursements and shared costs** — drawn in D6.5. Two 1440×900 artboards
duplicated from Transactions (`3WT-0`), one from Review · balance checks (`98K-0`), and a
states board. Every value is a bound token; typed values as on D6.4 (1px borders, the
Dock shadow on dialogs, SVG icon paths). No new tokens (hash still `6a126241`). Fixture
data continues D3/D6.4: the ASOS.com refund (+£34.00, 8 Nov, Joint account) and a
28 Oct purchase of −£68.00; Odeon (−£31.80, 7 Nov) shared with Sam; a work trip
(Trainline, Premier Inn Leeds) and an Uber to claim back; a +£150.60 repayment from
ACME LTD EXPENSES on 9 Nov.

Three kinds of link, one per `transaction_link.kind`. A **refund** ties money in to a
purchase; it takes the purchase's category, so it nets against it and never counts as
income. A **reimbursement** ties money in to a purchase marked to claim back, and clears
the claim. A **settlement** ties money in to a person (a share label) and reduces what
they owe. All three are made by the user or confirmed from a suggestion. Coffer's own
suggestions are neutral (no moldavite); the AI's `refund_link` suggestions are Inbox items
(D6.6).

- **Two new inspector sections** on every transaction, so they're always in the same
  place.
  - **Share** (outflows only), between Details and Links: an inspector-size segmented
    control with "Just me" (the default; nothing else shows), "Shared" and "Claim back".
  - **Links** (every transaction), between Share and Split: title "Links", outline
    "Link…", then one 28px row per link: a 12px `link` icon, what it is ("Refund", "Refund
    of ASOS.com", "Reimbursed") with its date in mono caption `text-muted`, the mono
    amount, and a 24px ✕ icon button that unlinks. Unlinking needs no dialog: the D6.3
    undo bar offers Undo. With no links, a caption says what can be linked.
- **Refund link** (`9WS-0`). The ASOS.com refund is selected. Its Links section holds
  Coffer's suggestion as a card (`float`, 1px `hairline`, `radius-surface`, 12px
  padding, like D6.3's rule prompt):
  - "Is this a refund?" and why ("Same merchant and account, 11 days earlier, for more
    than this amount");
  - the purchase as a 28px `field` strip (mono date, merchant, mono amount);
  - what linking changes ("it takes the purchase's category, so it's £34.00 less
    Shopping and never income. The purchase shows £34.00 of £68.00 refunded");
  - outline "Link as refund", ghost "Not a refund". "Not a refund" rejects the
    suggestion for good (`status = rejected`).
- **After linking** (states board `FDC-0`). The refund's Category row turns read-only:
  marker and name in `text-secondary`, a 10px `link` icon in `icon-faint` where the
  lock goes, and a caption saying the purchase sets it ("Unlink to set it yourself").
  The purchase's Links row reads "Refund · 8 Nov +£34.00" with the caption "£34.00 of
  £68.00 refunded, so Shopping counts £34.00".
- **Link dialog** (states board `EUY-0`, `--container-dialog`): "Link ASOS.com +£34.00",
  a Kind segmented control (Refund / Reimbursement / Settlement), a search field
  prefilled with the merchant, then candidates in 28px rows (mono date, merchant ·
  account, mono amount): same merchant first, then newest. A purchase that can't take
  more (already fully refunded) is at `opacity-disabled` with the reason. Amount (`£`
  prefix, mono) defaults to the smaller of the money in and what the purchase can still
  take, so one refund can be split across purchases. Cancel / "Link refund" (neutral
  primary: linking is undoable). Reimbursement lists purchases marked to claim back;
  Settlement lists people with what each owes.
- **Shared cost** (`BAT-0`). Odeon is selected with Share on "Shared":
  - Yours: a mono `£` field, "15.90", with "of £31.80" as a caption suffix
    (`my_share_minor`);
  - Owed by: a select with a 10px `lock` before the person's name (share labels never
    leave the app, like private tags) and their part in mono `text-secondary`;
  - a caption: budgets and reports count your share, the rest waits under Owed to you
    until a payment from that person is linked, and names stay on this computer.
  One person per transaction or split line: to share with two people, split the
  purchase.
- **Claim back** (states board): Still owed (mono, "£18.60 of £38.60", read-only), a
  caption that budgets and reports leave it out while it's claimed and it stays on Owed
  to you until repayments cover it, and the reimbursement in Links ("Reimbursed · 9 Nov
  +£20.00"). A part payment leaves the rest owed.
- **Table rows.** Amounts stay what the bank moved; the category cell says how a row
  counts, with a `text-muted` suffix: "Shopping · refund", "Shopping · refunded",
  "Entertainment · shared", "Transport · to claim", "Transport · claimed". Repayments
  have no category: "Repays 2 claims", "Repays Sam" (`text-secondary`, the 8px marker
  lane left empty so the text lines up). Day totals stay the bank's.
- **Owed to you** (`CCF-0`), a third row in the Inbox's CHECKS section (a 12px `owed`
  return-arrow icon, mono count of open items). Like the other checks it's Coffer's
  own list: neutral, outside the Inbox badge. Toolbar: "Owed to you", caption "£225.73
  to come back · 3 expense claims and 2 people". Two table sections:
  - **Expense claims**, oldest first. Lanes: a 16px status lane · the purchase (flex) ·
    PAID FROM (140) · SPENT (72, mono) · OWED (96). When an incoming payment matches some
    claims exactly, those rows show a 12px `link` icon and a strip under the section
    (indented to the text lane, like D6.4's failing window) names it: "+£150.60 from ACME
    LTD EXPENSES on 9 Nov matches the two marked claims exactly. Link it and both clear."
    with outline compact "Link as reimbursement" and ghost "Not this".
  - **Shared costs**, one row per person: name · COSTS ("2 since 30 Oct") · LAST PAID (mono
    date or "—") · OWED.
  - A closing caption: how items get here, that part payments leave the rest, and that
    people are labels on this computer, never sent to an AI client.
  - **Inspector** (Sam selected): name, what they owe as an unsigned display figure (the
    label gives the direction, see "Money"), "Owes you for 2 shared costs". Costs (their
    part of each: mono date, merchant, mono amount); Payments from Sam (outline "Link a
    payment…", each settlement, a caption with the settled date and "Paid in cash? Add it
    to your Cash account, then link it here"); Name (a field with the `lock`, renames the
    label everywhere) and a caption that it never goes to an AI client.
- **Empty queue** (states board): the D6.4 empty state with the `owed` icon, "Nothing
  owed to you", and a caption on how purchases get here.
- **New icons**, both 12px outline: `link` (two chain links) and `owed` (a return
  arrow). They'll be `Glyph::Link` and `Glyph::Owed` when M3.11 builds them.
- **States board** (`EB0-0`, 1440 wide): the link dialog and its undo bar; the purchase
  and refund inspectors after linking, and claim back partly repaid; every new row kind;
  the empty queue.
- **Data-model questions: decided** (user, 2026-11-10; `ARCHITECTURE.md` → "Decision
  record: refunds, reimbursements and shared costs"). Each went with the recommendation
  below, which the designs already assume.
  1. *Which month does a refund count in?* Recommended: its own date, as negative spending
     in the purchase's category. The purchase's month stays as it was, so a closed month
     doesn't change when a late refund arrives.
  2. *When does Coffer suggest a link?* Recommended, deterministic only: a refund when
     money comes in on the same account under the same `merchant_display` as a purchase
     in the previous 90 days with at least that much left to refund; a reimbursement
     when money comes in that equals one or a set of outstanding claims to the penny,
     within 60 days of the newest. Suggestions are `transaction_link` rows with
     `status = suggested`, shown in the inspector and on Owed to you, not in the Inbox.
     A rejected pair is never suggested again.
  3. *Personal or gross by default?* Recommended: spending, budgets, Overview, Trends and
     MCP reads use your share (personal); a purchase being claimed back counts as £0
     while it's outstanding, and switching it to "Just me" counts it in full again
     (for a claim that won't be paid). Balances, net worth and the Transactions table are
     always gross. A gross toggle on reports can come later.
  4. *Overpayment.* Recommended: a link's amount is capped at what the purchase can
     still take or what the person owes. The rest of the incoming money stays unlinked
     and counts by its own category.
  5. *Currency.* Recommended: a link needs both sides in the same currency in v1; FX
     refunds wait for L.28.

**Accounts you keep yourself** — drawn in D6.9. One 1440×900 artboard, Accounts · asset
(valuations) (`BSI-0`, duplicated from Transactions · manual (cash account)), and a states
board, Accounts · D6.9 states (`EPF-0`, 1440 wide). Every value is a bound token; typed
values as on D6.1 (1px borders, the Dock and Toolbar shadows, SVG icon paths, data-driven
chart positions, the 50% radius of a chart point). No new tokens (hash still
`6a126241`). Fixture data continues D6.3: Cash (£23.00) plus four valuation accounts,
House £385,000.00, Workplace pension £41,820.00, Stocks ISA £7,030.55 and Mortgage
−£212,640.00, so net worth reads £232,443.55.
- **Accounts panel.** A second section, ASSETS AND LIABILITIES, sits between ACCOUNTS and
  CATEGORIES, with its own `+` (opens New account with Kind set to Asset). Cash stays in
  ACCOUNTS: it has transactions like a bank account. Stocks ISA moves here, since its
  balance is a valuation (D6.1 already showed "Your valuation on 31 Oct"). Two new
  12px icons, outline like the rest: `asset` (an isometric box: something you own) and
  `liability` (a receipt with a minus). The earlier artboards' left panels still show
  the D6.1 list with Stocks ISA under ACCOUNTS; they change when next touched.
- **A liability isn't overdrawn.** Mortgage reads −£212,640.00 in the panel's usual
  `text-muted`, not `danger`: a liability is negative by nature, and `danger` would sit on
  it forever. See "Money" (changed in D6.9).
- **Valuation account selected** (`BSI-0`, House). Transactions tab; an asset or
  liability has no transactions, so its valuations replace the table.
  - **Toolbar** (52px, bottom hairline, like Balance checks): "Valuations" in title type,
    caption "5 values you entered · net worth uses the latest one on or before each date",
    and outline "Add valuation" with kbd ⌘N on the right. ⌘N adds the record the selected
    account takes: a transaction on a bank or cash account, a valuation here.
  - **Chart** (16px inset, 160px plot): "Value" plus caption "Held until the next
    valuation · Mar 2024 – today". A 1.5px `text` **step line**: the value holds flat
    until the next valuation, which is exactly how net worth reads it, and it runs on to
    today. No area fill, so the y axis fits the values (£340k–£390k) instead of starting at
    £0; the bottom gridline is `hairline`, not the `border-strong` of a £0 baseline, so it
    doesn't read as zero. Each valuation is a 6px `text-secondary` point; the latest is
    selected (6px `text` dot with a 2px `canvas` ring, molten label "12 Oct ·
    £385,000.00"). X labels every six months, mono 10/12 `text-muted`.
  - **Table**: DATE (72, mono `text-muted`) · NOTE (flex, `text`) · CHANGE (96, signed,
    `text-secondary`) · VALUE (96, mono `text`), newest first. The oldest row is the first
    value: "Purchase price · first value", CHANGE "—". A caption under the table: a
    valuation sets the balance, so it isn't checked; the account counts toward net worth
    from its first value; payments toward it (a deposit) stay on the account they left.
  - **Inspector** (account selected): header name, value as display figure, "Valued by
    you on 12 Oct · in net worth". Details: Name, Kind (select), Currency (read-only with
    the lock). No Bank, no Preset: nothing is imported. "Valuations" with outline "Add
    valuation…" and a caption on how net worth uses them. Footer: outline "Close
    account…" and destructive outline "Delete account…" (the D6.3 small danger outline),
    with a caption: closing on the sale date adds a £0 value then; delete only an account
    added by mistake.
- **States board** (`EPF-0`).
  - **Liability inspector** (Mortgage): −£212,640.00 in `text` (not `danger`), "Owed on 1
    Oct · from your statement". For a liability the section is "Balances" / "Add
    balance…", since "valuation" reads oddly for a debt; it's the same record. Rows: date
    (56px lane, so "2 Oct 25" fits), note, signed amount. Caption: monthly payments stay
    spending on the account they leave; net worth catches up at the next statement.
  - **Cash inspector**: "Balance on 10 Nov · from your count and entries". The Balances
    section becomes "Counts" with outline "Count cash…"; the oldest count is the
    "starting point" (as in Balance checks). Footer: "Close account…" only, because the
    account has transactions.
  - **New account, Kind set to Asset**: Bank and Preset disappear; Balance becomes
    "Value" (`£` prefix, mono, with its date) and a Note row is added. Caption: nothing is
    imported for an asset; for a liability, enter what you owe and Coffer stores it
    negative.
  - **Kind menu** (`float`, 1px `border`, `radius-lg`, Toolbar shadow, 4px padding): two
    groups under 10/12 caps labels, "Imported from your bank" (Current, Savings, Credit
    card) and, after a hairline, "Kept by you" (Cash · notes and coins, Asset · house,
    pension, investments, Liability · mortgage, loan), with the hint in `text-muted`. The
    chosen item has the `hover` fill and a `check`.
  - **Cash count** (`--container-dialog`), when the count is short: Counted (`£` field
    and date), then read-only rows: Coffer "£23.00 from transactions since the 1 Nov
    count" and Difference "−£5.00" in `warning` with the `alert` icon (as an unbalanced
    split: unresolved, not yet a failure). Footer: Cancel, outline "Save count only",
    neutral primary "Add −£5.00 and save", which also adds a manual transaction "Cash count
    difference" in Other on the count's date so the check passes. Nothing is added unless
    the user picks it.
  - **Add a balance for Mortgage**: Owed (`£`, unsigned, with its date) and Note. Caption:
    type what the statement says; Coffer stores it as −£212,640.00 and never treats it as
    a failed check.
  - **Close House?**: body first (it leaves the panel and counts as £0 from the date,
    so net worth drops by its value then; earlier net worth keeps its valuations; it can be
    reopened), then "Sold on" (96px date field), Cancel / "Close account" (neutral primary:
    reversible).
- **Data-model questions: decided** (user, 2026-11-10; `ARCHITECTURE.md` → "Decision
  record: accounts you keep yourself"). Each went with the recommendation below, which the
  designs already assume.
  1. *A note on each valuation.* Recommended: `balance_snapshot` gains an optional
     `note`, user text treated like `user_note` (not sent over MCP by default).
  2. *Can a valuation account have transactions?* Recommended: no. `asset` and
     `liability` accounts hold `valuation` snapshots only; a payment toward one stays on
     the paying account (spending, or a transfer the user categorises), and net worth
     catches up at the next valuation. Linking payments to a loan balance can come later.
  3. *Closing.* Recommended: `account.closed_on` (date) for every kind. Closing a
     valuation account also writes a £0 `valuation` snapshot on that date, which reopening
     removes. An account with transactions keeps D6.1's close-only rule.
  4. *Deleting.* Recommended: an account with no transactions (a valuation account, or a
     new empty one) can be deleted with its snapshots. It isn't undoable, so the
     confirmation says how many values go with it.
  5. *Cash counts.* Recommended: a count is a `balance_snapshot` with source `manual`,
     checked like any other balance (the first is the starting point, as in D6.4
     question 1). "Add and save" writes the snapshot and the difference transaction in
     one DB transaction; Coffer never posts a difference on its own.

**Inbox impact preview** — drawn in D6.6 (M3.14, M2.14). Two 1440×900 artboards built from
the Inbox (`564-0`) with the D6.4 CHECKS panel, Inbox · rule impact (`ABK-0`) and Inbox ·
merchant name impact (`DL8-0`), and a states board, Inbox · D6.6 states (`EWZ-0`). Every
value is a bound token; typed values as on D6.4. No new tokens (hash still `6a126241`).
Fixture data continues D6.1, plus a merchant name (`LIME*RIDE` → Lime) and a refund link
(Uniqlo +£29.90 on 7 Nov, half of a −£59.80 purchase on 24 Oct): 14 pending.

The point is containment (ARCHITECTURE → "Impact preview"): text in a transaction can steer
an agent, so every suggestion says what approving it would change, row by row, before it
can be approved. Nothing in the preview is the agent's own words except the card's reason.

- **Kinds and order.** KIND lists Rules, Merchant names, Transfer pairs, Refund links,
  Categorisations, and the canvas groups follow that order: **widest reach first**, so
  the suggestions that touch the most history are on screen without scrolling and one-row
  categorisations come last. "Merchant names" is D6.7's name for `merchant_alias` (its
  12px name-tag icon); refund links use D6.5's `link` icon.
- **Lanes per kind** (the D6.1 lanes, relabelled): Rules: DECIDES NOW · SUGGESTED ·
  CHANGES (how many past transactions would change category, not how many match).
  Merchant names: SHOWS AS NOW ("9 names" when matches show several) · SUGGESTED (the new
  name) · RENAMES. Refund links: NOW ("Income": an unlinked inflow counts as income) ·
  SUGGESTED ("Refund of 24 Oct") · DATE · AMOUNT.
- **The selected suggestion opens in place** (D6.4's pattern), indented 16px under its
  row, with its own lane header in 10/12 caps `text-faint`, 24px rows on the same lanes
  as the group so the eye runs straight down: BANK TEXT (mono, the matched part in
  `text-strong`, the rest `text-muted`) · NOW (marker + category, or the current name) ·
  AFTER (the table's AI-suggested style) · DATE · AMOUNT. A row the change leaves alone
  says why in AFTER: a 10px `you-set` person icon and "Stays, you set it" / "Stays, you
  named it" in `text-muted`, its NOW value in `text`. Footer: outline compact "Show 6 in
  Transactions" and a one-line caption. Rules with up to 10 matches list every one; more
  than that lists the newest 6 under "Bank text · 6 of 14 matches".
- **Rule** (`ABK-0`, Merchant contains UBER *EATS → Eating out). Six matches: five filed as
  Transport by Rule UBER change; one the user set to Groceries stays. Inspector: header
  (the rule, "5 of 6" display figure with "of 6" `text-muted`, "Past matches that change
  category · new rule"); the agent card ("File UBER *EATS under Eating out" and the
  reason); **If you approve** (D6.4's "If you merge" rows): Changes "5 past transactions",
  Keeps "1 category you set", Spending as two marker rows ("Transport £91.20 less",
  "Eating out £91.20 more"), Settles "The 4 Nov suggestion below" (approving the rule
  also settles the matching categorisation), and a caption: one change you can undo; a
  rule from an agent never changes a category you set, open it in Rules to include
  those. Then **The rule** (When, Then, Order "Before Rule UBER", D6.7's section names)
  and "6 matches" + Show in Transactions.
- **Merchant name** (`DL8-0`, Starts with LIME*RIDE → Lime). Fourteen matches; thirteen
  rename, one the user named "Lime monthly pass" stays. If you approve: Renames "13
  transactions", Keeps "1 name you set", Groups "9 names into one", Spending "No change",
  Rules "No change", with the caption that rules match the bank's text, not the name (as
  D6.7 says), so none starts or stops matching. Then **The merchant name** (Match, Shows
  as).
- **Refund link, transfer pair, categorisation** (states board, inspector only; the
  canvas row doesn't expand, since there is nothing more to list):
  - Refund: Links to "Uniqlo · 24 Oct · −£59.80", Refunded "£29.90 of £59.80", Income
    "£29.90 less in Nov", Spending "Shopping £29.90 less in Nov", Purchase "Shopping ·
    refunded" (D6.5's category cell), and a caption that a linked refund takes its
    purchase's category and counts in the month the money came back (D6.5 question 1).
  - Transfer pair: Income and Spending "£250.00 less in Nov", Balances "No change".
  - Categorisation (replaces the D6.1 caption): Changes "This transaction only", the two
    Spending rows, Set by "You · was Rule UBER", and a caption pointing at the rule that
    would cover this one and 4 more.
- **A wide rule** (states board; M3.14's "changes 143 past transactions; 12 have a
  category you set"). A rule is *wide* when its matches span more than 5 merchants (a
  default, not an invariant). Then:
  - CHANGES is `warning`, 600;
  - the open preview starts with D6.4's `warning-soft` block, `warning` triangle icon:
    "This rule reaches 38 merchants, including your salary", "It would change 131 of 143
    past transactions. A narrower pattern is safer…", and outline compact "Edit in
    Rules…" (opens D6.7's editor with the rule prefilled);
  - matches are summed **by merchant** (MERCHANT · NOW · AFTER · COUNT · AMOUNT), biggest
    surprise first: income turning into a bill sorts above everything else, then by
    count, then "and 35 more merchants" and "Categories you set · Stays";
  - **Approve all leaves it out** ("Approve all (1)", with "Leaves out PAY: a wide rule
    needs its own ✓" beside the buttons). Only the row's own ✓ or the inspector's Approve
    approves it.
- **Coffer's own suggestions** (M2.14, source `coffer`): the same card and preview, but
  neutral. Card: `float` fill, 1px `hairline`, `radius-lg`, the mono mark (12px,
  `text-secondary`) and "Coffer" 600 + "from your edits · today" `text-muted`, the
  proposal, a reason that says it was counted ("You moved WASABI to Eating out by hand 3
  times"), neutral primary Approve + outline Reject. Its group footer names "Coffer ·
  from your edits" with the mono mark; its buttons are neutral; it doesn't count
  towards the Inbox tab's moldavite badge.
- **After approving** anything that changes existing rows (rule, name, pair, link), the
  D6.3 undo bar: "Approved rule UBER *EATS: 5 transactions now Eating out", Undo. One
  `change_batch` per approval; Approve all makes one per suggestion.
- **States board** (`EWZ-0`, 1440 wide): the refund link, transfer pair and categorisation
  inspectors, Coffer's own card, the wide rule, and the undo bar.
- **Data-model questions: decided** (user, 2026-11-10; `ARCHITECTURE.md` → "Decision
  record: Inbox impact preview"). Each went with the recommendation below, which the
  designs already assume.
  1. *Does approving an agent's rule change categories the user set?* Recommended: never
     from the Inbox. It applies to uncategorised and rule-set rows only; including the
     user's own rows is a choice made in D6.7's apply-to-past preview ("Change these
     too"). The Inbox has no such toggle, so an approval can't overwrite a hand edit.
  2. *What makes a rule wide?* Recommended: matches spanning more than 5 distinct
     `merchant_display` values, or any match that would move income into a spending
     category. A default, not an invariant.
  3. *Does Approve all skip wide rules?* Recommended: yes, always. A wide rule needs its
     own approval with the preview open.
  4. *Does a rule's approval settle pending categorisations it covers?* Recommended: yes,
     marked approved with the rule's `change_batch`, so the Inbox doesn't ask twice.
  5. *Can a merchant name change which rules match?* Recommended: no. Rules match the
     cleaned bank text before names apply (D6.7), so a name suggestion is cosmetic and
     its preview says so.
  6. *Is the impact computed when the suggestion is filed or when it's shown?*
     Recommended: when shown, against today's data, so a later import or edit can't
     make a stale preview look smaller than the real change.

**Settings** — drawn in D6.8. Three 1440×900 artboards built from the Inbox shell,
Settings · AI clients (`AQ2-0`), Settings · access log (`DC6-0`) and Settings · backups
(`F63-0`), plus a states board, Settings · D6.8 states (`GQA-0`, 1440 wide). Every value
is a bound token; typed values as on D6.4 (1px borders, the Dock and Toolbar shadows, SVG
icon paths). No new tokens (hash still `6a126241`). Fixture data continues D6.1–D6.4:
Claude Code connected at standard access, the November Monzo import, 1,312 transactions.
- **Where it lives.** The view tabs are views of the money, plus Rules (D6.7), which
  decides how the money is filed; Settings is neither, so it doesn't take a tab. A 28px icon button with a 14px `settings` (sliders) icon
  sits at the right of the title bar, before the window controls and a 1×14 hairline
  separator; ⌘, opens it too. While Settings is open no view tab is active and the
  button takes the active icon-button look (`control-active` fill, `text` icon).
  Clicking the button again goes back to the view Settings was opened from (decided by
  the user, 2026-11-14); ⌘, only opens it. The
  status bar's "1 agent connected" opens AI clients, and the Reviews inspector's "Open
  access log" opens the access log filtered to that review's calls.
- **Left panel.** AI ACCESS: Clients (`plug` icon, mono count of clients), Access log
  (`log` icon, calls in the last 7 days). DATA: Backups (`backup` icon, count), Export…
  (`export` icon; opens the export dialog, so it never shows as selected). At the foot,
  above a hairline, a caption and the data file's path in mono ("Everything is in one
  file on this computer"). Home currency, exchange rates and presets will join this panel
  in their own tasks.
- **AI clients** (`AQ2-0`). Toolbar: "AI clients", caption "Each client sees only what
  its access level allows · nothing is shared until you enable a client", and on the
  right the **kill switch**: "AI access" (12/16 500 `text`) and a toggle.
  - *Clients* table (header and 28px rows like the Inbox's): a 16px status lane (the 6px
    moldavite dot while a client is connected, the only moldavite on these screens),
    the client's name with its `--client` id in mono `text-muted` (flex) · ACCESS (180)
    · LAST CONNECTED (140: "Connected now", or a mono date) · CALLS · 7 DAYS (96, mono)
    · ON (52, a toggle per client). A client turned off keeps its row, with the name in
    `text-secondary` and its values in `text-muted`.
  - *Asked to connect*: a client name Coffer hasn't enabled (`coffer mcp --client
    my-agent`) is refused before any tool runs and listed here with LAST TRIED and
    REFUSED (count), the name in mono. Its footer explains where the name came from, with
    outline "Ignore" and "Enable…". This is how a client gets enabled: MCP stays off
    until the user approves one here.
  - *What each level shares*: one row per level with CAN READ (flex) and CAN WRITE (180):
    Aggregate (totals, budgets, net worth; reviews only), Pseudonymous (transactions with
    merchant tokens such as `M-7f3a`; Inbox suggestions and reviews), Standard
    (merchant names and public tags; the same writes). A caption lists what is never
    shared at any level, and says a tool above a client's level isn't offered at all.
  - *Inspector* (Claude Code): name, calls in 7 days as display figure, rows returned.
    Access: Level (select) and a caption saying what it shares; a lower level takes
    effect on the client's next call. Connect: the command in a mono field with a copy
    icon button, and a caption that the `--client` name only tells clients apart and isn't
    a password. Last 7 days: Read (calls · rows), Wrote (suggestions · reviews), Refused,
    outline "Open access log". At the foot, a caption on the difference between turning
    a client off (refused, level kept) and removing it (level forgotten; log, suggestions
    and reviews stay), and destructive outline "Remove client…".
- **Access log** (`DC6-0`). Toolbar: "Access log", caption "Every tool call: which client,
  why it says it asked, and how many rows went back · never the data itself", and outline
  compact filters "All clients ⌄" and "Last 7 days ⌄". Day groups like Transactions
  ("Today, Tue 10 Nov" and a mono count); lanes labelled once, in the first group: time
  (48, mono) · client (140) · TOOL (200: a 12px kind lane, then the tool name in mono
  11/14) · PURPOSE, IN THE CLIENT'S WORDS (flex) · ROWS (96, mono).
  - Reads have an empty kind lane; writes (`propose_*`, `save_report`) show the
    `inbox` icon; a refused call shows a `refused` icon (circle with a slash), and the
    row is `text-muted` with "Refused: my-agent isn't enabled" as its purpose and "—"
    for rows. Refusal is Coffer doing its job, so it's neutral, not `danger`.
  - Purposes are shown in quotes in `text-secondary`; a call without one reads "No
    purpose given" in `text-muted`. The purpose is untrusted text: it's displayed, never
    interpreted.
  - A closing caption: the server writes one line per call, refused ones included, and
    a client can't edit or skip it.
  - *Inspector* (a call): the tool name in mono as the title, the row count as display
    figure, "Rows returned · today at 10:42:07". Client (with its level), Kind ("Read ·
    changes nothing"), Result (mono hash) and a caption that the rows aren't stored, and
    that two calls with the same hash returned the same data. Arguments: a mono block
    (`field` fill, `radius-md`) where dates, keys and numbers are shown and free text
    reads "text, 24 characters". Purpose: the text and a caption to treat it as the
    client's claim.
- **Backups** (`F63-0`). Toolbar: "Backups", caption "Taken before every import, restore
  and update · the last 10 and one a month are kept", outline "Restore from a file…" and
  "Back up now".
  - Two groups, Recent (10) and One a month (3): TAKEN (140, mono date) · REASON (flex:
    "Before importing `monzo-november.csv`" with the file name in mono, "Backed up by
    you", "Before updating Coffer · schema 6 → 7", "Before restoring the backup from 4
    Oct") · TRANSACTIONS (96) · SIZE (96, mono). A caption: backups stay on this
    computer, readable only by the user, and are full copies.
  - *Inspector* (the 10 Nov backup): "Backup from 10 Nov, 14:02", its transaction count
    as display figure. Taken (date · automatic), Holds (accounts, rules, settings),
    Schema ("7 · same as this version"), Size. "Since this backup": what restoring would
    undo, one row each with a mono count (the import of `monzo-november.csv`, 296 rows;
    your edits, 4; Inbox suggestions, 12), a caption that a backup of the current state
    is taken first, and neutral primary "Restore…". File: the path in mono on two
    lines, outline "Show in folder" and "Save a copy…".
- **States board** (`GQA-0`).
  - *AI access off* (872px canvas): the kill switch's toggle off; a neutral banner
    (hairline border, `radius-surface`, 12px padding, 14px `refused` icon, `text-strong`
    title "AI access is off", what it does, outline "Turn on"); the clients table at
    `opacity-disabled`, with no connected dot. Off is a safe state, so the banner is
    neutral, not `warning`. The status bar replaces "1 agent connected" with a 12px
    `refused` icon and "AI access off" in `text-muted`.
  - *Enable dialog* (`--container-dialog`): "Enable `my-agent`?", when it asked, Name
    (a display name, editable) and Level (select, **starting at Aggregate**, the least
    shared) with a caption saying what that level shares. Cancel / "Enable" (neutral
    primary).
  - *Restore dialog*: "Restore the backup from 10 Nov, 14:02?" Two checks as 24px rows
    with a 12px `check` and the check's name on the right in caption type ("The file is
    intact · integrity check", "This version of Coffer can read it · schema 7"); what
    restoring undoes; a caption that the current state is backed up first and AI clients
    are refused until the restore is done. Cancel / "Restore" (neutral primary:
    restoring is reversible through that backup).
  - *A check fails* (Restore from a file): the failed check's row is `danger` with an
    `alert` icon ("It was made by a newer Coffer · schema 9, this reads up to 7"), the
    body says what to do and that nothing changed, and Restore is at
    `opacity-disabled`.
  - *Export dialog* (`--container-dialog`): title with an inspector-size segmented
    control (CSV / JSON) on the right; Dates (two mono fields), Accounts, Categories and
    Tags (selects); a hairline, then toggle rows "Excluded transactions · with their
    reason" (off) and "Notes and tags · including private ones" (on); a caption with the
    counts (transactions and rows; each split line is its own row carrying its parent's
    ID) and where the columns are documented; footer caption "Saved where you choose.
    Nothing is uploaded." with Cancel / "Export…" (opens the system save dialog). Opened
    from Transactions, the selects start from the table's filters.
  - *After "Back up now"*: the D6.3 notification bar with a `check`, "Backed up 1,312
    transactions · 5.3 MB" and outline "Show in folder" in place of Undo.
- **Data-model questions: decided** (user, 2026-11-10; `ARCHITECTURE.md` → "Decision
  record: Settings, AI access and backups"). Each went with the recommendation below,
  which the designs already assume.
  1. *How does the kill switch reach a running server?* Recommended: it's a `setting`
     the server reads on every call, not only at start, so turning it off refuses the
     next call of a client that is already connected. Refused calls are logged, which
     needs an outcome on `mcp_access_log` (`ok`, `refused_off`, `refused_not_enabled`,
     `refused_level`).
  2. *Where do unknown clients come from?* Recommended: the first `coffer mcp --client X`
     for an unknown name creates an `mcp_client` row that isn't enabled (a `requested`
     state) and is refused; "Ignore" hides the row without enabling it. Only the app or
     CLI enables a client, never the server.
  3. *What does a lower level do to a connected client?* Recommended: the server checks
     the level on every call and refuses tools above it, and sends
     `notifications/tools/list_changed` so the client drops them from its list.
  4. *What does argument redaction keep?* Recommended: keys, dates, enum values, numbers
     and IDs; free-text values are stored as their type and length only.
  5. *How long is the access log kept?* Recommended: 12 months, as a default, not an
     invariant (rows are small; one a call).
  6. *Backups and restore.* Recommended: a backup is also taken before every restore and
     on "Back up now", and the reason and time live in the file name, so the list needs
     no table that a restore would roll back. A backup with an older schema is restored
     and then migrated; a newer one is refused. Running MCP servers are refused while a
     restore swaps the files and reopen the database afterwards.

- **Built in M3.15 and M3.16** (`crates/app/src/settings/`): the three screens, the
  states board's off state, the Enable, Restore and export dialogs and the "Back up now"
  notification, checked by eye against `AQ2-0`, `DC6-0`, `F63-0` and `GQA-0` in the
  `settings*` snapshots. New Coffer components, all from existing specs: the toggle
  (`ui::toggle`, on the unstyled `gpui-base` switch), the inspector-size segmented
  control (`ui::segmented`, `gpui-base` toggles), the notification bar
  (`ui::notification`), and the `refused`, `plug`, `log`, `backup`, `export`, `inbox`,
  `copy`, `alert` and dismiss icons from the artboards' paths. No new tokens.
  **Decided with the user (2026-11-14)**:
  - *No display names*: clients are their `--client` name, in Geist Mono, everywhere
    (no migration). The Enable dialog has no Name field.
  - *No live connection state*: LAST CONNECTED is the last call's time ("Today,
    10:42"), with no moldavite dot; nothing records whether a client is connected now.
  - *The access log shows what `coffer ai-access log` shows*: no result hash (the
    inspector's Result row and its caption are replaced by "The rows themselves aren't
    stored: the log keeps how many went back, never the data."); rows cut the purpose
    as the CLI does (60 characters), the inspector shows it whole; arguments are
    lines of key and value, free text as "text, N characters", keys cleaned of
    invisible characters.
  - *The ON toggle keeps the level; Remove forgets it* (`mcp_client::turn_off`,
    `turn_on`, `remove`), as the inspector's caption says.
  - *Backups are read*: each listed backup is opened read-only and `immutable` for its
    transactions, accounts and rules; a reason names the import's file where the current
    database still records that import; "Since this backup" counts imports, bulk edits
    (change batches) and suggestions made after it.
  - *Export*: "Excluded transactions" off leaves excluded rows out
    (`export::Options::include_excluded`); "Notes and tags" turns on notes and private
    tags together; the caption's `docs/EXPORT.md` now exists.
  **Composed from existing specs, not drawn** (review):
  - *Status bar*: "1 agent connected" and "AI access off" open Settings → AI clients;
    the other items are still fixtures.
  - *Empty and failed reads*: no clients reads as a caption under the Clients header
    (how a client asks to connect); an empty log period, and no backups, as a caption
    in the canvas; a read that failed, in caption type `danger` where its data would be.
  - *A client turned off*: its Level is read-only with the lock (`LockedValue`), and the
    caption says to turn it on to change it. While AI access is off, the faded clients
    stay usable.
  - *Remove client dialog*: "Remove `name`?", what removing does and keeps, Cancel /
    "Remove client" (destructive solid). The inspector's button is the outline's 24px
    geometry in `danger` (`ButtonVariant::DestructiveCompact`, Paper `DB7-0`).
  - *Refused and failed calls*: refusals say why ("Refused: AI access was off",
    "Refused: my-agent isn't enabled", …); a failed call reads "The tool failed"; a
    summary of calls the pending file had no room for says how many. The call
    inspector's figure is "—" for them, and Kind reads "Refused · before the tool ran" or
    "Failed · nothing returned"; a write reads "Wrote · to the Inbox".
  - *Log filters*: outline compact buttons with an 8px `icon-faint` chevron (`E2S-0`)
    opening the select menu: All clients and every client in the log; Today, Last 7 days,
    Last 30 days, Last 12 months. Earlier calls load 100 at a time as the list is
    scrolled to its end.
  - *Backups*: Recent holds every manual backup and the automatic ones rotation keeps as
    recent; One a month the rest. A restore backup reads "Before restoring a backup"
    (which one isn't recorded); an update with an unknown schema reads "Before updating
    Coffer". Schema rows say "· updated to N after restoring" for an older backup and
    "· newer than this version's N" for a newer one, whose Restore… is disabled. Nothing
    to undo reads "Nothing has been imported, edited in bulk or suggested since."
  - *Restore dialog*: "Checking the file…" while the checks run; "Restoring…" with the
    buttons disabled and Escape ignored while it restores; a damaged file fails the
    first row ("The file is damaged") and has no second; "It isn't a Coffer backup" and
    "It's from a different build of Coffer" fail the second; a current database that
    can't be read, or a refused restore, is said in `danger` with what to do; an earlier
    restore that stopped part-way is mentioned, as `coffer restore` does. After a
    restore, the notification bar says so, with "Show in folder" for the backup of what
    it replaced.
  - *Notifications*: the D6.3 bar also reports an export ("Exported 1,327 rows to
    `file`"), a saved copy, and failures (the 12px `alert` and the message in `danger`).
  - *Export dialog*: the dates start at the ledger's first and last days (everything);
    an empty date is no bound; a date that doesn't read, or a filter core refuses, is
    said in `danger` and Export… is disabled; private tags are offered only while notes
    and tags are included. Category options hint their group. Opening it from
    Transactions with the table's filters waits for M3.3.

**Rules** — drawn in D6.7 (M3.13, M1.23, M1.24). Four 1440×900 artboards built from the
Review · possible duplicates shell (`8SE-0`): Rules (`B1J-0`), Rules · apply to past
transactions (`EFT-0`), Rules · merchant names (`G2O-0`) and Rules · change history
(`GZT-0`), plus a states board, Rules · states (`HFH-0`, 1440 wide). Every value is a
bound token; typed values as on D6.4 (1px borders, the Dock and Toolbar shadows, SVG icon
paths). No new tokens (hash still `6a126241`). Fixture data continues D6.1–D6.6: the
VINTED edit from D6.3's rule prompt, and the UBER rule that D6.6's UBER *EATS suggestion
goes above. No rule matches a row that Transactions or the Inbox shows as uncategorised
or AI-suggested.
- **Where it lives.** An eighth view tab, **Rules**, after Reviews (decided by the user,
  2026-11-10). Rules are the one thing besides the money that the user returns to, and
  three routes already point here: D6.3's "Create rule…", the inspector's Rule section,
  and D6.6's "Open in Rules…". The left panel has RULES: Rules (→ icon, count),
  Merchant names (12px name-tag icon), Change history (clock-arrow icon). The second
  section filters the selected list: ACTION for rules (Set category, Set merchant, Add
  tag, Set note, Exclude, Split, My share, with the action icons below), MATCH for
  merchant names (Is, Starts with, Pattern), KIND for history (Rule applied, Bulk edit,
  Duplicate merge). Zero counts stay listed, in mono `text-faint` like the Inbox's.
- **Rules** (`B1J-0`, rule 2 selected). Toolbar: "Rules", caption "14 rules · checked top
  to bottom on every import", kbd ↑↓ Move and an outline "+ New rule". A table in
  priority order (header and 28px rows like the Inbox's): # (24px, mono `text-faint`) ·
  WHEN (flex: the field and test in `text-secondary`, the text to match in mono `text`;
  extra conditions follow after a `text-muted` "·", e.g. "· over £1,000") · ACCOUNT (140;
  "Any account" `text-muted`) · THEN (180: the category's marker and label; other actions
  as a `text-muted` suffix, "+ tag", "+ name", "+ my share"; a rule without a category
  shows its first action, "Tag Gift Aid", with the action's icon in the marker lane) ·
  MATCHES (96, mono). **Matches** counts the transactions the rule decides today: it
  matches them and no rule above already set the same fields. Drag a row to reorder.
  - **Never reached**: a rule that matches transactions but always loses to a rule above
    shows "Never reached" (caption `warning`) before a `text-muted` 0. Its inspector has a
    card (`float`, 1px `hairline`, `radius-surface`, 12px padding; `alert` icon in
    `warning`) naming the rule that catches them, with outline "Move above rule 2" and
    "Show rule 2" (states board).
  - A caption under the table: each field is set by the first rule that matches; rules
    run on new imports; past transactions change only through Apply.
- **Rule inspector.** Header: the rule as title ("Merchant contains `UBER`", the text in
  mono), the Matches count as display figure, caption "transactions match · last on 4 Nov
  · rule 2 of 14". Sections, divided by hairlines:
  - **When**: field rows, Field (select: Merchant, Bank text, Bank category, Bank type),
    Test (select: is, starts with, contains, matches pattern), Text (mono field), Account
    (select, "Any account"), Amount (select: Any amount, Over, Under, Between). Caption:
    matching ignores case; Merchant is the cleaned bank name, before any merchant name
    is applied (so a name never changes which rules match, as D6.6 assumes).
  - **Then**: one action row per action: the kind as the field label (64px), its control,
    and a 24px remove icon button (the split editor's line layout). Controls: Category
    (select with marker), Merchant (field), Tag (tag pills, as D6.3), Note (field), My
    share (mono field with a `%` unit), Exclude (reason select, as D6.3's dialog), Split
    (a template of category lines, edited like D6.3's split). Ghost "+ Add action"
    (pulled 8px left so its icon lines up with the labels, like "+ Add line") opens a
    menu of the kinds not yet used (below). Each kind at most once; Category and Split
    exclude each other, since a split sets a category per line.
  - **Past transactions**: caption (edits apply to new imports; to change past
    transactions, apply the rule and see every change first), outline "Apply to past
    transactions…" and "Show 31" (Transactions filtered to the matches). When an agent
    suggestion in the Inbox touches this rule, a sparkle line in `moldavite-text-muted`
    says so, with "Review in the Inbox" in `moldavite-text` (the only moldavite here).
  - **Footer**: "Created from an edit on 12 Mar" (caption, date in mono) and the small
    destructive outline "Delete rule…" (D6.3's "Delete transaction…" size).
- **Apply to past transactions** (`EFT-0`). The canvas swaps the table for a preview,
  with a 24px back button before the title; the left panel keeps Rules selected. Drawn as
  D6.3's "Create rule…" lands: a new rule, Merchant contains `VINTED` → Shopping, not saved
  yet. Toolbar: "Apply to past transactions", caption "New rule: … · nothing changes
  until you apply". Two table sections on one set of lanes: MERCHANT (flex) · ACCOUNT
  (140) · NOW (140) · AFTER (140) · DATE (48) · AMOUNT (96).
  - **Will change** (4): NOW is the current category (dashed "Uncategorised"), AFTER the
    rule's in `text`.
  - **Category you set** (2), under a hairline: rows the user categorised by hand. They
    are left out by default: merchant and amount drop to `text-secondary`, AFTER reads
    "No change" in `text-muted`. A toggle on the section header, "Change these too",
    includes them. Rows already in the rule's category aren't listed.
  - Footer above a hairline: neutral primary "Create rule and change 4" (for an existing
    rule: "Apply to 4 transactions"), outline "Create rule only", caption "Saved as one
    change. Undo it from the bar that follows, or later from Change history."
  - The inspector is the rule being made, with an **Order** section instead of Past
    transactions: Position (select, "First · rule 1 of 15") and a caption that a new rule
    goes first so it wins over broader ones, and whether it overlaps another rule. The
    preview follows the inspector as the user edits.
  - After applying, the D6.3 undo bar: "Changed 4 transactions to Shopping", Undo.
- **Merchant names** (`G2O-0`; `merchant_alias`). Toolbar: "Merchant names", caption "12
  names · how a bank's merchant text is shown", outline "+ New name". Table: BANK TEXT
  (flex: "Is", "Starts with" or "Pattern" in `text-secondary`, then the text in mono) ·
  SHOWS AS (180) · LAST SEEN (72, mono) · MATCHES (96, mono), sorted by name. A caption
  gives the order: Coffer cleans the bank's text (card numbers, "CONTACTLESS",
  processor prefixes), then applies these names, and a name typed on one transaction
  wins over both. Renaming changes only what's shown: rules still match the bank's text,
  and the bank's text is never changed.
  - **Inspector**: the name as title, the match count as display figure ("transactions
    show this name · last on 9 Nov"). Match: Test (select), Text (mono field), Shows as
    (field), caption. "Bank text it catches": up to three cleaned bank texts with their
    last date (mono 11/14), "and 6 more", outline "Show in Transactions". Footer: "Added
    on 3 Oct" and "Delete name…" (small destructive outline).
  - A name applies everywhere at once, with no preview or undo batch: `merchant_display`
    is derived and recomputed (ARCHITECTURE "Merchant names"), and changing the name back
    restores it. D6.6 previews an agent's suggested name because it comes from outside.
- **Change history** (`GZT-0`; `change_batch`). Toolbar: "Change history", caption "6
  changes to existing transactions · newest first". Table: WHEN (96, mono date and time)
  · CHANGE (flex; a rule's text in mono) · KIND (140) · TRANSACTIONS (96, mono). An
  undone change keeps its row: the description struck through in `text-muted`, KIND
  replaced by "Undone 28 Oct". Imports aren't listed: each account keeps its own import
  history (D6.1 inspector), and undoing an import is there.
  - **Inspector**: the change as title, the count as display figure, "transactions
    changed · 9 Nov at 14:05". "What changed": one 24px row per transaction (merchant
    `text-secondary`; "Uncategorised → Shopping", the new value in `text`), a caption,
    outline "Undo this change…" and "Show in Transactions".
- **Menu** (new pattern, first drawn here; states board). A list of choices that opens
  from a button: `float` fill, 1px `border`, `radius-surface`, the Toolbar shadow, 4px
  padding, `--container-search-field` (240px) wide. Items are 28px list rows (radius 6,
  8px inline padding, 12px icon, 8px gap, label 12/16 `text`); hover and keyboard focus
  take `hover`. An unavailable item is at `opacity-disabled` with its reason as a caption
  on the right ("In use", "Not with Category"). A hairline with 4px above and below
  separates groups. The button that opened it shows `control-active` while it's open.
  The filter chips' menus (D3) should use it too.
- **States board** (`HFH-0`):
  - **Delete rule dialog** (`--container-dialog`): "Delete the rule “Merchant contains
    UBER”?", a body saying its 31 transactions keep Transport and new Uber rows stay
    uncategorised unless another rule matches; Cancel and **destructive solid** "Delete
    rule" (deleting can't be undone; no transaction changes, so there's no change batch).
  - **Undo a change dialog**: "Undo “Applied rule TFL TRAVEL → Transport”?", a body
    counting what goes back (56 of 58) and what stays because it changed since (2);
    Cancel and neutral primary "Undo change" (undo is itself reversible by applying
    again).
  - **Undo bar** after Apply (above).
  - **No rules yet** (canvas, centred): the → icon in a 36px `field` well, "No rules yet",
    and a caption pointing to D6.3's prompt and "New rule".
  - **Several actions** on one rule (Category, Merchant, Tag, My share) and the caption on
    their limits; the **add-action menu** open, with Category "In use" and Split "Not with
    Category"; a **pattern that can't be used** (field in a 1px `danger` border with the
    `alert` icon, a `danger` caption naming the fault, "A bracket is opened but never
    closed."; the rule can't be saved until it's valid); **never reached** (above).
- **Data-model questions: decided** (user, 2026-11-10; `ARCHITECTURE.md` → "Decision
  record: rules"). Each went with the recommendation below, which the designs already
  assume.
  1. *Which rule wins?* Recommended: field by field. The first matching rule in priority
     order sets a field; a later rule can still set a different field (a tag rule below
     a category rule both apply). A rule is "never reached" when every transaction it
     matches already has all its fields set by rules above.
  2. *Where does a new rule go?* Recommended: first. A rule from an edit or from the Inbox
     is usually narrower than the rules it corrects. The Position select moves it.
  3. *Does a rule overwrite a category the user set?* Recommended: never on import, and
     on Apply only for rows the user includes with "Change these too" (rows with
     `category_source = user`). Matches D6.6's question 1 for agent rules.
  4. *Do rule edits re-run on history?* Recommended: no. Rules run on import; history
     changes only through Apply, one `change_batch` per apply.
  5. *Which merchant name wins when several match?* Recommended: Is, then the longest
     Starts with, then Pattern in list order. Names apply at once with no batch
     (above).
  6. *Undo after later edits.* Recommended: undo restores a field only where it still
     holds the batch's new value; rows changed since keep the later value, and the
     dialog counts them.
  7. *Deleting a rule.* Recommended: its transactions keep their values; `rule_id` is
     cleared, the inspector's "Set by" reads "A deleted rule", and a later rule or Apply
     may change them.
  8. *Do locked periods cover categories?* Recommended: no. `reconciled_through` guards
     balances, and categories, tags, notes and names don't change one, so Apply and
     Undo in a locked window aren't flagged. An Exclude action with reason Duplicate
     does change a balance and is flagged like any other change there.
- **Not drawn:** the Split action's template editor (D6.3's split lines without amounts,
  in percentages), the Exclude action's reason row, the Amount condition's Over / Under / Between fields, New rule from
  scratch (the inspector with empty When and Then), and the eighth tab on the artboards
  drawn before D6.7.

**Budgets: groups, rollover, pace and upcoming** — drawn in D6.10 (M4.8, M4.9). Two
1440×900 artboards built from the D6.1 Budgets screen (`6OE-0`), plus a states board.
Every value is a bound token. Typed values are as on D6.1: 1px borders, the Dock shadow,
SVG icon paths, and data-driven bar, line and point positions. No new tokens (hash still
`6a126241`). Fixture data continues D6.1: Monday 9 Nov 2026 (day 9 of 30) with the same
November spending, plus two categories, Insurance and Gifts. They have outline markers,
because only the six biggest categories by spending get a colour.
- **Where Upcoming lives.** The view tabs (eight with D6.7's Rules) have no room for
  another. Also, "what's left to spend" and "what's due before payday" are one question.
  So the Budgets tab has two halves, switched by an inspector-size segmented control
  (Budgets | Upcoming, `column-amount` × 2) at the right end of the canvas header. Budgets
  keeps the month switcher. Upcoming has no month, because it looks forward from today:
  its header is "Next 30 days" in title type, with the account and today's date as a
  caption. Overview's "Recurring · Next 30 days" is unchanged (detected series, all
  accounts).
- **Budgets · groups, rollover and pace** (`CXU-0`).
  - **Figures.** Left to spend (display figure) is the Flex group's LEFT, captioned
    "£19.23 a day for 21 days, in Flex". Spent covers every group, captioned "Flex 66%
    used, 30% into the month". **Set aside** is the non-monthly LEFT total, captioned
    "For Insurance and Gifts". The budgeted total moves to the Total row.
  - **Lanes:** CATEGORY (180) · bar (flex) · SPENT · BUDGET · ROLLOVER · LEFT (96 each).
    LEFT = BUDGET + ROLLOVER − SPENT, and the bar ends at BUDGET + ROLLOVER. Rollover is
    signed and `text-muted` like Budget, because it's an input, not a result. It reads
    "—" when off. Only LEFT takes `warning` or `danger`.
  - **Groups are table sections**, as in the Inbox: Fixed ("Bills on set dates"),
    Non-monthly ("Yearly costs, set aside each month"), then Flex. Each has a 28px header
    with 8px top padding: the group name (11/14 600 `text-secondary`), its caption in the
    bar lane, and the group's subtotals in mono 11/14 `text-muted`. Hairlines divide the
    groups. A group of one shows no subtotals, since they would repeat its row. The Total
    row sits under a hairline: £1,258.61 · £1,640.50 · +£897.30 · £1,279.19.
  - **Non-monthly rows.** BUDGET is the month's twelfth, ROLLOVER is what was set aside
    before, and LEFT is what's set aside now. The bar shows the money set aside against
    the yearly amount. It's filled `text-muted`, because it's savings, not spending. The
    next due date sits at the right end of the category lane (mono 10/12 `text-muted`), so
    every bar still ends on the same line.
  - **Pace line.** One 1px dashed `text-muted` line runs down through the Flex bars, at
    the share of the month gone (30% on 9 Nov). The Flex header labels it "Today · 30%"
    (mono 10/12). A bar past the line is ahead of pace. Bars aren't recoloured for pace:
    `warning` and `danger` keep meaning 85% and over budget. Fixed and non-monthly rows
    have no pace line, because bills land on dates.
  - **Inspector** (Groceries, rollover on). The header reads "of £324.60 · 65% used with
    21 days to go". Below it:
    - field rows: Budget (`£`, "/ month"), Group (a select: Fixed, Non-monthly, Flex) and
      Rollover ("Carry over what's left" and a toggle), then a caption;
    - the month's sum: November budget, Left from October, and To spend in November (600,
      under a hairline);
    - **Pace**, titled with "£115.02 ahead of pace". A 96px chart shows this month's
      cumulative spending as a 1.5px `text` line ending in the today point. The even-spend
      line runs 1px dashed `text-muted` from £0 to the month's amount, which is labelled
      top-left. Today is a 1px `border-strong` vertical. X labels: 1 Nov, "Today, 9 Nov"
      (`text`), 30 Nov. A caption gives the pace amount and the daily amount that stays
      within budget;
    - "Last 6 months" as on D6.1, with the dashed line at the monthly budget.
- **Budgets · upcoming** (`GD5-0`). Current account is selected in the left panel; the
  forecast follows that selection.
  - **Figures.** Lowest before payday (display figure, `danger` when negative), captioned
    "On 27 Nov, the day before Salary". Due before payday, captioned "5 payments, 1
    missed". Payday "+£3,450.00", captioned "Salary on 28 Nov, in 19 days". Payday is the
    account's next scheduled income.
  - **Chart** "Balance forecast", with the caption "End of each day · 1 Nov – 9 Dec" and a
    legend: Balance (solid) and Forecast (dashed). It has a 160px plot, and the y axis
    runs from £0 to the next round step in five intervals. Actual balances up to today are
    a 1.5px `text` line; the forecast is a 1.5px dashed (4 3) `text-secondary` line. One
    area runs to £0 at `opacity-area-fill`. Marks on the chart:
    - today: a 1px `border-strong` vertical labelled "Today" (mono micro);
    - the lowest point before payday: a 6px point with a `canvas` ring, and "Lowest ·
      £51.03" on a `canvas` patch with a 1px `text-muted` leader;
    - income: a muted label at its step ("Salary +£3,450.00");
    - the selected payment: a point with the molten selection label ("15 Nov ·
      −£164.00").

    X labels mark today (`text`), the selected date, payday and the weeks between.
  - **Scheduled payments.** The title has the caption "Only schedules you've confirmed
    count in the forecast" and an outline compact "Review 2 found" on the right. Table
    lanes: DATE (48, mono) · PAYMENT (marker + name, flex) · REPEATS (96) · BALANCE (96,
    the end-of-day forecast in mono `text-muted`, on a day's last payment only) · AMOUNT
    (96, signed). Two sections: "Before payday", with its total, and "From payday".
    - A missed payment reads "Missed" in `warning` under REPEATS, with a 6px `warning`
      dot before the amount.
    - A price rise is shown as on Overview.
    - Transfers are muted with ⇄, and income has no marker.
    - The selected row is `molten-soft`.
  - **Schedule inspector.** Header: the name, the signed amount as display figure, and
    "Monthly on the 15th · next on 15 Nov". Field rows: Account, Category, Amount (`£`,
    "out"), Repeats, Next, and Match. Match is the schedule's tolerance: ±£ in a field, and
    ± days in a 96px field. A caption says that a schedule never adds a transaction.
    "Matched payments" lists each match (check, date, "On the day" or "1 day late",
    amount) under the heading "7 since 15 Apr", with a caption saying when the user
    confirmed it. Then outline compact "Pause" and destructive outline compact "End
    schedule…", with a caption.
  - **The fixture's arithmetic.** Each day's balance is the last known balance, plus the
    schedule payments to date, minus the Flex budget left (£403.78) spread evenly over
    the rest of the month. December then spends its whole Flex budget the same way. All
    in integer pennies, with the remainder spread so the days sum exactly.
- **States board** (`HNB-0`, 1440 wide).
  - **Non-monthly inspector** (Insurance). Budget is £726.00 / year and Group is
    Non-monthly. The Rollover toggle is on and disabled ("Always, until it's spent"), with
    the set-aside sum below. "From yearly schedules" lists Admiral (14 Mar, −£540.00) and
    Aviva Home (2 Jul, −£186.00), with a caption: the yearly amount started as their
    total. "Enough by 14 Mar" closes it; when the money won't be there in time, it reads
    "£48.50 short by 14 Mar" with a `warning` icon.
  - **Rollover rows**: carried in, overspending carried in, and off, with notes.
  - **Schedule dialog** (`--container-dialog`), for a series Coffer found. The body says
    what was found and that no transaction is added. Field rows: Account, Category,
    Amount, Repeats, Next, Match. A yearly schedule in a category with a non-monthly
    budget adds a "Set aside" toggle, which adds it to the yearly amount. Cancel /
    "Add schedule" (neutral primary).
  - **"Review 2 found" popover** (440px): name, cadence and count found, amount, outline
    compact "Schedule…", and an ✕ icon button for "Not recurring".
  - **Missed payment inspector**: a `warning-soft` block, "Not in your imports yet", with
    outline compact "Match a transaction…" and "Skip this one".
  - **Overdrawn forecast** (Joint account). The display figure is `danger`, and a second
    figure counts the days overdrawn. The line below £0 and its area are `danger`.
  - **Empty Upcoming**: a `schedule` icon well, "No scheduled payments yet", and outline
    "Review 2 found".
- **Decided while drawing (review):**
  - Upcoming is the second half of the Budgets tab, not another tab.
  - Left to spend counts Flex only; non-monthly money shows as Set aside.
  - Rollover is a column, signed and muted, and only LEFT carries status colour.
  - The pace line is one line across the Flex bars, and bars aren't coloured by pace.
  - A missed payment stays in the forecast, counted from tomorrow, until it's matched or
    skipped.
  - A series Coffer detects stays behind "Review N found" and out of the forecast until
    the user schedules it, in Coffer's own UI (ARCHITECTURE "Schedules and forecast").
- **Data-model questions: decided** (user, 2026-11-10; `ARCHITECTURE.md` → "Decision
  record: budgets and the forecast"). Each went with the recommendation below, which the
  designs already assume.
  1. *Which account does Flex spending leave from?* Recommended: split each Flex
     budget's remainder across accounts by where that category was spent over the last 3
     months. An account with no Flex spending (Savings, or Joint in the fixture) gets none.
  2. *When is a payment missed?* Recommended: when its date plus tolerance has passed with
     no match and the latest import covers that date. Before an import covers it, the
     payment is just not imported yet. A missed payment stays in the forecast from
     tomorrow until it's matched or skipped.
  3. *What does rollover carry?* Recommended: both ways, so unspent money adds and
     overspending subtracts, and it accumulates month to month. Derive it from history
     rather than storing it, so a past edit flows into later months. Turning rollover off
     starts fresh from the next month. Fixed budgets don't offer rollover, and non-monthly
     ones always roll over.
  4. *Pace.* Recommended for v1: linear by day of the month, rather than weighted by past
     months' daily pattern.
  5. *Seeding non-monthly budgets.* Recommended: set the yearly amount once, from the
     category's yearly schedules, and let the user change it after that. A new yearly
     schedule asks before adding itself (the dialog's toggle). Set-aside money accrues
     from the month the budget was created, starting at £0 unless the user enters what's
     already saved.
  6. *No scheduled income.* Recommended: when an account has none, "Lowest before payday"
     becomes "Lowest in the next 30 days".
- **Not drawn:** Upcoming with "All accounts" selected (recommended: the sum of the
  accounts that have schedules, with each account's lowest point in the inspector), a
  paused schedule's row, and the Group select's menu.

**Not drawn in D6.1:** empty states for these screens (empty Inbox, no budgets, no
reviews), the Approved / Rejected inbox lists, Save as preset (M3.5), and the screens
D6.4–D6.11 add to.

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
   the 1.5px focus ring, the Toolbar and Dock shadows, SVG icon paths. Remaining screens
   ✅ done (D6.1, artboards `564-0`, `5ZQ-0`, `6OE-0`, `781-0`, `7LH-0`, `7YM-0` and the
   states boards `6J4-0`, `8D8-0`): built from the Transactions and Overview shells and
   Components clones, every value a bound token, two new tokens (`--container-dialog-wide`,
   `--container-reading`). Typed values: 1px borders, the Dock shadow, SVG icon paths and
   chart points, data-driven bar widths and heights.
5. Export `design/tokens.json` and `design/tokens.css` from the Coffer file. ✅ Done (D1.5):
   122 tokens, content hash `572f3e11`. Re-exported in D3.1: 123 tokens, content hash
   `86e4aeb1`. Re-exported in D6.3: 124 tokens, content hash `b442a048`. Re-exported in D6.1: 126 tokens, content hash
   `6a126241`. Re-export
   whenever a token changes (rule 4).

## Implementing in GPUI

- **Fonts:** bundle Inter and Geist Mono with the app (both SIL OFL 1.1, compatible with
  `MIT OR Apache-2.0` distribution; include their licence files). Fraunces is not used.
  Implemented: the files (Inter 4.1, Geist 1.9.2) and their `OFL.txt` are in
  `assets/fonts/`; `crates/app/src/theme/fonts.rs` compiles them into the binary, registers them at
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
       `stock_focus_style_is_off_and_disabled_fades_as_a_group` (`crates/app/src/ui/button.rs`) and
       `faded_colours_are_the_css_group_composite` (`crates/app/src/ui.rs`).
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
- **Fidelity findings (D5).** The Transactions screen (`crates/app/src/transactions.rs`, fixture data)
  rendered headlessly by `crates/app/examples/snapshot.rs` (GPUI's headless platform with the real
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
  Implemented in `crates/app/src/theme/tokens.rs`. Its drift test maps
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
- **gpui-component theme:** mapped onto the tokens by `crates/app/src/theme/component.rs` (D2.4);
  `coffer_app::theme::install` runs it at startup, then loads the fonts. It switches the
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
  `FocusRingExt::focus_visible_ring` (`crates/app/src/theme/focus.rs`): a `size::FOCUS_RING` (1.5px)
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
  the two. Coffer's buttons (`crates/app/src/ui/button.rs`, see "Stock components") close the stock
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
  | Button: neutral primary, outline, outline compact, destructive outline, destructive solid, icon (28px; 36px dock) | Stock, wrapped: **built**, `coffer_app::ui::button` (`Button`, `IconButton`) | The stock button keeps pointer and Enter/Space activation, Tab order, disabled handling and the accessible name (the label; an icon button takes one). Height, padding, radius, border, fill and label colour are set per instance from the tokens. The label is a child with its type role, because the stock label's size follows the stock `Size` (12.25px at the rem base). Hover and pressed go through `ButtonVariant::Custom` with token colours, because the stock variants also set the border on hover and pressed (the outline's `border-strong` would turn `hairline`, the destructive outline's `danger` a 40% tint). Disabled is the resting look faded to `opacity-disabled` as one group: element opacity where nothing sits under the label, opaque composites over `Button::with_surface` where a fill does (see "Rendering findings (D2.6)" → "Closed"). Focus: see "Focus ring". The active icon button is built (`IconButton::active`, M3.1: `control-active` fill and a `text` icon at rest, hovered and pressed; the title bar's Settings button, Paper `BAJ-0`); the dock's white active icon isn't. Not built: tooltips (Coffer's tooltip is custom and not built yet), and hover/pressed transitions (`motion-fast`; the stock button changes instantly). Read, not specified, and accepted in the D2.5 visual review: icon colour `text-muted` (the token for default icons) and icon size `icon-md` (14px, from the table's 12–16px) |
  | Agent outline | Stock, wrapped: **built**, `ButtonVariant::AgentOutline` | As above: `moldavite-border-strong` border, `moldavite-text` label, hover `moldavite-surface-raised`, pressed `moldavite-well`. It confirms the `Custom` path: its resting fill (20% of its colour) is replaced per instance, and the hover and pressed fills still apply over that |
  | Agent primary, agent ghost | Stock, wrapped; not built | Same `Custom` path; the solid agent primary sets `.bg(moldavite)` per instance. Needs Paper first: the table gives no horizontal padding for either. Agent primary's optional trailing kbd needs the Kbd wrapper; agent ghost needs a leading icon (sparkle) |
  | Input, field, select, command bar | Stock, wrapped | In dark mode the stock fill is the *border* colour (`input`) at 30% alpha, so no theme value gives `field` fill with a `hairline` border; the wrapper sets `.bg(field)`. Height is 28px from the rem base |
  | Data table | Not used for Transactions (D5): a GPUI `list` with Coffer's rows, which interleaves day headers of their own height | Colours map (no stripes, no row dividers, `hover`/`molten-soft` rows, `canvas` sticky header) and row height takes `Size::Size(size::CONTROL)`. These colours apply if a stock data table is used elsewhere. The in-place category select is the category menu (M3.3, "Built in M3.3") |
  | Dialog | **Custom: built**, `coffer_app::ui::dialog` (M3.2), on the unstyled `gpui-base` dialog host | The host keeps the focus trap and Escape; Coffer draws the spec surface (`float`, 1px `border`, `radius-lg`, Dock shadow `shadow::DOCK`, 16px padding and gaps, footer right-aligned) and the provisional backdrop. The stock `gpui-component` dialog sets its own shadow after the caller's style, so it can't carry the Dock shadow. Its owner keeps whether it's open. Cancel is the 30px `ButtonVariant::Secondary` (Paper `8JN-0`) |
  | Select and its menu | **Custom: built**, `coffer_app::ui::menu` (M3.2), on the unstyled `gpui-base` select root | The root keeps the combobox role, opening with Enter, Space and the arrows, Escape, and moving focus between trigger and menu; Coffer draws the closed field (`ui::field::SelectField`) and the Kind menu (`FHZ-0`): `float`, 1px `border`, `radius-lg`, Toolbar shadow, 4px padding, caps group labels (`typography::MENU_LABEL`), 28px options with a 16px icon slot and the `Glyph::Chosen` tick. Drawn `deferred` below the field so a scrolling inspector or a dialog doesn't clip it. Options are list-box options for assistive technology. M3.3 adds a category marker in the icon slot, the filter field, and `MenuList` for menus opened from a chip, a table row or the bulk-action bar, which handles ↑↓, Enter and Escape itself |
  | Popover, menu (other than a select's), sheet, notification | Stock; on the showcase's stock board, unwrapped | Fill and text map (`float`, `text`). Popovers use a hard-coded shadcn ring and shadow at `radius`; Coffer's floating spec is a 1px `border`, `radius-lg` and the toolbar shadow, set per instance |
  | Title bar | Stock; on the showcase's stock board, unwrapped | Keeps platform window controls and dragging. Height is a constant 34px; Coffer's is 40px |
  | List row | Stock, wrapped: **built**, `coffer_app::ui::list_row` (`ListRow`) | Wraps the stock `ListItem` (pointer, accessible row). 28px, radius 6, 8px inline padding, 16px icon slot, `text-secondary` label lifting to `text` on hover, `molten-soft` fill with white label when selected, trailing mono count. The owning list holds focus and passes `focus_visible`; the row draws the `Inset` ring on a wrapper, because `ListItem` lays children out in an inner box and scrolling lists clip an outside ring. Nesting is a 20px left indent on the wrapper. No pressed look (rows select on click) |
  | Transaction row, table header, day header | **Custom: built**, `coffer_app::ui::transaction_row` | Columns from `column-category`/`column-account`/`column-amount`; amounts in `mono-amount`, sign never carried by colour. Category cell: assigned (marker), uncategorised (dashed marker), suggested (sparkle, `moldavite-text`), transfer (row dimmed, transfer glyph). Selection and focus as the list row. D5 kept these rows, in a virtualised GPUI `list` (`crates/app/src/transactions.rs`) |
  | Status bar, tree row | Stock; not yet compared | Colours map (`float` status bar with `hairline`; `hover` and `molten-soft` rows) |
  | Scrollbar | Stock | Colours provisional (above) |
  | Kbd | Stock, wrapped | Fill and font differ (`hover`, Geist Mono 11/14, 18px); set per instance |
  | Toggle | **Custom** | Stock `Switch` (small) has the right geometry (28×16, 12px knob, 2px inset, pill), but one knob colour for both states; Coffer's knob is `text-muted` off and `chrome` on |
  | Segmented control | **Custom** | Stock segmented tabs are 24 or 32px (segments 18/24); Coffer needs 30px panel / 28px inspector with 2px inset, and an inset highlight on the active segment |
  | View tabs (title bar) | **Custom: built**, `coffer_app::ui::title_bar` (`ViewTab`), on the unstyled `gpui-base` `Tab` in a `Tabs` list (M3.1) | Stock tab heights are 20/24/32/36px; Coffer's tab is 28px with a `hover` active fill and hairline separators. The base tab gives the tab role, the selected state and the position in the list, and stops the press from reaching the title bar's drag. Tabs aren't Tab stops (the base primitive has no roving focus yet); the views have ⌘1–⌘8 |
  | Underline tabs | Stock; on the showcase's stock board, unwrapped | Stock underline tab heights are 26/30/36/44px; the spec gives no height |
  | Badge | **Custom: built**, `coffer_app::ui::badge` | Stock `Badge` is a dot or count overlaid on another element; Coffer's badge is an inline 16px count pill |
  | Tooltip | **Custom** | Stock tooltip uses the popover fill (`float`) and `text_sm`; Coffer's is `hover` fill, `tooltip-border`, tooltip shadow, title + kbd + body. Rendered through GPUI's `.tooltip(...)` |
  | Progress bar | **Custom** | Stock heights are 4/6/8/10px; Coffer's is 3px, radius 2 |
  | Agent card | **Custom: built**, `coffer_app::ui::agent_card` | Change rows are 26px tab stops with a `moldavite-well` glyph well and the detail in `moldavite-text-muted`; on hover or keyboard focus the row fills `moldavite-surface-raised` and swaps its age for approve/reject icon buttons, so both are reachable by keyboard. "Approve all" is a full-width agent outline (`Button::with_full_width`) |
  | Chip | **Custom: built**, `coffer_app::ui::button::Chip` | Pill with optional mono count |
  | Card, status dot, empty state, bulk-action bar, section label row, inspector field row | **Custom** | No stock component with the same structure; built from `div` and the tokens. Built: bulk-action bar, section label row, inspector field row, and the empty state (`coffer_app::ui::empty_state`, M3.1, matched against `13L-0` at 2x; its dashes are GPUI's, see "1px hairlines"). Not built yet: card, status dot |
  | Charts | **Custom** (decided in D4.1), on the `gpui_component::plot` primitives | Stock `BarChart` plots one value per band, so grouped spending/income bars would need two charts stacked; `LineChart` hard-codes a 2px stroke (spec 1.5px) and has no halo option; neither has a selected-band state for the molten selection overlay. Reuse the plot scales and tooltip; draw bars, the haloed line and the selection with the tokens |
- **Showcase** (`coffer --showcase`, `crates/app/src/showcase.rs`): three boards. *Design tokens*
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
