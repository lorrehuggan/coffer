//! The Foundations board (Paper `1-0`): colour swatches (`9-0`) and the
//! type scale (`6S-0`).

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, FontWeight, IntoElement, ParentElement as _, Rgba, SharedString, Styled as _, div,
};

use super::{board as artboard, section, spec};
use crate::theme::tokens::typography::{self, TextRole};
use crate::theme::tokens::{color, radius, size, spacing, weight};
use crate::ui::text::{CasedText as _, TextRoleExt as _};

pub(super) fn board() -> gpui_kit::Div {
    artboard(
        "Coffer · Design tokens",
        "Foundations",
        "Colour and type. Every colour, size, weight and gap on this board is a token.",
        [
            section("Colour", colours()),
            section("Type scale", type_scale()),
        ],
    )
}

// Colour.

/// What a swatch row shows.
#[derive(Clone, Copy, Debug)]
enum SwatchKind {
    /// The token's own colour, labelled with its hex.
    Fill,
    /// A token that aliases another, labelled "→ other".
    Alias(&'static str),
    /// The chart's "Other" series: a `text-muted` outline on `field`.
    ChartOther,
}

#[derive(Clone, Copy, Debug)]
struct Swatch {
    /// The token name without `--color-`.
    name: &'static str,
    color: Rgba,
    kind: SwatchKind,
}

const fn fill(name: &'static str, color: Rgba) -> Swatch {
    Swatch {
        name,
        color,
        kind: SwatchKind::Fill,
    }
}

struct Group {
    title: &'static str,
    swatches: &'static [Swatch],
}

/// The swatch board's columns, top to bottom, in Paper's order.
const COLUMNS: &[&[Group]] = &[
    &[
        Group {
            title: "Surfaces",
            swatches: &[
                fill("chrome", color::CHROME),
                fill("float", color::FLOAT),
                fill("canvas", color::CANVAS),
                fill("raised", color::RAISED),
                fill("field", color::FIELD),
                fill("hover", color::HOVER),
                fill("control-active", color::CONTROL_ACTIVE),
                fill("avatar", color::AVATAR),
            ],
        },
        Group {
            title: "Lines",
            swatches: &[
                fill("hairline", color::HAIRLINE),
                fill("border", color::BORDER),
                fill("border-strong", color::BORDER_STRONG),
                fill("tooltip-border", color::TOOLTIP_BORDER),
            ],
        },
    ],
    &[Group {
        title: "Text and icons",
        swatches: &[
            fill("white", color::WHITE),
            fill("text", color::TEXT),
            fill("text-strong", color::TEXT_STRONG),
            fill("text-secondary", color::TEXT_SECONDARY),
            fill("text-muted", color::TEXT_MUTED),
            fill("icon-faint", color::ICON_FAINT),
            fill("text-faint", color::TEXT_FAINT),
            fill("icon-disabled", color::ICON_DISABLED),
            Swatch {
                name: "on-light",
                color: color::ON_LIGHT,
                kind: SwatchKind::Alias("chrome"),
            },
        ],
    }],
    &[
        Group {
            title: "Molten · selection, focus",
            swatches: &[
                fill("molten", color::MOLTEN),
                fill("molten-soft", color::MOLTEN_SOFT),
            ],
        },
        Group {
            title: "Status",
            swatches: &[
                fill("danger", color::DANGER),
                fill("danger-hover", color::DANGER_HOVER),
                fill("danger-pressed", color::DANGER_PRESSED),
                fill("danger-soft", color::DANGER_SOFT),
                fill("on-danger", color::ON_DANGER),
                fill("warning", color::WARNING),
                fill("warning-soft", color::WARNING_SOFT),
                fill("on-warning", color::ON_WARNING),
            ],
        },
    ],
    &[Group {
        title: "Moldavite · the AI agent",
        swatches: &[
            fill("moldavite", color::MOLDAVITE),
            fill("moldavite-hover", color::MOLDAVITE_HOVER),
            fill("moldavite-pressed", color::MOLDAVITE_PRESSED),
            fill("moldavite-soft", color::MOLDAVITE_SOFT),
            fill("on-moldavite", color::ON_MOLDAVITE),
            fill("moldavite-text", color::MOLDAVITE_TEXT),
            fill("moldavite-text-muted", color::MOLDAVITE_TEXT_MUTED),
            fill("moldavite-text-faint", color::MOLDAVITE_TEXT_FAINT),
            fill("moldavite-surface", color::MOLDAVITE_SURFACE),
            fill("moldavite-surface-raised", color::MOLDAVITE_SURFACE_RAISED),
            fill("moldavite-well", color::MOLDAVITE_WELL),
            fill("moldavite-border", color::MOLDAVITE_BORDER),
            fill("moldavite-border-strong", color::MOLDAVITE_BORDER_STRONG),
            fill("moldavite-track", color::MOLDAVITE_TRACK),
        ],
    }],
    &[Group {
        title: "Chart series",
        swatches: &[
            fill("chart-1", color::CHART_1),
            fill("chart-2", color::CHART_2),
            fill("chart-3", color::CHART_3),
            fill("chart-4", color::CHART_4),
            fill("chart-5", color::CHART_5),
            fill("chart-6", color::CHART_6),
            Swatch {
                name: "Other",
                color: color::FIELD,
                kind: SwatchKind::ChartOther,
            },
        ],
    }],
];

/// `color` as `#RRGGBB`, or `#RRGGBBAA` when it's translucent.
fn hex(color: Rgba) -> String {
    let byte = |channel: f32| (channel * 255.).round() as u8;
    let rgb = format!(
        "#{:02X}{:02X}{:02X}",
        byte(color.r),
        byte(color.g),
        byte(color.b)
    );
    if color.a < 1. {
        format!("{rgb}{:02X}", byte(color.a))
    } else {
        rgb
    }
}

/// Swatch name: body type at 500.
const SWATCH_NAME: TextRole = TextRole {
    weight: weight::MEDIUM,
    ..typography::BODY
};

fn swatch_row(swatch: &Swatch) -> impl IntoElement {
    let value: SharedString = match swatch.kind {
        SwatchKind::Fill => hex(swatch.color).into(),
        SwatchKind::Alias(target) => format!("→ {target}").into(),
        SwatchKind::ChartOther => "outline · field".into(),
    };
    let chip = div()
        .flex_none()
        .size(spacing::S6)
        .rounded(radius::SM)
        .bg(swatch.color)
        .border(size::HAIRLINE)
        .map(|this| match swatch.kind {
            SwatchKind::ChartOther => this.border_color(color::TEXT_MUTED),
            _ => this.border_color(color::SWATCH_RING),
        });
    div()
        .flex()
        .items_center()
        .gap(spacing::S2)
        .child(chip)
        .child(
            div()
                .flex_1()
                .text_role(SWATCH_NAME)
                .text_color(color::TEXT)
                .child(swatch.name),
        )
        .child(
            div()
                .text_role(typography::MONO_VALUE)
                .text_color(color::TEXT_MUTED)
                .child(value),
        )
}

fn colours() -> impl IntoElement {
    super::groups().children(COLUMNS.iter().map(|column| {
        div()
            .flex()
            .flex_col()
            .gap(spacing::S6)
            .w(size::PANEL)
            .children(column.iter().map(|group| {
                super::group(
                    group.title,
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::S2)
                        .children(group.swatches.iter().map(swatch_row)),
                )
            }))
    }))
}

