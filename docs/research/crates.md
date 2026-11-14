# Crates for the remaining roadmap: research notes

Research date: 2026-11-13. Input for M3–M5 and Later; nothing here is a decision until it
is recorded in [`ARCHITECTURE.md`](../ARCHITECTURE.md) → "Dependencies". Roadmap items are
referenced by their [`ROADMAP.md`](../ROADMAP.md) numbers; the dependency rules are those in
[`AGENTS.md`](../../AGENTS.md) (few dependencies, each justified, licences compatible with
`MIT OR Apache-2.0`).

## Method and limits

- **Local checks** (most reliable): `cargo tree -i <crate> -e normal,build --target all`
  for what is already in the lockfile, and the sources of gpui-component / gpui-kit 0.9.0,
  gpui-pre 0.3.11, rmcp 3.9.0 and libsqlite3-sys 0.38.2 in the Cargo registry.
- **Web:** Firecrawl search and scrape for the ECB data formats, packaging tools, PDF
  crates, MCP streamable HTTP and keyring (about 10 requests).
- **Versions and licences:** the crates.io API (`/api/v1/crates/<name>`), read 2026-11-13.
  Firecrawl's package-registry provider returned 502 for every crate, so it wasn't used.
- Not covered: build-time or binary-size measurements, a hands-on trial of any crate, the
  feature flags of `pdf_oxide`, and the licence of `webpki-roots` (check before adding
  `ureq`).

## Summary

- **M3 needs no new crates.** gpui-kit 0.9, GPUI and the bundled SQLite already provide
  the components, file dialogs and full-text search.
- **M4's only new runtime dependency is `ureq`**, for the ECB fetch. Charts come from
  gpui-component; the safe markdown subset can use `markdown`, already in the tree.
- **M5 adds tools, not dependencies:** `cargo-deny`, `cargo-about` and a packager.
- **Later:** own parsers win for QIF and CAMT.053; `mt940`, `pdf_oxide`, `keyring` and
  `zip` are the realistic picks for MT940, PDF, the SQLCipher key and attachment bundles.

## M3 — GUI foundation

| Need | Covered by | Notes |
|---|---|---|
| Virtualized transactions table (M3.3) | gpui-component `table`, `virtual_list.rs` | |
| Date picker, select, sheet, sidebar, notifications | gpui-component `time/date_picker.rs`, `time/calendar.rs`, `select.rs`, `sheet.rs`, `sidebar`, `notification.rs` | Restyle to `DESIGN.md` tokens |
| Pick a file to import (M3.4), choose where to save an export (M3.16) | GPUI `App::prompt_for_paths`, `App::prompt_for_new_path` (`gpui-pre/src/app.rs:2039`, `:2052`) | Native dialogs; on Linux through the XDG portal (`ashpd`, already in the tree). `rfd` isn't needed |
| Show a backup in the file manager (M3.16) | GPUI `App::reveal_path` (`app.rs:2061`) | |
| Search transactions (M3.3) | SQLite FTS5 | The bundled `libsqlite3-sys` 0.38.2 is built with `-DSQLITE_ENABLE_FTS5` (`build.rs:159`). Needs a migration, not a crate |
| Window state (M3.1) | settings table + `serde_json` | Both already in core |

Not recommended: `nucleo-matcher` for fuzzy search (MPL-2.0, last release 2024-02, and
FTS5 plus substring matching covers M3.3).

## M4 — Analytics

### Charts (M4.1–M4.5, L.8)

gpui-component 0.9 has `chart/` with area, bar, line, pie, radar, candlestick and **sankey**
charts, plus a `plot` module with tooltips. L.8's Sankey view doesn't need a crate. Charts
must still follow the monochrome-first chart tokens in `DESIGN.md`.

### ECB rate fetch (M4.6)

**HTTP client: `ureq` 3.5.2** (MIT OR Apache-2.0, updated 2026-11-12). Blocking, so core
needs no async runtime; rustls with the `ring` provider by default; gzip on by default.

- rustls isn't in the native build today: `cargo tree --target all` finds it only under
  `gpui-pre-reqwest`, which `gpui-kit-assets` uses for wasm only. `ureq` adds rustls, ring
  and webpki-roots (or platform roots via `rustls-platform-verifier`). Check the
  webpki-roots licence with `cargo-deny` before adding it.
