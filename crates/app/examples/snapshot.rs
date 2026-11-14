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
//! - `transactions-category-menu`: the in-place category select open on the
//!   Blue Bottle row (composed for M3.3, no Paper drawing);
//! - `transactions-date-menu`: the Date chip's menu (composed for M3.3);
//! - `transactions-sorted`: sorted by amount, largest out first, with the
//!   Uncategorised chip on;
//! - `transactions-no-match`: a search that matches nothing;
//! - `transactions-split`: Paper `2Q3-0`, the Waitrose payment split, with
//!   tags, the bank's note and the user's;
//! - `transactions-split-short` and `-over`: the split editor not balanced
//!   (Paper `4XF-0`);
//! - `transactions-details`: an unsplit row's Details and Split button
//!   (Paper `3WT-0`, without the rule prompt of M3.13);
//! - `transactions-split-start`: just after "Split" (composed for M3.9);
//! - `transactions-tag-menu`: the tag menu with a new name typed (composed
//!   for M3.9);
//! - `rules`: the shell with the Rules tab active and its screen not built
//!   yet (title bar: Paper `B8Y-0`);
//! - `settings`: Settings · AI clients on sample data (Paper `AQ2-0`, title
//!   bar `B08-0`); `settings-off`: the same with AI access off (`GR9-0`);
//!   `settings-log` (`DC6-0`); `settings-backups` (`F63-0`);
//!   `settings-backed-up`: the notification after "Back up now" (`HF7-0`);
//!   `settings-enable` (`GYO-0`), `settings-restore` (`H6D-0`),
//!   `settings-restore-fails` (`H73-0`) and `settings-export` (`HAG-0`): the
//!   dialogs over Settings;
//! - `account`: Current account selected with no transaction selected, on
//!   the states board's accounts (panel and inspector: Paper `8D8-0`);
//! - `account-kind`: the same with the Kind menu open (Paper `FHZ-0`);
//! - `new-account`: the New account dialog (Paper `8IR-0`);
//! - `add-balance`: Add balance for Current account (Paper `I7C-0`);
//! - `inbox-rule`: the Inbox with the UBER *EATS rule open (Paper `ABK-0`);
//! - `inbox-name`: the LIME*RIDE merchant name open (Paper `DL8-0`);
//! - `inbox-categorisation`, `inbox-transfer`, `inbox-refund`,
//!   `inbox-coffer`: the other kinds' inspectors (Paper `564-0`, `EWZ-0`);
//! - `inbox-wide`: a wide rule summed by merchant (Paper `EWZ-0`);
//! - `inbox-approved`: after approving the DELIVEROO rule (`EWZ-0`'s bar,
//!   without Undo);
//! - `inbox-empty`: no suggestions;
//! - `showcase`: the component showcase.
//!
//! The Inbox reads a scratch database seeded with `inbox::fixture`.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use gpui_kit::{AnyView, App, AppContext as _, Entity, HeadlessAppContext, Window, size};