// Type.

/// One sample in a type-scale row.
struct Sample {
    text: &'static str,
    color: Rgba,
    /// Replaces the role's weight, where a role spans weights.
    weight: Option<FontWeight>,
}

const fn sample(text: &'static str, color: Rgba) -> Sample {
    Sample {
        text,
        color,
        weight: None,
    }
}

struct TypeRow {
    name: &'static str,
    spec: &'static str,
    role: TextRole,
    samples: &'static [Sample],
}

/// The type scale's rows in Paper's order, with its sample copy.
const TYPE_ROWS: &[TypeRow] = &[
    TypeRow {
        name: "Display",
        spec: "ui · 2xl/display · strong · display",
        role: typography::DISPLAY,
        samples: &[sample("Where did October go?", color::TEXT)],
    },
    TypeRow {
        name: "Display figure",
        spec: "mono · 2xl/display · strong · display",
        role: typography::DISPLAY_FIGURE,
        samples: &[sample("£2,481.90", color::TEXT)],
    },
    TypeRow {
        name: "Figure",
        spec: "mono · xl/figure · strong · tight",
        role: typography::FIGURE,
        samples: &[sample("£3,950.00", color::TEXT)],
    },
    TypeRow {
        name: "Reading heading",
        spec: "ui · lg/reading · strong · tight",
        role: typography::READING_HEADING,
        samples: &[sample("Groceries ran 18% over budget", color::TEXT)],
    },
    TypeRow {
        name: "Reading body",
        spec: "ui · md/reading · regular",
        role: typography::READING_BODY,
        samples: &[sample(
            "Most of the overspend came from three weekend shops in the last week. Eating out stayed flat.",
            color::TEXT,
        )],
    },
    TypeRow {
        name: "Title · card title",
        spec: "ui · md/ui · strong · tight (card: none)",
        role: typography::TITLE,
        samples: &[sample("Transactions", color::TEXT)],
    },
    TypeRow {
        name: "Section title",
        spec: "ui · sm/ui · strong",
        role: typography::SECTION_TITLE,
        samples: &[sample("Details", color::TEXT)],
    },
    TypeRow {
        name: "Body",
        spec: "ui · sm/ui · regular–medium",
        role: typography::BODY,
        samples: &[
            sample("Pret A Manger", color::TEXT),
            sample("Eating out", color::TEXT_SECONDARY),
            sample("Current account", color::TEXT_MUTED),
        ],
    },
    TypeRow {
        name: "Button · neutral, agent, outline",
        spec: "ui · sm/ui · strong · bold · medium",
        role: typography::BUTTON,
        samples: &[
            sample("Import statement", color::TEXT),
            Sample {
                text: "Review 12 suggestions",
                color: color::MOLDAVITE_TEXT,
                weight: Some(weight::BOLD),
            },
            Sample {
                text: "Cancel",
                color: color::TEXT_STRONG,
                weight: Some(weight::MEDIUM),
            },
        ],
    },
    TypeRow {
        name: "Caption",
        spec: "ui · xs/caption · regular–medium · muted",
        role: typography::CAPTION,
        samples: &[sample(
            "Imported 2 minutes ago from monzo-october.csv",
            color::TEXT_MUTED,
        )],
    },
    TypeRow {
        name: "Section label",
        spec: "ui · xs/caption · strong · caps · muted",
        role: typography::SECTION_LABEL,
        samples: &[sample("Accounts", color::TEXT_MUTED)],
    },
    TypeRow {
        name: "Mono value",
        spec: "mono · xs/caption · regular · text / muted",
        role: typography::MONO_VALUE,
        samples: &[sample("14 Oct 2026 · 128 transactions", color::TEXT)],
    },
    TypeRow {
        name: "Mono amount",
        spec: "mono · sm/ui · regular",
        role: typography::MONO_AMOUNT,
        samples: &[
            sample("−£12.40", color::TEXT),
            sample("+£2,150.00", color::TEXT),
            sample("−€38.00", color::TEXT),
        ],
    },
    TypeRow {
        name: "Mono micro",
        spec: "mono · 2xs/micro · regular–strong",
        role: typography::MONO_MICRO,
        samples: &[
            sample("⌘K", color::TEXT_SECONDARY),
            sample("⌘I", color::TEXT_SECONDARY),
            sample("Jan", color::TEXT_SECONDARY),
            sample("Feb", color::TEXT_SECONDARY),
            sample("Mar", color::TEXT_SECONDARY),
            sample("£0", color::TEXT_SECONDARY),
            sample("£500", color::TEXT_SECONDARY),
        ],
    },
    TypeRow {
        name: "Badge",
        spec: "ui · 2xs/micro · bold · moldavite",
        role: typography::BADGE,
        samples: &[sample("12", color::MOLDAVITE)],
    },
];

