//! Settings · AI clients (ROADMAP M3.15; `docs/DESIGN.md` → "Settings" →
//! **AI clients**; Paper `AQ2-0`, the off state `GR9-0`).
//!
//! The toolbar's kill switch turns AI access off for every client. Below:
//! the clients the user enabled (level, last call, calls in 7 days, an ON
//! toggle that keeps the level), the clients that asked to connect (Ignore,
//! Enable…), and what each level shares. The inspector shows the selected
//! client: its level (a select while it's on), the command that connects
//! it, what it did in 7 days, and Remove client….
//!
//! Composed for M3.15 (decided with the user; not drawn):
//! - clients have no display name: the `--client` name, in Geist Mono, is
//!   the name everywhere;
//! - nothing records whether a client is connected right now, so LAST
//!   CONNECTED is always the last call's time and there's no status dot;
//! - with no clients, the Clients group says how one gets here; a read
//!   that failed says why in `danger`;
//! - a client turned off shows its kept level read-only in the inspector,
//!   with a caption to turn it on to change it;
//! - while AI access is off the clients stay usable, faded as drawn.

use chrono::NaiveDateTime;
use coffer_core::access_log::Outcome;
use coffer_core::mcp_client::{AccessLevel, ClientName};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::base::actions::{SelectDown, SelectUp};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, ClipboardItem, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
};

use crate::theme::tokens::{color, opacity, radius, size, spacing, typography, weight};
use crate::ui::button::{Button, ButtonVariant, IconButton, IconButtonSize};
use crate::ui::field::{FieldRow, LockedValue, SelectField};
use crate::ui::icon::Glyph;
use crate::ui::menu::{MenuGroup, MenuOption, Select};
use crate::ui::text::TextRoleExt as _;
use crate::ui::toggle::Toggle;

use super::SettingsView;
use super::data::{ClientRow, RequestRow};
use super::table::{
    self, LEAD, caption, cell, column_label, count, group, group_title, mono, on_lane, plain,
    plural, rich, row, section, selectable_row, toolbar, value_row,
};

/// What a level is called.
pub fn level_label(level: AccessLevel) -> &'static str {
    match level {
        AccessLevel::Aggregate => "Aggregate",
        AccessLevel::Pseudonymous => "Pseudonymous",
        AccessLevel::Standard => "Standard",
    }
}

/// A level in a few words, after its name in the Level select ("Aggregate ·
/// totals only", Paper `GZI-0`).
pub fn level_hint(level: AccessLevel) -> &'static str {
    match level {
        AccessLevel::Aggregate => "totals only",
        AccessLevel::Pseudonymous => "merchant tokens",
        AccessLevel::Standard => "merchant names",
    }
}

/// What a level lets a client read (Paper `D8F-0`, `GZM-0`).
pub fn level_reads(level: AccessLevel) -> &'static str {
    match level {
        AccessLevel::Aggregate => {
            "Totals by month and category, budgets and net worth. No transactions, so no merchants."
        }
        AccessLevel::Pseudonymous => {
            "Transactions, with each merchant replaced by a token such as M-7f3a."
        }
        AccessLevel::Standard => "Transactions with merchant names and public tags.",
    }
}

/// What a level lets a client write.
pub fn level_writes(level: AccessLevel) -> &'static str {
    match level {
        AccessLevel::Aggregate => "It can save reviews but not suggest changes.",
        AccessLevel::Pseudonymous | AccessLevel::Standard => {
            "It can suggest changes in the Inbox and save reviews."
        }
    }
}

/// The Level select's options.
pub fn level_groups() -> Vec<MenuGroup> {
    vec![MenuGroup::new(
        None,
        AccessLevel::ALL
            .iter()
            .map(|level| {
                MenuOption::new(level.as_str(), level_label(*level)).hint(level_hint(*level))
            })
            .collect(),
    )]
}