use coffer_app::accounts::{Accounts, fixture as accounts_fixture};
use coffer_app::inbox::store::{Entry, InboxStore};
use coffer_app::inbox::{Inbox, fixture as inbox_fixture};
use coffer_app::settings::restore::{RestoreDialog, Stage};
use coffer_app::settings::{
    Notice, NoticeMessage, RestoreTarget, Section, SettingsView, fixture as settings_fixture,
};
use coffer_app::shell::{ShellView, View};
use coffer_app::showcase::Showcase;
use coffer_app::transactions::{Menu, fixture};
use coffer_core::db::Database;
use coffer_core::inbox::{COFFER_SOURCE, Payload};
use coffer_core::reporting::register::{Sort, SortColumn};

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
                view.select(
                    ["txn-bluebottle-09", "txn-steam-07", "txn-vinted-06"].map(fixture::id),
                    cx,
                )
            });
            shell
        })
        .into()
    })?;
    for (name, step) in [
        ("transactions-category-menu", TableStep::CategoryMenu),
        ("transactions-date-menu", TableStep::DateMenu),
        ("transactions-sorted", TableStep::Sorted),
        ("transactions-no-match", TableStep::NoMatch),
        ("transactions-split", TableStep::Split),
        ("transactions-split-short", TableStep::SplitShort),
        ("transactions-split-over", TableStep::SplitOver),
        ("transactions-details", TableStep::Details),
        ("transactions-split-start", TableStep::SplitStart),
        ("transactions-tag-menu", TableStep::TagMenu),
    ] {
        let built: Rc<RefCell<Option<Entity<ShellView>>>> = Rc::default();
        let keep = built.clone();
        let build = move |window: &mut Window, cx: &mut App| -> AnyView {
            let shell = cx.new(|cx| ShellView::new(View::Transactions, window, cx));
            *keep.borrow_mut() = Some(shell.clone());
            shell.into()
        };
        let then = move |window: &mut Window, cx: &mut App| {
            let shell = built.borrow().clone().expect("the shell was built");
            let view = shell.read(cx).transactions().clone();
            view.update(cx, |view, cx| match step {
                TableStep::CategoryMenu => {
                    let id = fixture::id("txn-bluebottle-09");
                    view.select([id], cx);
                    view.show_menu(Menu::Row(id), window, cx);
                }
                TableStep::DateMenu => view.show_menu(Menu::Date, window, cx),
                TableStep::Sorted => {
                    view.set_sort(Sort::Ascending(SortColumn::Amount), cx);
                    view.set_uncategorised(true, cx);
                }
                TableStep::NoMatch => view.set_search("zzz", window, cx),
                TableStep::Split | TableStep::SplitShort | TableStep::SplitOver => {
                    view.show_fixture(fixture::split_register(), cx);
                    view.select([fixture::id("txn-waitrose-07")], cx);
                    match step {
                        // Paper's states board: the third line removed.
                        TableStep::SplitShort => view.remove_split_line_at(2, window, cx),
                        TableStep::SplitOver => {
                            view.remove_split_line_at(2, window, cx);
                            view.type_split_amount(1, "17.10", window, cx);
                        }
                        _ => {}
                    }
                }
                TableStep::Details => view.select([fixture::id("txn-vinted-06")], cx),
                TableStep::SplitStart => {
                    view.select([fixture::id("txn-vinted-06")], cx);
                    view.split_selected(window, cx);
                }
                TableStep::TagMenu => {
                    view.show_fixture(fixture::split_register(), cx);
                    view.select([fixture::id("txn-waitrose-07")], cx);
                    view.show_menu(Menu::Tags, window, cx);
                    view.type_in_menu("Gift", window, cx);
                }
            });
        };
        snapshot_then(&mut cx, &out, name, build, then)?;
    }
    snapshot(&mut cx, &out, "rules", |window, cx| {
        cx.new(|cx| ShellView::new(View::Rules, window, cx)).into()
    })?;
    snapshot(&mut cx, &out, "settings", |window, cx| {
        cx.new(|cx| ShellView::new(View::Settings, window, cx))
            .into()
    })?;
    for (name, section) in [
        ("settings-log", Section::Log),
        ("settings-backups", Section::Backups),
    ] {
        snapshot(&mut cx, &out, name, move |window, cx| {
            let shell = cx.new(|cx| ShellView::new(View::Settings, window, cx));
            let settings = shell.read(cx).settings().clone();
            settings.update(cx, |settings, cx| settings.show(section, cx));
            shell.into()
        })?;
    }
    snapshot(&mut cx, &out, "settings-off", |window, cx| {
        let accounts = cx.new(|_| Accounts::fixture(accounts_fixture::accounts()));
        let settings = cx.new(SettingsView::fixture_off);
        cx.new(|cx| ShellView::with_settings(View::Settings, accounts, settings, window, cx))
            .into()
    })?;
    snapshot(&mut cx, &out, "settings-backed-up", |window, cx| {
        let shell = cx.new(|cx| ShellView::new(View::Settings, window, cx));
        let settings = shell.read(cx).settings().clone();
        settings.update(cx, |settings, cx| {
            settings.show(Section::Backups, cx);
            settings.set_notice(
                Some(Notice {
                    message: NoticeMessage::BackedUp {
                        transactions: Some(1_312),
                        size: 53 * 1024 * 1024 / 10,
                    },
                    reveal: Some(PathBuf::from("backup.db")),
                }),
                cx,
            );
        });
        shell.into()
    })?;
    for (name, dialog) in [
        ("settings-enable", SettingsDialog::Enable),
        ("settings-export", SettingsDialog::Export),
    ] {
        snapshot(&mut cx, &out, name, move |window, cx| {
            let shell = cx.new(|cx| ShellView::new(View::Settings, window, cx));
            let settings = shell.read(cx).settings().clone();
            settings.update(cx, |settings, cx| match dialog {
                SettingsDialog::Enable => settings.request_enable(
                    coffer_core::mcp_client::ClientName::new("my-agent").unwrap(),
                    cx,
                ),
                SettingsDialog::Export => settings.request_export(cx),
            });
            shell.into()
        })?;
    }
    for (name, fails) in [
        ("settings-restore", false),
        ("settings-restore-fails", true),
    ] {
        snapshot(&mut cx, &out, name, move |_, cx| {
            let settings = cx.new(SettingsView::fixture);
            let supported = coffer_core::db::Database::supported_schema_version();
            let (target, stage, since) = if fails {
                (
                    RestoreTarget {
                        path: PathBuf::from("coffer-laptop.db"),
                        taken_at: None,
                        local: None,
                    },
                    Stage::Checked(coffer_core::backup::detail::Assessment::TooNew {
                        found: supported + 2,
                        supported,
                    }),
                    None,
                )
            } else {
                let row = settings.read(cx).backups().rows[0].clone();
                (
                    RestoreTarget {
                        path: row.backup.path.clone(),
                        taken_at: Some(row.backup.taken_at),
                        local: Some(row.local),
                    },
                    Stage::Checked(coffer_core::backup::detail::Assessment::Ready(
                        coffer_core::backup::RestoreCheck {
                            path: row.backup.path.clone(),
                            named: Some((row.backup.reason, row.backup.taken_at)),
                            schema_version: supported,
                            supported_version: supported,
                            current: coffer_core::backup::Current::At(supported),
                            interrupted: None,
                        },
                    )),
                    Some(settings_fixture::since()),
                )
            };
            cx.new(|cx| RestoreDialog::preview(settings, target, stage, since, cx))
                .into()
        })?;
    }
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
    inbox_snapshots(&mut cx, &out)?;
    snapshot(&mut cx, &out, "showcase", |window, cx| {
        cx.new(|cx| Showcase::new(window, cx)).into()
    })?;
    Ok(())
}