- 3.5.2 was a day old at research time; pinning a release that has been out a while is
  safer.
- `reqwest` isn't worth it: it brings tokio and hyper into the app path for one request.

**Format: the ECB Data Portal's CSV, not the eurofxref XML.** The Data Portal API
(`https://data-api.ecb.europa.eu/service/data/<flow>/<key>`) accepts
`format=csvdata` (also `jsondata`, `structurespecificdata`, `genericdata`), with
`startPeriod`, `endPeriod`, `updatedAfter` and `lastNObservations`. Daily euro reference
rates for every currency:

```
https://data-api.ecb.europa.eu/service/data/EXR/D..EUR.SP00.A?format=csvdata&startPeriod=2026-01-01
```

- The `csv` crate is already a dependency, so this drops the planned `roxmltree`.
- `startPeriod` / `updatedAfter` make the fetch incremental: only rates newer than the
  cache.
- The older downloads on ecb.europa.eu are `eurofxref-daily.xml`, `eurofxref-hist.xml`,
  `eurofxref-hist-90d.xml` (XML) and `eurofxref.zip` / `eurofxref-hist.zip` (CSV inside a
  zip), so full history there needs an XML parser or `zip`.
- Rates arrive as decimal strings; parse them straight into the `i128` fixed point the
  "Money" section already specifies, never through `f64`.

