//! Settings (ROADMAP M3.15, M3.16; `docs/DESIGN.md` → "Settings"; Paper
//! `AQ2-0`, `DC6-0`, `F63-0`, states `GQA-0`): AI access and the data.
//!
//! The title bar's Settings button (⌘,) opens it. The left panel ([`panel`])
//! has AI ACCESS: Clients and Access log; DATA: Backups and Export…; and at
//! its foot the data file's path.
//!
//! - [`clients`]: the kill switch, the enabled clients (level, last call,
//!   calls in 7 days, on/off), the clients that asked to connect (Ignore,
//!   Enable…), what each level shares, and the client inspector (level,
//!   the command, the last 7 days, Remove client…).
//! - [`log`]: the access log by day, filtered by client and period, and
//!   the call inspector (the redacted arguments, the purpose as the
//!   client's untrusted words). Never the result hash (decided with the
//!   user, as `coffer ai-access log`).
//! - [`backups`]: Recent and One a month, Back up now, Restore from a
//!   file…, and the backup inspector (what it holds, what restoring would
//!   undo, Restore…, Show in folder, Save a copy…).
//! - Dialogs, opened by the shell over everything: [`enable`], [`remove`],
//!   [`restore`] and [`export`].
//!
//! [`SettingsView`] holds what the screens show and makes every change
//! through [`store::SettingsStore`], which opens the database per call; the
//! rules are core's. Without a database (snapshots, tests) it shows
//! [`fixture`] data, which can't change.

pub mod backups;
pub mod clients;
pub mod data;
pub mod enable;
pub mod export;
pub mod fixture;
pub mod log;
pub mod panel;
pub mod remove;
pub mod restore;
pub mod store;
pub(crate) mod table;

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use coffer_core::access_log::activity::Cursor;
use coffer_core::backup::detail::Since;
use coffer_core::mcp_client::{AccessLevel, ClientName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, EventEmitter, FocusHandle, InteractiveElement as _, IntoElement,
    KeyBinding, ParentElement as _, Render, ScrollHandle, SharedString, Styled as _, Task, Window,
    div,
};

use crate::accounts::store::StoreError;
use crate::theme::tokens::{color, spacing};
use crate::ui::menu::SelectState;

use data::{AiData, BackupsData, Clock, LogFilter, LogRow, Period};
use store::SettingsStore;

/// Binds the arrow keys of Settings' panel and lists. Call once.
pub fn init(cx: &mut App) {
    use gpui_kit::base::actions::{SelectDown, SelectUp};
    cx.bind_keys([
        KeyBinding::new("up", SelectUp, Some(panel::CONTEXT)),
        KeyBinding::new("down", SelectDown, Some(panel::CONTEXT)),
        KeyBinding::new("up", SelectUp, Some(table::LIST_CONTEXT)),
        KeyBinding::new("down", SelectDown, Some(table::LIST_CONTEXT)),
    ]);
}

/// Which screen Settings shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Section {
    #[default]
    Clients,
    Log,
    Backups,
}

impl Section {
    pub const ALL: [Section; 3] = [Section::Clients, Section::Log, Section::Backups];
}

/// What a dialog restores: a listed backup, or a file the user chose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestoreTarget {
    pub path: PathBuf,
    /// When it was taken, for a listed backup (or a file with a backup's
    /// name): the title names it, and "since then" is counted from it.
    pub taken_at: Option<DateTime<Utc>>,
    /// That time, local.
    pub local: Option<chrono::NaiveDateTime>,
}

/// Something Settings asks the shell for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsEvent {
    /// Open the Enable dialog for a client that asked to connect.
    EnableRequested(ClientName),
    /// Open the Remove client dialog.
    RemoveRequested(ClientName),
    /// Open the Restore dialog.
    RestoreRequested(RestoreTarget),
    /// Open the export dialog.
    ExportRequested,
    /// A backup was restored: everything else reads the database again.
    Restored,
    /// The kill switch or the clients changed: the status bar follows.
    AiAccessChanged,
}

/// What just happened, in the notification bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub message: NoticeMessage,
    /// "Show in folder" reveals this file.
    pub reveal: Option<PathBuf>,
}