/// What a Transactions snapshot does once the window has settled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TableStep {
    CategoryMenu,
    DateMenu,
    Sorted,
    NoMatch,
    Split,
    SplitShort,
    SplitOver,
    Details,
    SplitStart,
    TagMenu,
}

/// Which suggestion an Inbox snapshot opens.
type Pick = fn(&Entry) -> bool;

fn is_pattern(entry: &Entry, pattern: &str) -> bool {
    matches!(&entry.suggestion.payload, Payload::Rule(rule) if rule.pattern().as_str() == pattern)
}

/// The Inbox snapshots: each on a fresh scratch database.
fn inbox_snapshots(cx: &mut HeadlessAppContext, out: &Path) -> Result<()> {
    let shots: [(&str, Option<Pick>, bool, bool); 9] = [
        (
            "inbox-rule",
            Some(|e| is_pattern(e, "UBER *EATS")),
            false,
            false,
        ),
        (
            "inbox-name",
            Some(|e| matches!(e.suggestion.payload, Payload::MerchantAlias(_))),
            false,
            false,
        ),
        (
            "inbox-categorisation",
            Some(|e| {
                matches!(&e.subject, coffer_core::inbox::preview::Subject::Categorization(items)
                    if items.first().and_then(|i| i.transaction.as_ref()).is_some_and(|t| t.name() == "UBER *EATS"))
            }),
            false,
            false,
        ),
        (
            "inbox-transfer",
            Some(|e| matches!(e.suggestion.payload, Payload::TransferPair(_))),
            false,
            false,
        ),
        (
            "inbox-refund",
            Some(|e| matches!(e.suggestion.payload, Payload::RefundLink(_))),
            false,
            false,
        ),
        (
            "inbox-coffer",
            Some(|e| e.suggestion.source_client == COFFER_SOURCE),
            false,
            false,
        ),
        ("inbox-wide", Some(|e| is_pattern(e, "PAY")), true, false),
        (
            "inbox-approved",
            Some(|e| is_pattern(e, "DELIVEROO")),
            false,
            true,
        ),
        ("inbox-empty", None, false, false),
    ];
    for (name, pick, wide, approve) in shots {
        let dir = tempfile::TempDir::new()?;
        let database = dir.path().join("coffer.db");
        {
            let mut db = Database::open(&database)?;
            if name != "inbox-empty" {
                inbox_fixture::seed(&mut db);
            }
            if wide {
                inbox_fixture::seed_wide_rule(&mut db);
            }
        }
        snapshot(cx, out, name, move |window, cx| {
            let accounts = cx.new(|_| Accounts::fixture(accounts_fixture::accounts()));
            let inbox = cx.new(|_| Inbox::from_store(InboxStore::new(database)));
            inbox.update(cx, |inbox, cx| {
                let id = pick.and_then(|pick| {
                    inbox
                        .data()
                        .entries
                        .iter()
                        .find(|entry| pick(entry))
                        .map(Entry::id)
                });
                if approve {
                    if let Some(id) = id {
                        inbox.approve(id, cx);
                    }
                } else {
                    inbox.select(id, cx);
                }
            });
            cx.new(|cx| ShellView::with_data(View::Inbox, accounts, inbox, window, cx))
                .into()
        })?;
        drop(dir);
    }
    Ok(())
}

/// Which Settings dialog a snapshot opens over the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsDialog {
    Enable,
    Export,
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
