//! Importing a statement (ROADMAP M3.4, M3.5; `docs/DESIGN.md` → "Import";
//! `docs/ARCHITECTURE.md` → "Decision record: the import flow").
//!
//! - [`dialog`]: the Import dialog, File → Map → Review (Paper `5ZQ-0`,
//!   `6J4-0`), with "Save as preset…" on the Map step (M3.5).
//! - [`session`]: one file being imported, in memory: how it's read, the
//!   preset and account, the column mapping, the statement.
//! - [`store`]: the database and the disk, called on a background thread.
//! - [`text`]: what the dialog says.
//! - [`fixture`]: a synthetic Monzo-style export for snapshots and tests.
//!
//! [`Imports`] outlives the dialog: it holds the notification after an
//! import ("Imported 296 transactions from monzo-november.csv", with Undo),
//! and tells the shell when the database changed so the accounts panel and
//! Transactions read it again.

pub mod dialog;
pub mod fixture;
pub mod session;
pub mod store;
pub mod text;

use coffer_core::id::BatchId;
use gpui_kit::{AppContext as _, Context, EventEmitter, SharedString, Task};

use store::ImportStore;
use text::Seg;

/// The database changed: an import was committed or undone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportsChanged;

/// What the notification bar says after an import.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportNotice {
    /// A batch was committed; Undo removes it.
    Imported { batch: BatchId, message: Vec<Seg> },
    /// Undo worked.
    Undone { message: Vec<Seg> },
    /// Undo didn't: why (core's message, never a row's text).
    Failed(SharedString),
}

/// Where imports go, and the notification after one.
pub struct Imports {
    store: Option<ImportStore>,
    notice: Option<ImportNotice>,
    /// The file the last import came from, for the undo notice.
    file: String,
    task: Option<Task<()>>,
}

impl EventEmitter<ImportsChanged> for Imports {}

impl Imports {
    /// Imports into the database `store` opens.
    pub fn from_store(store: ImportStore) -> Self {
        Self {
            store: Some(store),
            notice: None,
            file: String::new(),
            task: None,
        }
    }

    /// No database (sample data, snapshots): the dialog shows the sample
    /// file and can't import.
    pub fn sample() -> Self {
        Self {
            store: None,
            notice: None,
            file: String::new(),
            task: None,
        }
    }

    /// The store, when there's a database.
    pub fn store(&self) -> Option<&ImportStore> {
        self.store.as_ref()
    }

    pub fn notice(&self) -> Option<&ImportNotice> {
        self.notice.as_ref()
    }

    /// A batch was committed from `file`: show it, and have the views read
    /// the database again.
    pub fn imported(
        &mut self,
        batch: BatchId,
        transactions: usize,
        file: &str,
        cx: &mut Context<Self>,
    ) {
        self.file = file.to_owned();
        self.notice = Some(ImportNotice::Imported {
            batch,
            message: text::imported(transactions, file),
        });
        cx.emit(ImportsChanged);
        cx.notify();
    }

    /// Undo on the notification: removes the batch, on a background thread.
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        let (Some(store), Some(ImportNotice::Imported { batch, .. })) =
            (self.store.clone(), self.notice.as_ref())
        else {
            return;
        };
        let batch = *batch;
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { store.undo(batch) })
                .await
                .map_err(|error| SharedString::from(session::sentence(&error.to_string())));
            let _ = this.update(cx, |this, cx| {
                this.notice = Some(match result {
                    Ok(()) => {
                        cx.emit(ImportsChanged);
                        ImportNotice::Undone {
                            message: text::undone(&this.file),
                        }
                    }
                    Err(error) => ImportNotice::Failed(error),
                });
                cx.notify();
            });
        }));
    }

    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.notice = None;
        cx.notify();
    }

    /// Sets the notice directly (snapshots).
    pub fn set_notice(&mut self, notice: Option<ImportNotice>, cx: &mut Context<Self>) {
        self.notice = notice;
        cx.notify();
    }
}

/// The notification bar for `imports`' notice, centred 24px above the
/// bottom of the area it's drawn over (`ui::notification`).
pub fn notice_layer(
    imports: &gpui_kit::Entity<Imports>,
    cx: &mut gpui_kit::App,
) -> Option<gpui_kit::AnyElement> {
    use gpui_kit::prelude::FluentBuilder as _;
    use gpui_kit::{IntoElement as _, ParentElement as _, Styled as _, div};

    use crate::theme::tokens::{color, spacing, typography};
    use crate::ui::button::{Button, ButtonVariant};
    use crate::ui::notification::NotificationBar;

    let notice = imports.read(cx).notice()?.clone();
    let (message, failed, undo) = match &notice {
        ImportNotice::Imported { message, .. } => (message.clone(), false, true),
        ImportNotice::Undone { message } => (message.clone(), false, false),
        ImportNotice::Failed(error) => (
            vec![Seg {
                text: error.to_string(),
                mono: false,
            }],
            true,
            false,
        ),
    };
    let base = if failed { color::DANGER } else { color::TEXT };
    let message = dialog::rich_segs(typography::BODY, base, &message);
    let undo_target = imports.downgrade();
    let dismiss_target = imports.downgrade();
    let bar = NotificationBar::new(message)
        .failed(failed)
        .when(undo, |bar| {
            bar.action(
                Button::new("import-undo", "Undo")
                    .with_variant(ButtonVariant::Outline)
                    .on_click(move |_, _, cx| {
                        let _ = undo_target.update(cx, |imports, cx| imports.undo(cx));
                    }),
            )
        })
        .on_dismiss(move |_, _, cx| {
            let _ = dismiss_target.update(cx, |imports, cx| imports.dismiss(cx));
        });
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