/// A notification's message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoticeMessage {
    /// After "Back up now": the transactions it holds and its size.
    BackedUp {
        transactions: Option<u64>,
        size: u64,
    },
    /// After an export: rows written and the file's name.
    Exported { rows: usize, file: String },
    /// After a copy of a backup was saved.
    Copied { file: String },
    /// After a restore.
    Restored {
        migrated: bool,
        log_kept: Option<usize>,
    },
    /// Something failed; the text says what, and that nothing changed.
    Failed(SharedString),
}

/// Where Settings' data comes from.
enum Source {
    Database(SettingsStore),
    /// Sample data that can't change.
    Fixture,
}

/// A read that failed, shown where its data would be.
type LoadError = Option<SharedString>;

/// The Settings screen.
pub struct SettingsView {
    source: Source,
    section: Section,
    clock: Clock,
    /// The data file, as the panel's foot shows it (`~` for home).
    data_file: SharedString,

    ai: AiData,
    ai_error: LoadError,
    selected_client: Option<ClientName>,
    level_select: SelectState,

    log_filter: LogFilter,
    log: Vec<LogRow>,
    log_more: bool,
    log_error: LoadError,
    selected_call: Option<i64>,
    client_filter: SelectState,
    period_filter: SelectState,
    log_scroll: ScrollHandle,

    backups: BackupsData,
    backups_error: LoadError,
    selected_backup: Option<PathBuf>,
    /// What restoring the selected backup would undo.
    since: Option<(PathBuf, Since)>,
    /// A backup is being taken.
    backing_up: bool,

    notice: Option<Notice>,
    panel_focus: FocusHandle,
    list_focus: FocusHandle,
    _task: Option<Task<()>>,
}

impl EventEmitter<SettingsEvent> for SettingsView {}

/// `path` with the home directory written as `~`.
pub fn display_path(path: &Path) -> String {
    let text = path.display().to_string();
    match std::env::home_dir() {
        Some(home) if !home.as_os_str().is_empty() => match path.strip_prefix(&home) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => text,
        },
        _ => text,
    }
}

/// `message` as a sentence: a capital first letter and a full stop.
pub fn sentence(message: &str) -> SharedString {
    crate::accounts::new_account::sentence(message)
}

impl SettingsView {
    /// Settings over the database `store` opens, read now.
    pub fn from_store(store: SettingsStore, cx: &mut Context<Self>) -> Self {
        let clock = store::now().unwrap_or_else(|_| fixture::clock());
        let data_file = display_path(store.database()).into();
        let mut view = Self::empty(Source::Database(store), clock, data_file, cx);
        view.reload(cx);
        view
    }

    /// Settings showing sample data, which can't change.
    pub fn fixture(cx: &mut Context<Self>) -> Self {
        let mut view = Self::empty(
            Source::Fixture,
            fixture::clock(),
            fixture::DATA_FILE.into(),
            cx,
        );
        view.ai = fixture::ai();
        view.log = fixture::log();
        view.backups = fixture::backups();
        view.since = view
            .backups
            .rows
            .first()
            .map(|row| (row.backup.path.clone(), fixture::since()));
        view.select_defaults();
        view
    }

    /// Sample data with AI access switched off (the states board, `GR9-0`).
    pub fn fixture_off(cx: &mut Context<Self>) -> Self {
        let mut view = Self::fixture(cx);
        view.ai.on = false;
        view
    }

    /// Settings with nothing to show and why: no data folder.
    pub fn unavailable(reason: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        let reason = reason.into();
        let mut view = Self::empty(Source::Fixture, fixture::clock(), "".into(), cx);
        view.ai_error = Some(reason.clone());
        view.log_error = Some(reason.clone());
        view.backups_error = Some(reason);
        view
    }

