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
//! - `rules`: the shell with the Rules tab active and its screen not built
//!   yet (title bar: Paper `B8Y-0`);
//! - `settings`: the shell with Settings open (title bar: Paper `B08-0`);
//! - `account`: Current account selected with no transaction selected, on
//!   the states board's accounts (panel and inspector: Paper `8D8-0`);
//! - `account-kind`: the same with the Kind menu open (Paper `FHZ-0`);
//! - `new-account`: the New account dialog (Paper `8IR-0`);
//! - `add-balance`: Add balance for Current account (Paper `I7C-0`);
//! - `showcase`: the component showcase.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use gpui_kit::{AnyView, App, AppContext as _, Entity, HeadlessAppContext, Window, size};

use coffer_app::accounts::{Accounts, fixture as accounts_fixture};
use coffer_app::shell::{ShellView, View};
use coffer_app::showcase::Showcase;

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
        coffer_app::shell::init(cx);
    });

    snapshot(&mut cx, &out, "transactions", |window, cx| {
        cx.new(|cx| ShellView::new(View::Transactions, window, cx))
            .into()
    })?;
    snapshot(&mut cx, &out, "transactions-multi", |window, cx| {
        cx.new(|cx| {
            let shell = ShellView::new(View::Transactions, window, cx);
            shell.transactions().update(cx, |view, cx| {
                view.select(["txn-bluebottle-09", "txn-steam-07", "txn-vinted-06"], cx)
            });
            shell
        })
        .into()
    })?;
    snapshot(&mut cx, &out, "rules", |window, cx| {
        cx.new(|cx| ShellView::new(View::Rules, window, cx)).into()
    })?;
    snapshot(&mut cx, &out, "settings", |window, cx| {
        cx.new(|cx| ShellView::new(View::Settings, window, cx))
            .into()
    })?;
    for (name, step) in [
        ("account", AccountStep::Select),
        ("account-kind", AccountStep::KindMenu),
        ("new-account", AccountStep::NewAccount),
        ("add-balance", AccountStep::AddBalance),
    ] {
        let built: Rc<RefCell<Option<Entity<ShellView>>>> = Rc::default();
        let keep = built.clone();
        let build = move |window: &mut Window, cx: &mut App| -> AnyView {
            let accounts = cx.new(|_| Accounts::fixture(accounts_fixture::with_states()));
            let shell = cx.new(|cx| {
                ShellView::with_accounts(View::Transactions, accounts.clone(), window, cx)
            });
            accounts.update(cx, |accounts, cx| {
                accounts.select(Some(accounts_fixture::CURRENT), cx);
                match step {
                    AccountStep::Select | AccountStep::KindMenu => {}
                    AccountStep::NewAccount => accounts.request_new_account(cx),
                    AccountStep::AddBalance => {
                        accounts.request_add_balance(accounts_fixture::CURRENT, cx)
                    }
                }
            });
            *keep.borrow_mut() = Some(shell.clone());
            shell.into()
        };
        // Opened once the selection has settled, as a click would.
        let then = move |window: &mut Window, cx: &mut App| {
            if step != AccountStep::KindMenu {
                return;
            }
            let shell = built.borrow().clone().expect("the shell was built");
            let inspector = shell
                .read(cx)
                .transactions()
                .read(cx)
                .account_inspector(cx)
                .expect("an account is selected");
            inspector.update(cx, |inspector, cx| inspector.open_kind_menu(window, cx));
        };
        snapshot_then(&mut cx, &out, name, build, then)?;
    }
    snapshot(&mut cx, &out, "showcase", |window, cx| {
        cx.new(|cx| Showcase::new(window, cx)).into()
    })?;
    Ok(())
}

/// What the account snapshots do after selecting Current account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AccountStep {
    Select,
    KindMenu,
    NewAccount,
    AddBalance,
}

/// Opens `build`'s view in a 1440×900 window and writes it as
/// `<name>@1x.png` and `<name>@2x.png`.
fn snapshot(
    cx: &mut HeadlessAppContext,
    out: &Path,
    name: &str,
    build: impl FnOnce(&mut Window, &mut App) -> AnyView,
) -> Result<()> {
    snapshot_then(cx, out, name, build, |_, _| {})
}

/// [`snapshot`], running `then` once the window has settled, before the
/// capture: for a state reached by a click (an open menu).
fn snapshot_then(
    cx: &mut HeadlessAppContext,
    out: &Path,
    name: &str,
    build: impl FnOnce(&mut Window, &mut App) -> AnyView,
    then: impl FnOnce(&mut Window, &mut App),
) -> Result<()> {
    let window = cx.open_window(
        size(gpui_kit::px(WIDTH), gpui_kit::px(HEIGHT)),
        |window, cx| {
            let view = build(window, cx);
            cx.new(|cx| gpui_kit::base::Root::new(view, window, cx))
        },
    )?;
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| then(window, cx))?;
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