fn type_row(row: &TypeRow) -> impl IntoElement {
    // Amounts sit 16px apart, like the figures in a summary; other samples 12.
    let gap = if row.role == typography::MONO_AMOUNT {
        spacing::S4
    } else {
        spacing::S3
    };
    div()
        .flex()
        .items_baseline()
        .gap(spacing::S6)
        .child(spec(row.name, row.spec))
        .child(
            div()
                .flex()
                .flex_1()
                .flex_wrap()
                .gap(gap)
                .children(row.samples.iter().map(|sample| {
                    div()
                        .text_role(row.role)
                        .text_color(sample.color)
                        .when_some(sample.weight, |this, weight| this.font_weight(weight))
                        .child(row.role.cased(sample.text))
                })),
        )
}

fn type_scale() -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(spacing::S4)
        .children(TYPE_ROWS.iter().map(type_row))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::theme::tokens::tests::code_tokens;

    #[test]
    fn swatches_cover_every_colour_token_once() {
        let exported: BTreeMap<String, String> = code_tokens()
            .into_iter()
            .filter(|(name, _)| name.starts_with("--color-"))
            .map(|(name, value)| (name.to_owned(), value))
            .collect();

        let mut shown = BTreeMap::new();
        for swatch in COLUMNS
            .iter()
            .flat_map(|column| column.iter())
            .flat_map(|group| group.swatches)
        {
            if matches!(swatch.kind, SwatchKind::ChartOther) {
                continue;
            }
            let name = format!("--color-{}", swatch.name);
            let previous = shown.insert(name.clone(), hex(swatch.color));
            assert!(previous.is_none(), "{name} is shown twice");
        }

        assert_eq!(shown, exported, "the swatches drifted from the tokens");
    }

    #[test]
    fn hex_matches_the_token_export() {
        assert_eq!(hex(color::CANVAS), "#18181A");
        assert_eq!(hex(color::MOLTEN_SOFT), "#FF6B1A24");
    }
}