    fn empty(
        source: Source,
        clock: Clock,
        data_file: SharedString,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            source,
            section: Section::default(),
            clock,
            data_file,
            ai: AiData::default(),
            ai_error: None,
            selected_client: None,
            level_select: SelectState::new(cx),
            log_filter: LogFilter::default(),
            log: Vec::new(),
            log_more: false,
            log_error: None,
            selected_call: None,
            client_filter: SelectState::new(cx),
            period_filter: SelectState::new(cx),
            log_scroll: ScrollHandle::new(),
            backups: BackupsData::default(),
            backups_error: None,
            selected_backup: None,
            since: None,
            backing_up: false,
            notice: None,
            panel_focus: cx.focus_handle(),
            list_focus: cx.focus_handle(),
            _task: None,
        }
    }

    pub fn section(&self) -> Section {
        self.section
    }

    pub fn ai(&self) -> &AiData {
        &self.ai
    }

    pub fn backups(&self) -> &BackupsData {
        &self.backups
    }

    pub fn log_rows(&self) -> &[LogRow] {
        &self.log
    }

    pub fn log_filter(&self) -> &LogFilter {
        &self.log_filter
    }

    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    pub fn selected_client(&self) -> Option<&ClientName> {
        self.selected_client.as_ref()
    }

    pub fn selected_backup(&self) -> Option<&PathBuf> {
        self.selected_backup.as_ref()
    }

    pub fn selected_call(&self) -> Option<i64> {
        self.selected_call
    }

    /// Whether AI access is on (the kill switch), for the status bar.
    pub fn ai_on(&self) -> bool {
        self.ai.on
    }

    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    /// The store, or why nothing can change.
    pub fn store(&self) -> Result<&SettingsStore, SharedString> {
        match &self.source {
            Source::Database(store) => Ok(store),
            Source::Fixture => Err("This is sample data, which can't be changed.".into()),
        }
    }

    /// Shows `section`, reading its data again.
    pub fn show(&mut self, section: Section, cx: &mut Context<Self>) {
        self.section = section;
        self.notice = None;
        self.reload_section(section);
        self.select_defaults();
        cx.notify();
    }

    /// Reads everything again: after a restore, or when Settings opens.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        for section in Section::ALL {
            self.reload_section(section);
        }
        self.select_defaults();
        cx.notify();
    }

    fn reload_section(&mut self, section: Section) {
        let Source::Database(store) = &self.source else {
            return;
        };
        if let Ok(clock) = store::now() {
            self.clock = clock;
        }
        let store = store.clone();
        match section {
            Section::Clients => match store.load_ai(&self.clock) {
                Ok(ai) => {
                    self.ai = ai;
                    self.ai_error = None;
                }
                Err(error) => {
                    tracing::warn!(%error, "couldn't read the AI clients");
                    self.ai_error = Some(sentence(&error.to_string()));
                }
            },
            Section::Log => self.load_log(&store, None),
            Section::Backups => {
                match store.load_backups() {
                    Ok(backups) => {
                        self.backups = backups;
                        self.backups_error = None;
                    }
                    Err(error) => {
                        tracing::warn!(%error, "couldn't read the backups");
                        self.backups_error = Some(sentence(&error.to_string()));
                    }
                }
                self.since = None;
            }
        }
    }

    /// Reads the first page of the log for the filter, or the next page
    /// after `after`.
    fn load_log(&mut self, store: &SettingsStore, after: Option<Cursor>) {
        let next = after.is_some();
        match store.load_log(&self.log_filter, &self.clock, after) {
            Ok(page) => {
                if next {
                    self.log.extend(page.rows);
                } else {
                    self.log = page.rows;
                }
                self.log_more = page.more;
                self.log_error = None;
            }
            Err(error) => {
                tracing::warn!(%error, "couldn't read the access log");
                self.log_error = Some(sentence(&error.to_string()));
            }
        }
    }

    /// Reads the next page of the log, when there is one: earlier days load
    /// as the list is scrolled to its end.
    pub fn load_more_log(&mut self, cx: &mut Context<Self>) {
        if !self.log_more {
            return;
        }
        let Source::Database(store) = &self.source else {
            return;
        };
        let store = store.clone();
        let after = self.log.last().map(|row| Cursor::after(&row.call));
        self.load_log(&store, after);
        cx.notify();
    }

    /// Selects the first row of each list whose selection is gone.
    fn select_defaults(&mut self) {
        if self
            .selected_client
            .as_ref()
            .is_none_or(|name| self.ai.client(name).is_none())
        {
            self.selected_client = self.ai.clients.first().map(|client| client.name.clone());
        }
        if self
            .selected_call
            .is_none_or(|id| !self.log.iter().any(|row| row.call.id == id))
        {
            self.selected_call = self.log.first().map(|row| row.call.id);
        }
        if self
            .selected_backup
            .as_ref()
            .is_none_or(|path| self.backups.row(path).is_none())
        {
            self.selected_backup = self.backups.rows.first().map(|row| row.backup.path.clone());
        }
        self.load_since();
    }

    /// Reads what restoring the selected backup would undo, unless known.
    fn load_since(&mut self) {
        let Some(path) = self.selected_backup.clone() else {
            self.since = None;
            return;
        };
        if self.since.as_ref().is_some_and(|(known, _)| *known == path) {
            return;
        }
        let Source::Database(store) = &self.source else {
            return;
        };
        let Some(row) = self.backups.row(&path) else {
            return;
        };
        self.since = match store.since(row.backup.taken_at) {
            Ok(since) => Some((path, since)),
            Err(error) => {
                tracing::warn!(%error, "couldn't count what restoring would undo");
                None
            }
        };
    }

    pub fn select_client(&mut self, name: ClientName, cx: &mut Context<Self>) {
        self.level_select.dismiss();
        self.selected_client = Some(name);
        cx.notify();
    }

    pub fn select_call(&mut self, id: i64, cx: &mut Context<Self>) {
        self.selected_call = Some(id);
        cx.notify();
    }

    pub fn select_backup(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.selected_backup = Some(path);
        self.load_since();
        cx.notify();
    }

    /// Moves the selection in the open list `step` rows (the arrow keys).
    pub fn select_next(&mut self, step: isize, cx: &mut Context<Self>) {
        fn next<T: PartialEq + Clone>(rows: &[T], current: Option<&T>, step: isize) -> Option<T> {
            if rows.is_empty() {
                return None;
            }
            let at = current.and_then(|current| rows.iter().position(|row| row == current));
            let index = match at {
                Some(at) => at.saturating_add_signed(step).min(rows.len() - 1),
                None if step < 0 => rows.len() - 1,
                None => 0,
            };
            Some(rows[index].clone())
        }
        match self.section {
            Section::Clients => {
                let names: Vec<ClientName> = self
                    .ai
                    .clients
                    .iter()
                    .map(|client| client.name.clone())
                    .collect();
                if let Some(name) = next(&names, self.selected_client.as_ref(), step) {
                    self.select_client(name, cx);
                }
            }
            Section::Log => {
                let ids: Vec<i64> = self.log.iter().map(|row| row.call.id).collect();
                if let Some(id) = next(&ids, self.selected_call.as_ref(), step) {
                    self.select_call(id, cx);
                }
            }
            Section::Backups => {
                let paths: Vec<PathBuf> = table::backup_order(&self.backups);
                if let Some(path) = next(&paths, self.selected_backup.as_ref(), step) {
                    self.select_backup(path, cx);
                }
            }
        }
    }

    /// Moves to the next or previous section in the panel (the arrow keys
    /// there).
    pub fn show_next(&mut self, step: isize, cx: &mut Context<Self>) {
        let at = Section::ALL
            .iter()
            .position(|section| *section == self.section)
            .unwrap_or(0);
        let index = at.saturating_add_signed(step).min(Section::ALL.len() - 1);
        self.show(Section::ALL[index], cx);
    }

    /// Runs a change through the store, then reads the clients again and
    /// tells the shell. A refused change is shown in the notification bar.
    fn change_ai(
        &mut self,
        change: impl FnOnce(&SettingsStore) -> Result<(), StoreError>,
        cx: &mut Context<Self>,
    ) {
        let result = match self.store() {
            Ok(store) => change(store).map_err(|error| sentence(&error.to_string())),
            Err(error) => Err(error),
        };
        if let Err(error) = result {
            self.notice = Some(Notice {
                message: NoticeMessage::Failed(error),
                reveal: None,
            });
        }
        self.reload_section(Section::Clients);
        self.select_defaults();
        cx.emit(SettingsEvent::AiAccessChanged);
        cx.notify();
    }

    /// The kill switch.
    pub fn set_ai_on(&mut self, on: bool, cx: &mut Context<Self>) {
        self.change_ai(|store| store.set_ai_on(on), cx);
    }

    /// A client's ON switch: off keeps its level.
    pub fn turn_client(&mut self, name: ClientName, on: bool, cx: &mut Context<Self>) {
        self.change_ai(move |store| store.turn(&name, on), cx);
    }

    pub fn set_level(&mut self, name: ClientName, level: AccessLevel, cx: &mut Context<Self>) {
        self.change_ai(move |store| store.set_level(&name, level), cx);
    }

    /// "Ignore" on a request: hidden, not enabled.
    pub fn ignore(&mut self, name: ClientName, cx: &mut Context<Self>) {
        self.change_ai(move |store| store.ignore(&name), cx);
    }

    /// Enables a client at `level` (the Enable dialog). The error says why
    /// it wasn't.
    pub fn enable(
        &mut self,
        name: &ClientName,
        level: AccessLevel,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        self.store()?
            .enable(name, level)
            .map_err(|error| sentence(&error.to_string()))?;
        self.reload_section(Section::Clients);
        self.selected_client = Some(name.clone());
        self.select_defaults();
        cx.emit(SettingsEvent::AiAccessChanged);
        cx.notify();
        Ok(())
    }

    /// Removes a client (the Remove client dialog).
    pub fn remove(
        &mut self,
        name: &ClientName,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        self.store()?
            .remove(name)
            .map_err(|error| sentence(&error.to_string()))?;
        self.reload_section(Section::Clients);
        self.select_defaults();
        cx.emit(SettingsEvent::AiAccessChanged);
        cx.notify();
        Ok(())
    }

    /// "Open access log" in the client inspector: the log, that client's
    /// calls only.
    pub fn open_log_for(&mut self, client: Option<String>, cx: &mut Context<Self>) {
        self.log_filter.client = client;
        self.selected_call = None;
        self.show(Section::Log, cx);
    }

    /// Changes the log's filter and reads it again.
    pub fn set_log_filter(&mut self, filter: LogFilter, cx: &mut Context<Self>) {
        self.log_filter = filter;
        self.selected_call = None;
        self.reload_section(Section::Log);
        self.select_defaults();
        self.log_scroll.set_offset(Default::default());
        cx.notify();
    }

    pub fn request_enable(&mut self, name: ClientName, cx: &mut Context<Self>) {
        cx.emit(SettingsEvent::EnableRequested(name));
    }

    pub fn request_remove(&mut self, name: ClientName, cx: &mut Context<Self>) {
        cx.emit(SettingsEvent::RemoveRequested(name));
    }

    pub fn request_export(&mut self, cx: &mut Context<Self>) {
        cx.emit(SettingsEvent::ExportRequested);
    }

    /// "Restore…" on the selected backup.
    pub fn request_restore_selected(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self
            .selected_backup
            .as_ref()
            .and_then(|path| self.backups.row(path))
        else {
            return;
        };
        cx.emit(SettingsEvent::RestoreRequested(RestoreTarget {
            path: row.backup.path.clone(),
            taken_at: Some(row.backup.taken_at),
            local: Some(row.local),
        }));
    }

    /// "Restore from a file…": the system's open dialog, then the Restore
    /// dialog for the file chosen.
    pub fn restore_from_file(&mut self, cx: &mut Context<Self>) {
        if let Err(error) = self.store() {
            self.fail(error, cx);
            return;
        }
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Restore".into()),
        });
        self._task = Some(cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let named = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(coffer_core::backup::parse_file_name)
                .map(|(_, taken_at)| taken_at);
            let _ = this.update(cx, |_, cx| {
                cx.emit(SettingsEvent::RestoreRequested(RestoreTarget {
                    path,
                    taken_at: named,
                    local: named.map(store::local),
                }))
            });
        }));
    }

    /// "Back up now": a manual backup on a background thread, then the
    /// notification.
    pub fn back_up_now(&mut self, cx: &mut Context<Self>) {
        if self.backing_up {
            return;
        }
        let store = match self.store() {
            Ok(store) => store.clone(),
            Err(error) => {
                self.fail(error, cx);
                return;
            }
        };
        self.backing_up = true;
        self.notice = None;
        cx.notify();
        let work = cx
            .background_executor()
            .spawn(async move { store.back_up() });
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.backing_up = false;
                this.notice = Some(match result {
                    Ok((backup, contents)) => {
                        this.selected_backup = Some(backup.path.clone());
                        Notice {
                            message: NoticeMessage::BackedUp {
                                transactions: contents.map(|contents| contents.transactions),
                                size: backup.size,
                            },
                            reveal: Some(backup.path),
                        }
                    }
                    Err(error) => Notice {
                        message: NoticeMessage::Failed(sentence(&format!(
                            "Couldn't back up: {error}. Nothing changed"
                        ))),
                        reveal: None,
                    },
                });
                this.reload_section(Section::Backups);
                this.select_defaults();
                cx.notify();
            });
        }));
    }

    /// "Save a copy…" on the selected backup: the system's save dialog,
    /// then a private copy there.
    pub fn save_copy(&mut self, cx: &mut Context<Self>) {
        let store = match self.store() {
            Ok(store) => store.clone(),
            Err(error) => {
                self.fail(error, cx);
                return;
            }
        };
        let Some(from) = self.selected_backup.clone() else {
            return;
        };
        let name = from
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dir = std::env::home_dir().unwrap_or_default();
        let target = cx.prompt_for_new_path(&dir, Some(&name));
        self._task = Some(cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(to))) = target.await else {
                return;
            };
            let result = store.save_copy(&from, &to);
            let _ = this.update(cx, |this, cx| {
                this.notice = Some(match result {
                    Ok(()) => Notice {
                        message: NoticeMessage::Copied {
                            file: to
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                        },
                        reveal: Some(to),
                    },
                    Err(error) => Notice {
                        message: NoticeMessage::Failed(sentence(&format!(
                            "Couldn't save a copy: {error}"
                        ))),
                        reveal: None,
                    },
                });
                cx.notify();
            });
        }));
    }

    /// Shows the file at `path` in the system's file manager.
    pub fn reveal(&self, path: &Path, cx: &mut Context<Self>) {
        cx.reveal_path(path);
    }

    /// After a restore (the Restore dialog): reads everything again, says
    /// so, and tells the shell.
    pub fn restored(&mut self, restored: &coffer_core::backup::Restored, cx: &mut Context<Self>) {
        self.notice = Some(Notice {
            message: NoticeMessage::Restored {
                migrated: restored.migrated_to.is_some(),
                log_kept: restored.log_rows_carried,
            },
            reveal: restored
                .pre_restore
                .as_ref()
                .map(|backup| backup.path.clone()),
        });
        self.selected_backup = None;
        self.since = None;
        self.reload(cx);
        cx.emit(SettingsEvent::Restored);
        cx.emit(SettingsEvent::AiAccessChanged);
    }

    /// After an export (the export dialog).
    pub fn exported(&mut self, rows: usize, path: PathBuf, cx: &mut Context<Self>) {
        self.notice = Some(Notice {
            message: NoticeMessage::Exported {
                rows,
                file: path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            },
            reveal: Some(path),
        });
        cx.notify();
    }

    fn fail(&mut self, error: SharedString, cx: &mut Context<Self>) {
        self.notice = Some(Notice {
            message: NoticeMessage::Failed(error),
            reveal: None,
        });
        cx.notify();
    }

    /// Shows `notice` in the notification bar (snapshots of the states
    /// board).
    pub fn set_notice(&mut self, notice: Option<Notice>, cx: &mut Context<Self>) {
        self.notice = notice;
        cx.notify();
    }

    pub fn dismiss_notice(&mut self, cx: &mut Context<Self>) {
        self.notice = None;
        cx.notify();
    }

    /// The notification bar, centred 24px above the canvas's foot.
    fn render_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let notice = self.notice.as_ref()?;
        let bar = table::notice_bar(notice, cx);
        Some(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(spacing::S6)
                .flex()
                .justify_center()
                .child(bar)
                .into_any_element(),
        )
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let canvas = match self.section {
            Section::Clients => self.render_clients(window, cx),
            Section::Log => self.render_log(window, cx),
            Section::Backups => self.render_backups(window, cx),
        };
        let inspector = match self.section {
            Section::Clients => self.render_client_inspector(window, cx),
            Section::Log => self.render_call_inspector(cx),
            Section::Backups => self.render_backup_inspector(cx),
        };
        let notice = self.render_notice(cx);
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(self.render_panel(window, cx))
            .child(
                div()
                    .id("settings-canvas")
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .bg(color::CANVAS)
                    .child(canvas)
                    .when_some(notice, |this, notice| this.child(notice)),
            )
            .children(inspector)
    }
}

impl Period {
    /// The period's key as a menu option key.
    pub fn option_key(self) -> SharedString {
        self.key().into()
    }
}

#[cfg(test)]
mod tests;
