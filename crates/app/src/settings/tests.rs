//! UI integration tests for Settings: the shell's Settings view and its
//! dialogs in a headless window, against a scratch database through
//! [`SettingsStore`], as `coffer` runs them.

use coffer_core::access_log::{self, NewEntry, Outcome};
use coffer_core::backup;
use coffer_core::db::Database;
use coffer_core::mcp_client::{self, AccessLevel, ClientAccess, ClientName};
use coffer_core::setting;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use tempfile::TempDir;

use super::*;
use crate::accounts::Accounts;
use crate::shell::{ShellView, View};

/// A scratch database with Claude Code enabled at standard access and
/// my-agent asking to connect.
struct Scratch {
    _dir: TempDir,
    path: PathBuf,
}

fn name(name: &str) -> ClientName {
    ClientName::new(name).unwrap()
}

impl Scratch {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("coffer.db");
        let mut db = Database::open(&path).unwrap();
        mcp_client::enable_at(&mut db, &name("claude-code"), AccessLevel::Standard).unwrap();
        mcp_client::record_requested(&mut db, &name("my-agent")).unwrap();
        for (client, outcome, rows) in [
            ("claude-code", Outcome::Ok, 37),
            ("my-agent", Outcome::RefusedNotEnabled, 0),
        ] {
            access_log::append(
                &mut db,
                &NewEntry {
                    client: name(client),
                    tool: "search_transactions".into(),
                    access_level: (outcome == Outcome::Ok).then_some(AccessLevel::Standard),
                    outcome,
                    purpose: Some("Find takeaway spending".into()),
                    args_redacted_json: r#"{"query":{"length":8,"type":"string"}}"#.into(),
                    row_count: rows,
                    result_hash: (outcome == Outcome::Ok).then(|| "ab".into()),
                    withheld_count: 0,
                },
            )
            .unwrap();
        }
        Self { _dir: dir, path }
    }

    fn db(&self) -> Database {
        Database::open(&self.path).unwrap()
    }

    fn access(&self, client: &str) -> ClientAccess {
        mcp_client::access(&self.db().reader().unwrap(), &name(client)).unwrap()
    }
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    scratch: &Scratch,
) -> (
    Entity<ShellView>,
    Entity<SettingsView>,
    &'a mut VisualTestContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        crate::shell::init(cx);
    });
    let store = SettingsStore::new(scratch.path.clone());
    let (shell, cx) = cx.add_window_view(|window, cx| {
        let accounts = cx.new(|_| Accounts::fixture(crate::accounts::fixture::accounts()));
        let settings = cx.new(|cx| SettingsView::from_store(store, cx));
        ShellView::with_settings(View::Settings, accounts, settings, window, cx)
    });
    let settings = cx.update(|_, cx| shell.read(cx).settings().clone());
    redraw(cx);
    (shell, settings, cx)
}

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn shows(id: &str, cx: &mut VisualTestContext) -> bool {
    let id = gpui_kit::SharedString::from(id.to_owned());
    cx.update(move |window, _| window.try_find(id).is_some())
}

fn click(id: &str, cx: &mut VisualTestContext) {
    let id = gpui_kit::SharedString::from(id.to_owned());
    cx.update(move |window, cx| window.click(id, cx));
    redraw(cx);
}

fn press(key: &str, cx: &mut VisualTestContext) {
    let key = key.to_owned();
    cx.update(move |window, cx| window.press(&key, cx));
    redraw(cx);
}

#[gpui_kit::test]
fn the_kill_switch_and_a_clients_switch_refuse_without_forgetting(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_shell, settings, cx) = open(cx, &scratch);
    assert!(shows("client-claude-code", cx));
    assert!(shows("enable-my-agent", cx), "the request is listed");

    click("ai-access", cx);
    assert!(!setting::mcp_enabled(&scratch.db().reader().unwrap()).unwrap());
    assert!(shows("ai-access-turn-on", cx), "the off banner");
    assert!(!cx.update(|_, cx| settings.read(cx).ai_on()));
    click("ai-access-turn-on", cx);
    assert!(setting::mcp_enabled(&scratch.db().reader().unwrap()).unwrap());

    click("client-on-claude-code", cx);
    assert_eq!(scratch.access("claude-code"), ClientAccess::Revoked);
    click("client-on-claude-code", cx);
    assert_eq!(
        scratch.access("claude-code"),
        ClientAccess::Enabled(AccessLevel::Standard),
        "back on at the level it had"
    );
}