/// When something last happened: `Today, 10:41`, `3 Nov, 21:40`, or with
/// the year when it isn't this one.
pub fn when(time: NaiveDateTime, today: chrono::NaiveDate) -> String {
    use chrono::Datelike as _;
    if time.date() == today {
        time.format("Today, %H:%M").to_string()
    } else if time.year() == today.year() {
        time.format("%-d %b, %H:%M").to_string()
    } else {
        time.format("%-d %b %Y, %H:%M").to_string()
    }
}

/// The words after a client's refused count: why they were refused, when
/// every refusal had the same reason.
fn refused_reason(row: &ClientRow) -> Option<&'static str> {
    let mut reasons = row.week.refused.keys();
    let first = *reasons.next()?;
    if reasons.next().is_some() {
        return None;
    }
    Some(match first {
        Outcome::RefusedOff => " while AI access was off",
        Outcome::RefusedNotEnabled => " while it was turned off",
        Outcome::RefusedLevel => " above its level",
        Outcome::RefusedRestoring => " while a backup was restored",
        Outcome::RefusedUnavailable => " while the database couldn't be opened",
        Outcome::Ok | Outcome::Error => return None,
    })
}

/// The command a client's configuration runs.
pub fn command(name: &ClientName) -> String {
    format!("coffer mcp --client {name}")
}

/// The neutral banner while AI access is off (Paper `GS7-0`).
fn off_banner(cx: &mut Context<SettingsView>) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_start()
        .gap(spacing::S2_5)
        .p(spacing::S3)
        .rounded(radius::SURFACE)
        .border(size::HAIRLINE)
        .border_color(color::HAIRLINE)
        .child(
            Glyph::Refused
                .render_at(size::ICON_MD)
                .text_color(color::TEXT_SECONDARY),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(spacing::S1)
                .child(
                    div()
                        .text_role(typography::SECTION_TITLE)
                        .text_color(color::TEXT_STRONG)
                        .child("AI access is off"),
                )
                .child(caption(
                    "Every client is refused before any tool runs, and each refusal is logged. \
                     Levels, the log, suggestions and reviews are kept. Turn it back on and \
                     enabled clients can connect again.",
                )),
        )
        .child(
            Button::new("ai-access-turn-on", "Turn on")
                .on_click(cx.listener(|this, _, _, cx| this.set_ai_on(true, cx))),
        )
}

