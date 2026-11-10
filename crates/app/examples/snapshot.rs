//! Renders Coffer screens offscreen to PNG, for comparing them against the
//! Paper artboards (`docs/ROADMAP.md` D5.1).
//!
//! ```sh
//! cargo run --example snapshot -- <out-dir>
//! ```
//!
//! Uses GPUI's headless platform with the real text system (cosmic-text on
//! Linux, so the bundled fonts shape as in the app) and the wgpu headless
//! renderer, so no window or display is needed. Each screen is written at
//! 1x and 2x:
//!
//! - `transactions`: Paper `18E-0`, one AI-suggested transaction selected;
//! - `transactions-multi`: Paper `1NP-0`, three uncategorised rows selected;
//! - `showcase`: the component showcase.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use gpui_kit::{AnyView, AppContext as _, HeadlessAppContext, size};

use coffer_app::showcase::Showcase;
use coffer_app::transactions::TransactionsView;

const WIDTH: f32 = 1440.;
const HEIGHT: f32 = 900.;

fn main() -> Result<()> {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "snapshots".into()),
    );
    std::fs::create_dir_all(&out)?;

    let text_system = gpui_kit::platform::current_platform(true).text_system();
    let mut cx = HeadlessAppContext::with_platform(
        text_system,
        Arc::new(gpui_kit::assets::Assets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(|cx| {
        gpui_kit::init(cx);
        coffer_app::theme::install(cx).expect("theme installs");
        coffer_app::showcase::init(cx);
        coffer_app::transactions::init(cx);
    });

    snapshot(&mut cx, &out, "transactions", |window, cx| {
        cx.new(|cx| TransactionsView::new(window, cx)).into()
    })?;
    snapshot(&mut cx, &out, "transactions-multi", |window, cx| {
        cx.new(|cx| {
            let mut view = TransactionsView::new(window, cx);
            view.select(["txn-bluebottle-09", "txn-steam-07", "txn-vinted-06"], cx);
            view
        })
        .into()
    })?;
    snapshot(&mut cx, &out, "showcase", |window, cx| {
        cx.new(|cx| Showcase::new(window, cx)).into()
    })?;
    Ok(())
}

/// Opens `build`'s view in a 1440×900 window and writes it as
/// `<name>@1x.png` and `<name>@2x.png`.
fn snapshot(
    cx: &mut HeadlessAppContext,
    out: &Path,
    name: &str,
    build: impl FnOnce(&mut gpui_kit::Window, &mut gpui_kit::App) -> AnyView,
) -> Result<()> {
    let window = cx.open_window(
        size(gpui_kit::px(WIDTH), gpui_kit::px(HEIGHT)),
        |window, cx| {
            let view = build(window, cx);
            cx.new(|cx| gpui_kit::base::Root::new(view, window, cx))
        },
    )?;
    cx.run_until_parked();

    for scale in [1.0_f32, 2.0] {
        // The headless window's backing is always 2x. A 1x scene fills its
        // top-left quarter, so crop to the logical size times the scale.
        cx.update_window(window.into(), |_, window, _| {
            window.set_scale_factor(scale);
        })?;
        cx.run_until_parked();
        let full = cx
            .capture_screenshot(window.into())
            .context("capture the window")?;
        let (w, h) = ((WIDTH * scale) as u32, (HEIGHT * scale) as u32);
        let image = image::imageops::crop_imm(&full, 0, 0, w, h).to_image();
        let path = out.join(format!("{name}@{scale}x.png"));
        image.save(&path)?;
        println!("{} {}x{}", path.display(), image.width(), image.height());
    }
    Ok(())
}