#[gpui_kit::test]
fn a_request_is_enabled_at_aggregate_through_the_dialog(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, _settings, cx) = open(cx, &scratch);
    click("enable-my-agent", cx);
    assert!(shows("enable-confirm", cx));
    // Escape cancels and enables nothing.
    press("escape", cx);
    assert!(cx.update(|_, cx| shell.read(cx).dialog().is_none()));
    assert_eq!(scratch.access("my-agent"), ClientAccess::Requested);

    click("enable-my-agent", cx);
    click("enable-confirm", cx);
    assert!(cx.update(|_, cx| shell.read(cx).dialog().is_none()));
    assert_eq!(
        scratch.access("my-agent"),
        ClientAccess::Enabled(AccessLevel::Aggregate)
    );
    assert!(shows("client-my-agent", cx), "now a client");
}

#[gpui_kit::test]
fn ignore_hides_a_request_and_remove_forgets_a_client(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_shell, _settings, cx) = open(cx, &scratch);
    click("ignore-my-agent", cx);
    assert_eq!(scratch.access("my-agent"), ClientAccess::Ignored);
    assert!(!shows("enable-my-agent", cx));

    click("client-claude-code", cx);
    click("remove-client", cx);
    assert!(shows("remove-confirm", cx));
    click("remove-confirm", cx);
    assert_eq!(scratch.access("claude-code"), ClientAccess::Unknown);
    assert!(!shows("client-claude-code", cx));
    // Its log stays.
    assert_eq!(
        access_log::recent(&scratch.db().reader().unwrap(), 10)
            .unwrap()
            .len(),
        2
    );
}

#[gpui_kit::test]
fn the_access_log_shows_calls_and_filters_by_client(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_shell, settings, cx) = open(cx, &scratch);
    click("settings-log", cx);
    let calls = cx.update(|_, cx| settings.read(cx).log_rows().len());
    assert_eq!(calls, 2);
    let selected = cx.update(|_, cx| settings.read(cx).selected_call());
    assert!(selected.is_some(), "the newest call is selected");

    // "Open access log" from a client: its calls only.
    click("settings-clients", cx);
    click("client-claude-code", cx);
    click("open-access-log", cx);
    let (section, rows) = cx.update(|_, cx| {
        let settings = settings.read(cx);
        (
            settings.section(),
            settings
                .log_rows()
                .iter()
                .map(|row| row.call.client.clone())
                .collect::<Vec<_>>(),
        )
    });
    assert_eq!(section, Section::Log);
    assert_eq!(rows, ["claude-code"]);
}

#[gpui_kit::test]
fn back_up_now_then_restore_it(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_shell, settings, cx) = open(cx, &scratch);
    click("settings-backups", cx);
    click("back-up-now", cx);
    let backups = backup::list(&scratch.path).unwrap();
    assert_eq!(backups.len(), 1);
    let notice = cx.update(|_, cx| settings.read(cx).notice().cloned());
    assert!(matches!(
        notice.map(|notice| notice.message),
        Some(NoticeMessage::BackedUp { .. })
    ));

    // A change after the backup, which restoring takes back.
    mcp_client::remove(&mut scratch.db(), &name("claude-code")).unwrap();

    click("restore-backup", cx);
    assert!(shows("restore-confirm", cx));
    cx.run_until_parked();
    click("restore-confirm", cx);
    cx.run_until_parked();
    redraw(cx);
    assert!(!shows("restore-confirm", cx), "the dialog closes");
    assert_eq!(
        scratch.access("claude-code"),
        ClientAccess::Enabled(AccessLevel::Standard),
        "the backup's clients are back"
    );
    // The state before the restore was backed up first.
    assert!(
        backup::list(&scratch.path)
            .unwrap()
            .iter()
            .any(|backup| backup.reason == backup::Reason::Restore)
    );
}

#[gpui_kit::test]
fn the_export_dialog_counts_and_follows_its_toggles(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, _settings, cx) = open(cx, &scratch);
    click("settings-export", cx);
    let dialog = cx.update(|_, cx| {
        shell
            .read(cx)
            .dialog()
            .cloned()
            .and_then(|dialog| dialog.downcast::<export::ExportDialog>().ok())
    });
    let dialog = dialog.expect("the export dialog is open");
    let (counts, problem) =
        cx.update(|_, cx| (dialog.read(cx).counts(), dialog.read(cx).problem().cloned()));
    assert_eq!(problem, None);
    assert_eq!(counts.map(|counts| counts.rows), Some(0));

    click("export-format-json", cx);
    assert_eq!(
        cx.update(|_, cx| dialog.read(cx).format()),
        coffer_core::export::Format::Json
    );
    click("export-excluded", cx);
    click("export-notes", cx);
    let options = cx.update(|_, cx| dialog.update(cx, |dialog, cx| dialog.options(cx)).unwrap());
    assert!(options.include_excluded);
    assert!(!options.include_notes && !options.include_private_tags);
    press("escape", cx);
    assert!(cx.update(|_, cx| shell.read(cx).dialog().is_none()));
}