Source: [ECB Data Portal API, Data](https://data.ecb.europa.eu/help/api/data);
[euro reference rates downloads](https://www.ecb.europa.eu/stats/policy_and_exchange_rates/euro_reference_exchange_rates/html/index.en.html).

### Safe report rendering (M4.10)

`markdown` 1.0.0 (markdown-rs, MIT) is already in the tree via `gpui-base`. Parse to its
mdast and render an allow-listed subset (headings, paragraphs, emphasis, lists, tables,
code) into GPUI elements; show links as plain text and drop images.

Don't render reports through gpui-component's `TextView` as-is:

- Its default image source turns any image URI into a local `PathBuf`
  (`gpui-component/src/text/mod.rs:421`), so an AI-written `![](/home/…/file)` would read a
  local file.
- Remote images can't load today only because GPUI's default HTTP client is
  `NullHttpClient` (`gpui-pre/src/app.rs:3613`); that is a side effect, not a guarantee.

`pulldown-cmark` 0.13.6 (MIT) would work too, but a second markdown parser isn't needed.

### Forecasts and budgets (M4.8, M4.9)

`chrono` is enough for schedules, periods and rollover. No recurrence-rule crate is needed.

## M5 — Release

These are tools run in CI or the pre-push hook, not dependencies of any crate.

| Tool | Version, licence | Use |
|---|---|---|
| `cargo-deny` | 0.20.2, MIT OR Apache-2.0 | Licence allow-list, RustSec advisories, duplicate versions, banned crates. Automates the licence rule; a good addition to `lefthook.yml` `pre-push` |
| `cargo-about` | 0.9.3, MIT OR Apache-2.0 | Third-party licence notice to ship in release artifacts (M5.6) |
| `cargo-packager` | 0.11.8, Apache-2.0 OR MIT | `.app`, `.dmg` and AppImage from one config. Last release 2025-11-27 |
| `cargo-bundle` | 0.12.0, MIT OR Apache-2.0 | Alternative for `.app` / `.dmg`; more recently released (2026-09-20) |
| `cargo-dist` | 0.34.0, MIT OR Apache-2.0 | Tarballs and GitHub releases; no `.dmg` or AppImage |

The Nix flake (M5.6) needs a Nix builder such as crane, not a crate. Beancount and hledger
export (M5.8) is plain text; no crate.

### Test tooling (dev-dependencies)

| Crate | Version, licence | Use |
|---|---|---|
| `proptest` | 1.12.0, MIT OR Apache-2.0 | Money parsing, split sums, FX rounding, fingerprints, dedupe |
| `insta` | 1.49.0, Apache-2.0 | Snapshots of CSV/JSON exports, Beancount/hledger journals (M5.8) and MCP results |

## Later

| Item | Pick | Version, licence | Notes |
|---|---|---|---|
| L.2 QIF | own parser | — | `qif` 0.1.0 (MIT) has 64 downloads in 90 days and one release |
| L.3 CAMT.053 | own mapping on `roxmltree` | 0.21.2, MIT OR Apache-2.0 | `camt053` 0.4.2 (MIT) has 29 downloads in total and also converts to MT940, which Coffer doesn't need; `mx20022` 0.3.2 and `iso20022-rs` 0.2.0 (Apache-2.0) are similarly unused. A statement only needs a few elements (`Bal`, `Ntry`, `Amt`, `CdtDbtInd`, `BookgDt`, `AcctSvcrRef`), which suits a read-only DOM. `quick-xml` 0.42.0 (MIT) is the streaming alternative |
| L.4 MT940 | `mt940` | 1.1.1, MIT | Maintained (2026-09-23). A strict parser: test it against fixtures with the quirks real banks produce before relying on it. `swift-mt-message` 3.1.5 (Apache-2.0) covers far more than statements |
| L.5 PDF | `pdf_oxide` | 0.3.78, MIT OR Apache-2.0 | Character and span positions with bounding boxes, which per-layout templates need. Large feature surface (OCR, conversions): check its features and keep only text extraction. `pdf-extract` 0.12.1 (MIT) is the simpler fallback; `lopdf` 0.47.0 (MIT) has no text extraction of its own |
| L.6 SQLCipher | rusqlite `bundled-sqlcipher-vendored-openssl`; `keyring` | 4.2.0, MIT OR Apache-2.0 | keyring 4 is `keyring-core` plus a store crate per platform: `apple-native-keyring-store`, `zbus-secret-service-keyring-store` or `dbus-secret-service-keyring-store`, `linux-keyutils-keyring-store`, `windows-native-keyring-store`. The zbus store reuses zbus, already in the tree via `ashpd`. `zeroize` 1.9 is already in the tree |
| L.7 Streamable-HTTP MCP | rmcp `transport-streamable-http-server` | rmcp 3.9.0 | Plus `axum` / tower. Bind `127.0.0.1` only. Not outbound, but it opens a listener, so it needs its own decision record |
| L.9 UI translations | `rust-i18n` | 4.2.4 | Already in the tree via gpui-component, so component strings and Coffer's share one system |
| L.11 Attachments | `sha2` (in tree); `zip` | 9.5.1, MIT | Content hash with the existing `sha2`, so `blake3` isn't needed. Turn `zip`'s default features off to avoid C codecs (bzip2, zstd) and AES |
| L.14 Investments | own fixed-point quantity type | — | `rust_decimal` 1.43.0 is tempting for unit quantities, but money stays integer minor units |
| L.20 What-if | `rand_chacha` (+ `rand`) | 0.10, MIT OR Apache-2.0 | Seeded ChaCha streams are reproducible across platforms. Neither is in the tree today |
| L.29 Search syntax | `winnow`, or hand-written | 1.0.5, MIT | Not in the tree today (`toml` 1.x doesn't use it) |
| L.30 Benchmark | `criterion`; `fake` | 0.8.2; 5.1.0, both MIT OR Apache-2.0 | `divan` 0.1.21 is nicer to use but hasn't had a release since 2025-04. `fake` generates the 100k synthetic rows |

## Avoid

- `rust_decimal` (or any float or decimal type) for stored amounts: money is integer minor
  units.
- `chardetng` or other encoding detection: a file without a known encoding is refused,
  never guessed.
- `reqwest` for the ECB fetch: async stack for one request.
- `nucleo-matcher`: MPL-2.0.
- GPL crates, as already listed in `ARCHITECTURE.md` (`ofxy`, `tree_magic_mini`'s GPL data
  feature).

## Already in the tree via GPUI (useful to know)

`markdown` 1.0.0, `rust-i18n` 4.2.4, `notify` 8.2.2 (file watching), `zeroize` 1.9.1,
`oo7` 0.6.0 and `ashpd` 0.13.13 (Secret Service and XDG portals), `quick-xml` 0.41 (only as
a proc-macro dependency of `wayland-scanner`, so not usable at runtime for free).