impl SettingsView {
    fn client_row(&self, client: &ClientRow, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.selected_client.as_ref() == Some(&client.name);
        let on = client.enabled;
        let value = if on { color::TEXT } else { color::TEXT_MUTED };
        let name = client.name.clone();
        let toggled = client.name.clone();
        selectable_row(format!("client-{}", client.name), selected)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.list_focus.focus(window, cx);
                this.select_client(name.clone(), cx)
            }))
            .child(
                cell(None)
                    .gap(spacing::S2)
                    .child(div().flex_none().w(LEAD))
                    .child(
                        div()
                            .min_w_0()
                            .text_ellipsis()
                            .text_role(typography::MONO_AMOUNT)
                            .text_color(if on {
                                color::TEXT
                            } else {
                                color::TEXT_SECONDARY
                            })
                            .child(client.name.to_string()),
                    ),
            )
            .child(
                cell(Some(size::COLUMN_CATEGORY))
                    .text_role(typography::BODY)
                    .text_color(value)
                    .child(level_label(client.level)),
            )
            .child(
                cell(Some(size::COLUMN_ACCOUNT)).child(match client.last_seen {
                    Some(time) => div()
                        .text_role(typography::MONO_VALUE)
                        .text_color(if on {
                            color::TEXT_SECONDARY
                        } else {
                            color::TEXT_MUTED
                        })
                        .child(when(time, self.clock.today)),
                    None => div()
                        .text_role(typography::BODY)
                        .text_color(color::TEXT_MUTED)
                        .child("Never"),
                }),
            )
            .child(
                cell(Some(size::COLUMN_AMOUNT))
                    .justify_end()
                    .text_role(typography::MONO_AMOUNT)
                    .text_color(value)
                    .child(count(client.week.calls)),
            )
            .child(
                cell(Some(on_lane())).justify_end().child(
                    Toggle::new(
                        format!("client-on-{}", client.name),
                        on,
                        format!("Allow {}", client.name),
                    )
                    .on_change(cx.listener(move |this, next: &bool, _, cx| {
                        this.turn_client(toggled.clone(), *next, cx)
                    })),
                ),
            )
            .test_support()
            .into_any_element()
    }

    fn request_rows(&self, request: &RequestRow, cx: &mut Context<Self>) -> [AnyElement; 2] {
        let (ignore, enable) = (request.name.clone(), request.name.clone());
        let line = row()
            .child(
                cell(None)
                    .gap(spacing::S2)
                    .child(div().flex_none().w(LEAD))
                    .child(
                        div()
                            .text_role(typography::MONO_AMOUNT)
                            .text_color(color::TEXT)
                            .child(request.name.to_string()),
                    ),
            )
            .child(
                cell(Some(size::COLUMN_CATEGORY))
                    .text_role(typography::BODY)
                    .text_color(color::TEXT_MUTED)
                    .child("Not enabled"),
            )
            .child(
                cell(Some(size::COLUMN_ACCOUNT))
                    .text_role(typography::MONO_VALUE)
                    .text_color(color::TEXT_SECONDARY)
                    .child(
                        request
                            .last_tried
                            .map_or_else(|| "—".to_owned(), |time| when(time, self.clock.today)),
                    ),
            )
            .child(
                cell(Some(size::COLUMN_AMOUNT))
                    .justify_end()
                    .text_role(typography::MONO_AMOUNT)
                    .text_color(color::TEXT)
                    .child(count(request.refused)),
            )
            .child(div().flex_none().w(on_lane()));
        let foot = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S2)
            .pt(spacing::S1)
            .pr(spacing::S2)
            .pl(spacing::S2 + LEAD + spacing::S2)
            .child(
                rich(
                    typography::CAPTION,
                    color::TEXT_MUTED,
                    vec![
                        plain("Its configuration runs "),
                        mono(command(&request.name)).color(color::TEXT_SECONDARY),
                        plain(
                            ". It was refused before any tool ran, and stays refused until you \
                             enable it.",
                        ),
                    ],
                )
                .flex_1()
                .min_w_0(),
            )
            .child(
                Button::new(format!("ignore-{}", request.name), "Ignore")
                    .on_click(cx.listener(move |this, _, _, cx| this.ignore(ignore.clone(), cx))),
            )
            .child(
                Button::new(format!("enable-{}", request.name), "Enable…").on_click(
                    cx.listener(move |this, _, _, cx| this.request_enable(enable.clone(), cx)),
                ),
            );
        [line.into_any_element(), foot.into_any_element()]
    }

    pub(super) fn render_clients(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let on = self.ai.on;
        let kill_switch = div()
            .flex()
            .items_center()
            .gap(spacing::S2)
            .child(
                div()
                    .text_role(crate::theme::tokens::typography::TextRole {
                        weight: weight::MEDIUM,
                        ..typography::BODY
                    })
                    .text_color(color::TEXT)
                    .child("AI access"),
            )
            .child(
                Toggle::new("ai-access", on, "AI access")
                    .on_change(cx.listener(|this, next: &bool, _, cx| this.set_ai_on(*next, cx))),
            );

        let clients_header = row()
            .child(group_title(
                "Clients",
                Some(self.ai.clients.len().to_string()),
                None,
            ))
            .child(column_label("Access", Some(size::COLUMN_CATEGORY), false))
            .child(column_label(
                "Last connected",
                Some(size::COLUMN_ACCOUNT),
                false,
            ))
            .child(column_label(
                "Calls · 7 days",
                Some(size::COLUMN_AMOUNT),
                true,
            ))
            .child(column_label("On", Some(on_lane()), true));
        let rows: Vec<AnyElement> = self
            .ai
            .clients
            .clone()
            .iter()
            .map(|client| self.client_row(client, cx))
            .collect();
        let clients = group(false)
            .when(!on, |this| this.opacity(opacity::DISABLED))
            .child(clients_header)
            .children(rows)
            .when(self.ai.clients.is_empty() && self.ai_error.is_none(), |this| {
                this.child(
                    div()
                        .px(spacing::S2)
                        .pl(spacing::S2 + LEAD + spacing::S2)
                        .py(spacing::S1)
                        .child(rich(
                            typography::CAPTION,
                            color::TEXT_MUTED,
                            vec![
                                plain("No client is enabled yet. A client asks to connect the first time it runs "),
                                mono("coffer mcp --client <name>").color(color::TEXT_SECONDARY),
                                plain(", and is listed below until you enable or ignore it."),
                            ],
                        )),
                )
            });

        let requests = (!self.ai.requests.is_empty()).then(|| {
            let header = row()
                .child(group_title(
                    "Asked to connect",
                    Some(self.ai.requests.len().to_string()),
                    None,
                ))
                .child(column_label("Access", Some(size::COLUMN_CATEGORY), false))
                .child(column_label(
                    "Last tried",
                    Some(size::COLUMN_ACCOUNT),
                    false,
                ))
                .child(column_label("Refused", Some(size::COLUMN_AMOUNT), true))
                .child(div().flex_none().w(on_lane()));
            let rows: Vec<AnyElement> = self
                .ai
                .requests
                .clone()
                .iter()
                .flat_map(|request| self.request_rows(request, cx))
                .collect();
            group(true).child(header).children(rows)
        });

        let level_name_lane = LEAD + spacing::S2 + size::COLUMN_ACCOUNT;
        let level_rows = [
            (
                AccessLevel::Aggregate,
                vec![plain(
                    "Totals by month and category, budgets, net worth. No transactions",
                )],
                "Reviews",
            ),
            (
                AccessLevel::Pseudonymous,
                vec![
                    plain("Transactions, with each merchant replaced by a token such as "),
                    mono("M-7f3a"),
                ],
                "Inbox suggestions, reviews",
            ),
            (
                AccessLevel::Standard,
                vec![plain(
                    "Transactions with merchant names, tags you haven't made private",
                )],
                "Inbox suggestions, reviews",
            ),
        ]
        .into_iter()
        .map(|(level, reads, writes)| {
            row()
                .child(
                    cell(Some(level_name_lane))
                        .gap(spacing::S2)
                        .child(div().flex_none().w(LEAD))
                        .child(
                            div()
                                .text_role(typography::BODY)
                                .text_color(color::TEXT)
                                .child(level_label(level)),
                        ),
                )
                .child(cell(None).child(rich(typography::BODY, color::TEXT_SECONDARY, reads)))
                .child(
                    cell(Some(size::COLUMN_CATEGORY))
                        .text_role(typography::BODY)
                        .text_color(color::TEXT_SECONDARY)
                        .child(writes),
                )
        });
        let levels = group(true)
            .child(
                row()
                    .child(group_title(
                        "What each level shares",
                        None,
                        Some(level_name_lane),
                    ))
                    .child(column_label("Can read", None, false))
                    .child(column_label(
                        "Can write",
                        Some(size::COLUMN_CATEGORY),
                        false,
                    )),
            )
            .children(level_rows)
            .child(
                div()
                    .pt(spacing::S2)
                    .pr(spacing::S2)
                    .pl(spacing::S2 + LEAD + spacing::S2)
                    .child(rich(
                        typography::CAPTION,
                        color::TEXT_MUTED,
                        vec![
                            plain("Never shared at any level:")
                                .color(color::TEXT_SECONDARY)
                                .weight(weight::MEDIUM),
                            plain(
                                " account numbers, sort codes, names and addresses, your notes, \
                                 share labels and private tags. Bank text reaches a standard \
                                 client only filtered and labelled as untrusted. A tool above a \
                                 client's level isn't offered to it at all.",
                            ),
                        ],
                    )),
            );

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(toolbar(
                "AI clients",
                "Each client sees only what its access level allows · nothing is shared until you enable a client",
                kill_switch,
            ))
            .child(
                table::body("clients-body")
                    .key_context(table::LIST_CONTEXT)
                    .track_focus(&self.list_focus.clone().tab_stop(true))
                    .on_action(cx.listener(|this, _: &SelectUp, _, cx| this.select_next(-1, cx)))
                    .on_action(cx.listener(|this, _: &SelectDown, _, cx| this.select_next(1, cx)))
                    .when(!on, |this| {
                        this.child(div().flex_none().pt(spacing::S3).child(off_banner(cx)))
                    })
                    .when_some(self.ai_error.clone(), |this, error| {
                        this.child(
                            div()
                                .pt(spacing::S3)
                                .px(spacing::S2)
                                .text_role(typography::CAPTION)
                                .text_color(color::DANGER)
                                .child(error),
                        )
                    })
                    .child(clients)
                    .children(requests)
                    .child(levels),
            )
            .into_any_element()
    }

    fn level_select(&self, client: &ClientRow, cx: &mut Context<Self>) -> Select {
        let view = cx.entity().downgrade();
        let (choose, step) = (view.clone(), view.clone());
        let chosen: SharedString = client.level.as_str().into();
        let open_chosen = chosen.clone();
        let name = client.name.clone();
        let groups = level_groups();
        let moved = groups.clone();
        Select::new(
            "client-level",
            "Level",
            SelectField::new("client-level-field", level_label(client.level)),
            groups,
            Some(chosen),
            &self.level_select,
        )
        .menu_width(size::INSPECTOR - spacing::S3_5 * 2. - size::FIELD_LABEL - spacing::S2)
        .on_toggle(move |open, window, cx| {
            let _ = view.update(cx, |this, cx| {
                this.level_select
                    .set_open(open, Some(&open_chosen), window, cx);
                cx.notify();
            });
        })
        .on_choose(move |key, window, cx| {
            let _ = choose.update(cx, |this, cx| {
                this.level_select.set_open(false, None, window, cx);
                if let Ok(level) = key.parse::<AccessLevel>() {
                    this.set_level(name.clone(), level, cx);
                }
                cx.notify();
            });
        })
        .on_move(move |delta, _, cx| {
            let _ = step.update(cx, |this, cx| {
                this.level_select.move_highlight(&moved, delta);
                cx.notify();
            });
        })
    }

    pub(super) fn render_client_inspector(
        &mut self,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let client = self
            .selected_client
            .as_ref()
            .and_then(|name| self.ai.client(name))?
            .clone();
        let week = &client.week;
        let header = table::inspector_header(
            table::mono_title(client.name.to_string()),
            Some(count(week.calls)),
            rich(
                typography::CAPTION,
                color::TEXT_MUTED,
                vec![
                    plain("Tool calls in 7 days · "),
                    mono(count(week.read_rows)),
                    plain(" rows returned"),
                ],
            )
            .into_any_element(),
        );

        let level = if client.enabled {
            FieldRow::new("Level", self.level_select(&client, cx)).into_any_element()
        } else {
            FieldRow::new("Level", LockedValue::new(level_label(client.level))).into_any_element()
        };
        let access = section(Some("Access"))
            .child(level)
            .child(caption(if client.enabled {
                format!(
                    "{} A lower level takes effect on the client's next call.",
                    level_reads(client.level)
                )
            } else {
                format!(
                    "{} It's turned off, so it's refused; turn it on to change its level.",
                    level_reads(client.level)
                )
            }));

        let command_text = command(&client.name);
        let copied = command_text.clone();
        let connect = section(Some("Connect"))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(spacing::S2)
                    .h(size::CONTROL)
                    .pl(spacing::S2)
                    .pr(spacing::S0_5)
                    .rounded(radius::MD)
                    .bg(color::FIELD)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_role(typography::MONO_VALUE)
                            .text_color(color::TEXT)
                            .child(command_text),
                    )
                    .child(
                        IconButton::new("copy-command", Glyph::Copy, "Copy the command")
                            .with_size(IconButtonSize::Small)
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()))
                            }),
                    ),
            )
            .child(rich(
                typography::CAPTION,
                color::TEXT_MUTED,
                vec![
                    plain("The command in the client's MCP configuration. The name after "),
                    mono("--client"),
                    plain(
                        " is how Coffer tells clients apart. It isn't a password: any program on \
                         this computer can use it.",
                    ),
                ],
            ));

        let muted = |text: &str| plain(text.to_owned()).color(color::TEXT_MUTED);
        let refused = week.refused_total();
        let mut refused_runs = vec![
            mono(count(refused)),
            plain(format!(" {}", plural(refused, "call"))),
        ];
        if refused > 0
            && let Some(reason) = refused_reason(&client)
        {
            refused_runs.push(muted(reason));
        }
        let open_log = client.name.to_string();
        let last_week =
            section(Some("Last 7 days"))
                .child(value_row(
                    "Read",
                    rich(
                        typography::BODY,
                        color::TEXT,
                        vec![
                            mono(count(week.reads)),
                            plain(format!(" {} ", plural(week.reads, "call"))),
                            muted("·"),
                            plain(" "),
                            mono(count(week.read_rows)),
                            plain(format!(" {}", plural(week.read_rows, "row"))),
                        ],
                    ),
                ))
                .child(value_row(
                    "Wrote",
                    rich(
                        typography::BODY,
                        color::TEXT,
                        vec![
                            mono(count(client.written.suggestions)),
                            plain(format!(
                                " {} ",
                                plural(client.written.suggestions, "suggestion")
                            )),
                            muted("·"),
                            plain(" "),
                            mono(count(client.written.reports)),
                            plain(format!(" {}", plural(client.written.reports, "review"))),
                        ],
                    ),
                ))
                .child(value_row(
                    "Refused",
                    rich(typography::BODY, color::TEXT, refused_runs),
                ))
                .child(div().flex().pt(spacing::S1).child(
                    Button::new("open-access-log", "Open access log").on_click(cx.listener(
                        move |this, _, _, cx| this.open_log_for(Some(open_log.clone()), cx),
                    )),
                ));

        let remove = client.name.clone();
        let foot = section(None)
            .child(caption(
                "Turning a client off refuses its next call and keeps its level. Removing it also \
                 forgets the level; its log, suggestions and reviews stay.",
            ))
            .child(
                div().flex().child(
                    Button::new("remove-client", "Remove client…")
                        .with_variant(ButtonVariant::DestructiveCompact)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.request_remove(remove.clone(), cx)
                        })),
                ),
            );

        Some(
            table::inspector("client-inspector")
                .child(header)
                .child(access)
                .child(connect)
                .child(last_week)
                .child(div().flex_1())
                .child(foot)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    #[test]
    fn times_read_relative_to_today() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        let at = |date: &str| NaiveDateTime::parse_from_str(date, "%Y-%m-%d %H:%M").unwrap();
        assert_eq!(when(at("2026-11-10 10:41"), today), "Today, 10:41");
        assert_eq!(when(at("2026-11-03 21:40"), today), "3 Nov, 21:40");
        assert_eq!(when(at("2025-12-31 08:15"), today), "31 Dec 2025, 08:15");
    }

    #[test]
    fn every_level_says_what_it_shares() {
        for level in AccessLevel::ALL {
            assert!(level_reads(*level).ends_with('.'));
            assert!(level_writes(*level).ends_with('.'));
        }
        assert_eq!(
            command(&ClientName::new("my-agent").unwrap()),
            "coffer mcp --client my-agent"
        );
    }
}
