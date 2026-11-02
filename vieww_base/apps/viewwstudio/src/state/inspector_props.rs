//! ST-02: the property grid — the Say registry, one picked line at a time.
//!
//! # What this module is
//!
//! The Inspector has always shown what a render object says about itself
//! through `describe()`: read-only, and the same for a `.say` buffer and a
//! Rust one. This module gives a `.say` buffer the other half: for the
//! picked render node's own `// say:` line (the same bridge ST-01's handles
//! use), a list of *typed* fields — number, colour, bool, enum, text —
//! parsed from the line, and written back to it through the same one-line
//! rewrite machinery the canvas handles drag through. One editing
//! pipeline, two pointing devices; nothing here is a second editor.
//!
//! # Which fields a line gets, and why a mirror and not a query
//!
//! The fields offered are the properties the Say parser accepts for the
//! line's kind — mirrored here from `parse_line.rs`'s acceptance matrix,
//! because the parser's tables are private and re-deriving them from a
//! compile of a probe line per field would cost a compile per pick. The
//! mirror is *narrower* than the parser in two places where the parser is
//! wider than its own generator: `from A to B` parses on any line but only
//! `a slider` reads it, and `pinned` parses on any line but only positions
//! anything inside `a stack` — the grid offers those two only where they
//! are used, so it never offers an edit that compiles and then does
//! nothing. The cross-check unit test at the bottom of this file writes
//! every kind-exclusive clause onto a line of the wrong kind and requires
//! the real compiler's diagnostic, and every offered clause onto a line of
//! the right kind and requires success — the mirror is tested against the
//! thing it mirrors.
//!
//! # Absent fields, and the values they show
//!
//! A field for a property the line does not carry is shown with the value
//! the framework would use anyway — spacing 0, children centred, the body
//! text size — read from the generator's own defaults, so the grid never
//! implies a value the pixels do not show. Width and height, the two a
//! canvas drag starts from, fall back to the picked row's rendered
//! rectangle: the number the person is looking at. Writing an absent field
//! *inserts* the clause (the line API's insertion path), so the grid grows
//! properties exactly as a handle does.
//!
//! # The honest limits
//!
//! An interpolated literal (`"Tapped \(count) times"`) is read-only — the
//! line API refuses to rewrite it rather than silently drop the `\(…)`, and
//! the grid marks it so. Colours are validated against the vocabulary
//! Say's lexer accepts (hex, the six names, the ten theme slots) before
//! they are written; anything a person types that Say would reject is
//! refused with a message rather than written and discovered at the next
//! Render.

use vieww_foundation::Rect;
use vieww_say_codegen::line::{self, LineProp, WidgetKindName, WidgetLine};

use super::*;

/// The three words an alignment clause can carry.
const ALIGNMENTS: &[&str] = &["the start", "the center", "the end"];

/// The colour names Say's lexer accepts; hex and the theme slots are
/// checked structurally.
const NAMED_COLORS: &[&str] = &["red", "green", "blue", "black", "white", "transparent"];

/// The theme slots, as the `with the … color` clause writes them.
const THEME_SLOTS: &[&str] = &[
    "the primary color",
    "the on primary color",
    "the surface color",
    "the on surface color",
    "the surface variant color",
    "the on surface variant color",
    "the outline color",
    "the error color",
    "the on error color",
];

/// One typed field of the picked say line, as the grid shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct SayField {
    /// The property this field writes — the shape, with the value as
    /// written where the line carries one. `None` for the line's own
    /// leading literal (`a button "Add one"`), which is a value, not a
    /// property.
    pub prop: Option<LineProp>,
    /// The grid's label for the field. Say vocabulary, deliberately
    /// untranslated: it is the word the next Render reads back.
    pub label: &'static str,
    /// The current value, typed for the grid's editor.
    pub value: FieldValue,
    /// Whether writing is allowed. False only for an interpolated literal.
    pub writable: bool,
}

/// A field's value, and the editor it implies.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    Number(f32),
    Text(String),
    /// A colour as written: `#RRGGBB`, a name, or a theme slot.
    Color(String),
    /// The enumerated words the clause accepts, and the current one.
    Choice(&'static [&'static str], String),
    /// A clause that is either on the line or not.
    Flag(bool),
}

impl FieldValue {
    /// The value as a read-only field shows it — the same text an editor
    /// would hold, because the read-only difference is the editor, not the
    /// value.
    #[must_use]
    pub fn to_display(&self) -> String {
        match self {
            Self::Number(n) => n.to_string(),
            Self::Text(text) | Self::Color(text) => text.clone(),
            Self::Choice(_, current) => current.clone(),
            Self::Flag(on) => (*on).then_some("\u{2713}").unwrap_or("\u{2013}").to_owned(),
        }
    }
}

impl Studio {
    /// The typed fields of the picked say line, for the property grid.
    ///
    /// `None` when nothing is picked, the pick does not map into the file,
    /// or the active buffer is not Say — the grid stays away rather than
    /// guessing, and the render object's own `describe()` rows (always
    /// shown) are what a Rust buffer gets instead.
    #[must_use]
    pub fn say_fields(&self) -> Option<Vec<SayField>> {
        let widget = self.picked_say_line()?;
        let rendered = self.picked_row_rect();
        // The value literal's writability is probed exactly: if the line API
        // can rewrite the literal to what it already says, it can rewrite it
        // to something else; an interpolated literal fails the probe.
        let raw = self.active().map(|b| b.value.text.clone());
        let writable_value = match (&widget.value, raw.as_deref()) {
            (Some(value), Some(raw)) => line::rewrite_value(raw, value).is_some(),
            _ => false,
        };
        Some(fields_of(&widget, rendered, writable_value))
    }

    /// Write `value` back through `field` — the grid's one write path.
    ///
    /// Every kind of field lands in the same place: the picked line's
    /// text, rewritten by the line API, applied through [`Studio::edit`] —
    /// the same path a keystroke and a canvas handle take, so the dirty
    /// flag, undo history and the 600 ms auto-render all engage. Refusals
    /// (a number that is not one, a colour the vocabulary does not have)
    /// notify rather than write, because a grid that silently keeps a
    /// value a person believes they set is worse than one that says why
    /// it will not.
    pub fn say_set(&self, field: &SayField, value: &FieldValue) {
        let Some(shape) = field.prop.as_ref() else {
            // The line's own words.
            if let FieldValue::Text(text) = value {
                if !self.say_rewrite(|old| line::rewrite_value(old, text)) {
                    self.notify(
                        "That value interpolates state — it is edited in the buffer, not here.",
                    );
                }
            }
            return;
        };
        match value {
            FieldValue::Flag(false) => {
                if !self.say_rewrite(|old| line::remove_prop(old, shape)) {
                    self.notify("That property is not on this line.");
                }
            }
            FieldValue::Flag(true) => self.canvas_write(shape.clone()),
            FieldValue::Number(n) => {
                self.canvas_write(with_number(shape, clamp_number(*n)));
            }
            FieldValue::Color(c) => {
                if !is_colour(c) {
                    // `notify` translates, template and all — the English
                    // sentence is the key and the argument rides along.
                    self.notify(&format!(
                        "{} is not a colour Say knows — hex, a name, or a theme slot.",
                        c,
                    ));
                    return;
                }
                self.canvas_write(LineProp::Color(c.trim().to_owned()));
            }
            FieldValue::Choice(_, word) => {
                if ALIGNMENTS.contains(&word.as_str()) {
                    self.canvas_write(with_word(shape, word.clone()));
                }
            }
            FieldValue::Text(text) => match shape {
                LineProp::Placeholder(_) => {
                    self.canvas_write(LineProp::Placeholder(text.clone()));
                }
                LineProp::DescribedAs(_) => {
                    self.canvas_write(LineProp::DescribedAs(text.clone()));
                }
                LineProp::Labelled(_) => {
                    self.canvas_write(LineProp::Labelled(text.clone()));
                }
                LineProp::From(_, _) => {
                    if let Some((from, to)) = parse_range(text) {
                        self.canvas_write(LineProp::From(from, to));
                    } else {
                        self.notify("That is not a number this field can write.");
                    }
                }
                _ => {
                    self.notify("That is not a number this field can write.");
                }
            },
        }
    }
}

/// The fields of one widget line: the properties its kind accepts (present
/// values, or the framework's defaults where absent), then any property the
/// line carries that the offered set does not cover — a parser-legal clause
/// the grid did not offer is still shown, because hiding a value the file
/// holds is the one thing worse than not offering to change it.
fn fields_of(widget: &WidgetLine, rendered: Option<Rect>, writable_value: bool) -> Vec<SayField> {
    use WidgetKindName as K;
    let kind = widget.kind;
    // A clause of one shape the line already carries, if it carries one.
    let on_line = |check: fn(&LineProp) -> bool| widget.props.iter().find(|p| check(p)).cloned();
    let is_padded = |p: &LineProp| matches!(p, LineProp::Padded(_));
    let is_spaced = |p: &LineProp| matches!(p, LineProp::Spaced(_));
    let is_children = |p: &LineProp| matches!(p, LineProp::ChildrenAligned(_));
    let is_aligned = |p: &LineProp| matches!(p, LineProp::Aligned(_));
    let is_size = |p: &LineProp| matches!(p, LineProp::Size(_));
    let is_bold = |p: &LineProp| matches!(p, LineProp::Bold);
    let is_color = |p: &LineProp| matches!(p, LineProp::Color(_));
    let is_radius = |p: &LineProp| matches!(p, LineProp::Radius(_));
    let is_wide = |p: &LineProp| matches!(p, LineProp::Wide(_));
    let is_tall = |p: &LineProp| matches!(p, LineProp::Tall(_));
    let is_each_row = |p: &LineProp| matches!(p, LineProp::EachRow(_));
    let is_lines = |p: &LineProp| matches!(p, LineProp::Lines(_));
    let is_single = |p: &LineProp| matches!(p, LineProp::SingleLine);
    let is_placeholder = |p: &LineProp| matches!(p, LineProp::Placeholder(_));
    let is_described = |p: &LineProp| matches!(p, LineProp::DescribedAs(_));
    let is_labelled = |p: &LineProp| matches!(p, LineProp::Labelled(_));
    let is_from = |p: &LineProp| matches!(p, LineProp::From(_, _));

    let mut fields: Vec<SayField> = Vec::with_capacity(12);

    // The line's own words, for the kinds that carry a literal.
    if let Some(value) = &widget.value {
        let label = match kind {
            K::Button => "label",
            K::Tab => "tab",
            _ => "text",
        };
        fields.push(SayField {
            prop: None,
            label,
            value: FieldValue::Text(value.clone()),
            writable: writable_value,
        });
    }

    // One numeric clause: what is on the line, or the framework's default.
    // A field *returned*, not pushed — the offered set interleaves these with
    // fields of other shapes, and a closure that pushed would hold the one
    // mutable borrow of `fields` across them.
    let number = |prop: Option<LineProp>, label: &'static str, fallback: f32| -> SayField {
        let prop = prop.unwrap_or_else(|| with_number(&default_of(label, fallback), fallback));
        let value = prop.number().unwrap_or(fallback);
        SayField {
            value: FieldValue::Number(value),
            prop: Some(prop),
            label,
            writable: true,
        }
    };

    fields.push(number(on_line(is_padded), "padding", 0.0));

    if matches!(kind, K::Row | K::Column) {
        fields.push(number(on_line(is_spaced), "spacing", 0.0));
        let word = on_line(is_children)
            .map(|p| match p {
                LineProp::ChildrenAligned(w) => w,
                _ => "the center".to_owned(),
            })
            .unwrap_or_else(|| "the center".to_owned());
        fields.push(SayField {
            prop: Some(LineProp::ChildrenAligned(word.clone())),
            label: "children alignment",
            value: FieldValue::Choice(ALIGNMENTS, word),
            writable: true,
        });
    }

    if matches!(kind, K::Card | K::Center) {
        let word = on_line(is_aligned)
            .map(|p| match p {
                LineProp::Aligned(w) => w,
                _ => "the start".to_owned(),
            })
            .unwrap_or_else(|| "the start".to_owned());
        fields.push(SayField {
            prop: Some(LineProp::Aligned(word.clone())),
            label: "alignment",
            value: FieldValue::Choice(ALIGNMENTS, word),
            writable: true,
        });
    }

    if matches!(kind, K::Text | K::Heading | K::Label) {
        fields.push(number(on_line(is_size), "text size", body_size(kind)));
        let bold = on_line(is_bold).is_some();
        fields.push(SayField {
            prop: Some(LineProp::Bold),
            label: "bold",
            value: FieldValue::Flag(bold),
            writable: true,
        });
    }

    let color = on_line(is_color)
        .map(|p| match p {
            LineProp::Color(c) => c,
            _ => String::new(),
        })
        .unwrap_or_default();
    fields.push(SayField {
        prop: Some(LineProp::Color(color.clone())),
        label: "color",
        value: FieldValue::Color(color),
        writable: true,
    });

    fields.push(number(on_line(is_radius), "corner radius", 0.0));

    // Width and height default to what the picked row renders at — the
    // number the person is looking at — falling back to zero.
    let (width, height) = rendered.map_or((0.0, 0.0), |r| (r.width(), r.height()));
    fields.push(number(on_line(is_wide), "width", width));
    fields.push(number(on_line(is_tall), "height", height));

    if kind == K::ListView {
        fields.push(number(on_line(is_each_row), "row height", 48.0));
    }

    if kind == K::TextField {
        let lines = on_line(is_lines)
            .map(|p| match p {
                LineProp::Lines(n) => n as f32,
                _ => 1.0,
            })
            .unwrap_or(1.0);
        fields.push(SayField {
            prop: Some(with_number(&LineProp::Lines(1), lines)),
            label: "lines",
            value: FieldValue::Number(lines),
            writable: true,
        });
        let single = on_line(is_single).is_some();
        fields.push(SayField {
            prop: Some(LineProp::SingleLine),
            label: "single line",
            value: FieldValue::Flag(single),
            writable: true,
        });
        if let Some(LineProp::Placeholder(text)) = on_line(is_placeholder) {
            fields.push(SayField {
                prop: Some(LineProp::Placeholder(text.clone())),
                label: "placeholder",
                value: FieldValue::Text(text),
                writable: true,
            });
        }
    }

    if let Some(LineProp::DescribedAs(text)) = on_line(is_described) {
        if matches!(kind, K::EmptyState | K::Image) {
            fields.push(SayField {
                prop: Some(LineProp::DescribedAs(text.clone())),
                label: "description",
                value: FieldValue::Text(text),
                writable: true,
            });
        }
    }

    if let Some(LineProp::Labelled(text)) = on_line(is_labelled) {
        if kind == K::Fab {
            fields.push(SayField {
                prop: Some(LineProp::Labelled(text.clone())),
                label: "label",
                value: FieldValue::Text(text),
                writable: true,
            });
        }
    }

    if kind == K::Slider {
        let (from, to) = match on_line(is_from) {
            Some(LineProp::From(a, b)) => (a, b),
            _ => (0.0, 1.0),
        };
        fields.push(SayField {
            prop: Some(LineProp::From(from, to)),
            label: "range",
            value: FieldValue::Text(range_text(from, to)),
            writable: true,
        });
    }

    // Anything the line carries that the offered set does not cover —
    // legal to the parser, unusual to the eye, still the file's own value.
    let offered = |fields: &[SayField], p: &LineProp| {
        fields
            .iter()
            .any(|f| f.prop.as_ref().is_some_and(|shape| same_shape(shape, p)))
    };
    for prop in &widget.props {
        if offered(&fields, prop) {
            continue;
        }
        let field = match prop {
            LineProp::Placeholder(text) => SayField {
                prop: Some(prop.clone()),
                label: "placeholder",
                value: FieldValue::Text(text.clone()),
                writable: true,
            },
            LineProp::DescribedAs(text) => SayField {
                prop: Some(prop.clone()),
                label: "description",
                value: FieldValue::Text(text.clone()),
                writable: true,
            },
            LineProp::Labelled(text) => SayField {
                prop: Some(prop.clone()),
                label: "label",
                value: FieldValue::Text(text.clone()),
                writable: true,
            },
            LineProp::From(a, b) => SayField {
                prop: Some(prop.clone()),
                label: "range",
                value: FieldValue::Text(range_text(*a, *b)),
                writable: true,
            },
            LineProp::Pinned(side, n) => SayField {
                prop: Some(prop.clone()),
                label: pinned_label(side),
                value: FieldValue::Number(*n),
                writable: true,
            },
            LineProp::ChildrenAligned(word) => SayField {
                prop: Some(prop.clone()),
                label: "children alignment",
                value: FieldValue::Choice(ALIGNMENTS, word.clone()),
                writable: true,
            },
            LineProp::Aligned(word) => SayField {
                prop: Some(prop.clone()),
                label: "alignment",
                value: FieldValue::Choice(ALIGNMENTS, word.clone()),
                writable: true,
            },
            // Every remaining shape is always offered above.
            _ => continue,
        };
        fields.push(field);
    }

    fields
}

/// The text size the theme gives the kind when the line says nothing.
fn body_size(kind: WidgetKindName) -> f32 {
    if kind == WidgetKindName::Heading {
        24.0
    } else {
        15.0
    }
}

/// `pinned <side>`'s label, for the unusual-line pass.
fn pinned_label(side: &str) -> &'static str {
    match side {
        "top" => "pinned top",
        "bottom" => "pinned bottom",
        "left" => "pinned left",
        _ => "pinned right",
    }
}

/// The default prop a numeric field's label stands for.
fn default_of(label: &str, value: f32) -> LineProp {
    match label {
        "spacing" => LineProp::Spaced(value),
        "text size" => LineProp::Size(value),
        "corner radius" => LineProp::Radius(value),
        "width" => LineProp::Wide(value),
        "height" => LineProp::Tall(value),
        "row height" => LineProp::EachRow(value),
        _ => LineProp::Padded(value),
    }
}

/// A prop of the same shape with `value` — the write a field makes.
fn with_number(shape: &LineProp, value: f32) -> LineProp {
    match shape {
        LineProp::Padded(_) => LineProp::Padded(value),
        LineProp::Spaced(_) => LineProp::Spaced(value),
        LineProp::Wide(_) => LineProp::Wide(value),
        LineProp::Tall(_) => LineProp::Tall(value),
        LineProp::Size(_) => LineProp::Size(value),
        LineProp::Radius(_) => LineProp::Radius(value),
        LineProp::EachRow(_) => LineProp::EachRow(value),
        LineProp::Lines(_) => LineProp::Lines(clamp_number(value) as u32),
        LineProp::From(_, to) => LineProp::From(value, *to),
        LineProp::Pinned(side, _) => LineProp::Pinned(side.to_owned(), value),
        other => other.clone(),
    }
}

/// An alignment prop carrying `word`.
fn with_word(shape: &LineProp, word: String) -> LineProp {
    match shape {
        LineProp::Aligned(_) => LineProp::Aligned(word),
        LineProp::ChildrenAligned(_) => LineProp::ChildrenAligned(word),
        other => other.clone(),
    }
}

/// Whether two props are the same clause regardless of value.
fn same_shape(a: &LineProp, b: &LineProp) -> bool {
    match (a, b) {
        (LineProp::Padded(_), LineProp::Padded(_))
        | (LineProp::Spaced(_), LineProp::Spaced(_))
        | (LineProp::Wide(_), LineProp::Wide(_))
        | (LineProp::Tall(_), LineProp::Tall(_))
        | (LineProp::Size(_), LineProp::Size(_))
        | (LineProp::Radius(_), LineProp::Radius(_))
        | (LineProp::EachRow(_), LineProp::EachRow(_))
        | (LineProp::Bold, LineProp::Bold)
        | (LineProp::SingleLine, LineProp::SingleLine)
        | (LineProp::Color(_), LineProp::Color(_))
        | (LineProp::Aligned(_), LineProp::Aligned(_))
        | (LineProp::ChildrenAligned(_), LineProp::ChildrenAligned(_))
        | (LineProp::Placeholder(_), LineProp::Placeholder(_))
        | (LineProp::DescribedAs(_), LineProp::DescribedAs(_))
        | (LineProp::Labelled(_), LineProp::Labelled(_))
        | (LineProp::Lines(_), LineProp::Lines(_))
        | (LineProp::From(_, _), LineProp::From(_, _)) => true,
        (LineProp::Pinned(side_a, _), LineProp::Pinned(side_b, _)) => side_a == side_b,
        _ => false,
    }
}

/// Say writes no negative numbers — the lexer makes `-7` a word — so a
/// number field clamps at zero rather than writing a line the next Render
/// would reject.
fn clamp_number(n: f32) -> f32 {
    n.max(0.0)
}

/// A slider range as the field shows it: `0 to 1`.
fn range_text(from: f32, to: f32) -> String {
    format!("{} to {}", plain(from), plain(to))
}

/// Parse `A to B` back. Whitespace-tolerant, nothing else — the field is
/// the vocabulary's own spelling.
fn parse_range(text: &str) -> Option<(f32, f32)> {
    let (a, b) = text.split_once(" to ")?;
    let from = a.trim().parse().ok()?;
    let to = b.trim().parse().ok()?;
    Some((clamp_number(from), clamp_number(to)))
}

/// A number without its trailing zero: `16`, not `16.0`.
fn plain(n: f32) -> String {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "a property value outside i64 is not a dimension anybody edits"
    )]
    let whole = n as i64;
    if whole as f32 == n {
        whole.to_string()
    } else {
        format!("{n}")
    }
}

/// Whether `text` is a colour the Say lexer accepts.
fn is_colour(text: &str) -> bool {
    let text = text.trim();
    if text.is_empty() {
        return false;
    }
    if let Some(hex) = text.strip_prefix('#') {
        return (hex.len() == 6 || hex.len() == 8) && hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    NAMED_COLORS.contains(&text) || THEME_SLOTS.contains(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One widget line, for the field tests.
    fn line_of(kind: WidgetKindName, value: Option<&str>, props: Vec<LineProp>) -> WidgetLine {
        WidgetLine {
            line: 8,
            phrase: "a column",
            kind,
            value: value.map(str::to_owned),
            props,
            depth: 0,
        }
    }

    /// The counter's column, as the grid reads it: spacing from the line,
    /// padding absent at zero, children alignment from the line, width and
    /// height from the rendered rectangle — and nothing a column cannot
    /// take (no bold, no placeholder, no alignment of its own).
    #[test]
    fn a_column_reads_seven_fields() {
        let column = line_of(
            WidgetKindName::Column,
            None,
            vec![
                LineProp::Spaced(16.0),
                LineProp::ChildrenAligned("the start".to_owned()),
            ],
        );
        let fields = fields_of(&column, Some(Rect::new(0.0, 0.0, 320.0, 500.0)), false);
        let labels: Vec<&str> = fields.iter().map(|f| f.label).collect();
        assert_eq!(
            labels,
            vec![
                "padding",
                "spacing",
                "children alignment",
                "color",
                "corner radius",
                "width",
                "height",
            ]
        );
        assert_eq!(
            fields[0].value,
            FieldValue::Number(0.0),
            "absent padding is zero"
        );
        assert_eq!(fields[1].value, FieldValue::Number(16.0));
        assert_eq!(
            fields[2].value,
            FieldValue::Choice(ALIGNMENTS, "the start".to_owned())
        );
        assert_eq!(
            fields[5].value,
            FieldValue::Number(320.0),
            "width from the render"
        );
        assert_eq!(
            fields[6].value,
            FieldValue::Number(500.0),
            "height from the render"
        );
    }

    /// A heading's grid: the literal, the padding any widget can take, the
    /// heading size the theme would give it, bold as a flag — and no
    /// spacing, which the parser refuses.
    #[test]
    fn a_heading_offers_only_what_it_can_take() {
        let heading = line_of(WidgetKindName::Heading, Some("Counter"), vec![]);
        let fields = fields_of(&heading, None, true);
        let labels: Vec<&str> = fields.iter().map(|f| f.label).collect();
        assert_eq!(
            labels,
            vec![
                "text",
                "padding",
                "text size",
                "bold",
                "color",
                "corner radius",
                "width",
                "height",
            ]
        );
        assert_eq!(fields[0].value, FieldValue::Text("Counter".to_owned()));
        assert!(fields[0].writable);
        assert_eq!(fields[1].value, FieldValue::Number(0.0), "absent padding");
        assert_eq!(
            fields[2].value,
            FieldValue::Number(24.0),
            "the heading size"
        );
        assert_eq!(fields[3].value, FieldValue::Flag(false));
    }

    /// A text field's grid carries its own vocabulary: lines, single line,
    /// placeholder — and the placeholder only when the line has one.
    #[test]
    fn a_text_field_carries_its_own_vocabulary() {
        let field = line_of(
            WidgetKindName::TextField,
            None,
            vec![
                LineProp::Placeholder("Your name".to_owned()),
                LineProp::Lines(3),
            ],
        );
        let fields = fields_of(&field, None, false);
        let labels: Vec<&str> = fields.iter().map(|f| f.label).collect();
        assert!(labels.contains(&"lines"));
        assert!(labels.contains(&"single line"));
        assert!(labels.contains(&"placeholder"));
        let lines = fields.iter().find(|f| f.label == "lines").unwrap();
        assert_eq!(lines.value, FieldValue::Number(3.0));
        assert!(
            matches!(lines.prop, Some(LineProp::Lines(_))),
            "the lines field writes `in N lines`, not `size N`"
        );
        let placeholder = fields.iter().find(|f| f.label == "placeholder").unwrap();
        assert_eq!(placeholder.value, FieldValue::Text("Your name".to_owned()));
    }

    /// An unusual line keeps its own values: a pinned clause on a widget
    /// the offered set does not grow one for is still shown, still
    /// writable, with its own shape.
    #[test]
    fn an_unusual_clause_is_still_shown() {
        let child = line_of(
            WidgetKindName::Text,
            Some("x"),
            vec![LineProp::Pinned("top".to_owned(), 12.0)],
        );
        let fields = fields_of(&child, None, true);
        let pinned = fields
            .iter()
            .find(|f| f.label == "pinned top")
            .expect("the pinned clause is shown");
        assert_eq!(pinned.value, FieldValue::Number(12.0));
        assert!(
            matches!(pinned.prop, Some(LineProp::Pinned(ref side, _)) if side == "top"),
            "and it writes `pinned top N`, not `size N`"
        );
    }

    /// The colour validation admits the vocabulary and refuses the rest.
    #[test]
    fn colour_validation_admits_the_vocabulary() {
        for good in [
            "#3366FF",
            "#FF3366FF",
            "red",
            "transparent",
            "the primary color",
            "the on surface variant color",
        ] {
            assert!(is_colour(good), "{good} is a colour");
        }
        for bad in ["", "#336", "3366FF", "teal", "the teal color", "#GGGGGG"] {
            assert!(!is_colour(bad), "{bad} is not a colour");
        }
    }

    /// A range round-trips through its own spelling.
    #[test]
    fn a_range_round_trips() {
        assert_eq!(parse_range("0 to 1"), Some((0.0, 1.0)));
        assert_eq!(parse_range("0.5 to 2.5"), Some((0.5, 2.5)));
        assert_eq!(parse_range(&range_text(0.0, 1.0)), Some((0.0, 1.0)));
        assert_eq!(parse_range("0 - 1"), None);
        assert_eq!(parse_range("zero to one"), None);
    }

    /// The mirror is tested against the thing it mirrors. Each case is a
    /// full file whose one widget line carries the clause the grid would
    /// write; the grid offers it only where the compiler accepts it, and
    /// the line API is proven to produce exactly that clause from the bare
    /// line — the grid's write path and its acceptance agree.
    #[test]
    fn the_offered_set_is_the_accepted_set() {
        #[derive(Debug)]
        struct Case {
            /// The clause's prop, as the grid holds it.
            prop: LineProp,
            /// The line with the clause, as the grid would write it.
            with_clause: &'static str,
            /// The line with the clause removed, for the write-path probe.
            bare: &'static str,
            /// Whether the file compiles.
            compiles: bool,
        }
        let head = "-- say-language: 1\nkeep text called draft\nkeep a list of text called names starting at \"a\", \"b\"\nscreen \"A\":\n    ";
        let cases: Vec<Case> = vec![
            // Offered, and accepted.
            Case {
                prop: LineProp::Spaced(4.0),
                with_clause: "a column, spaced 4:",
                bare: "a column:",
                compiles: true,
            },
            Case {
                prop: LineProp::ChildrenAligned("the start".to_owned()),
                with_clause: "a column, children aligned to the start:",
                bare: "a column:",
                compiles: true,
            },
            Case {
                prop: LineProp::Size(14.0),
                with_clause: "a text \"x\", size 14:",
                bare: "a text \"x\":",
                compiles: true,
            },
            Case {
                prop: LineProp::Bold,
                with_clause: "a text \"x\", bold:",
                bare: "a text \"x\":",
                compiles: true,
            },
            Case {
                prop: LineProp::Aligned("the start".to_owned()),
                with_clause: "a card, aligned to the start:",
                bare: "a card:",
                compiles: true,
            },
            Case {
                prop: LineProp::Placeholder("…".to_owned()),
                with_clause: "a text field bound to draft, with placeholder \"…\":",
                bare: "a text field bound to draft:",
                compiles: true,
            },
            Case {
                prop: LineProp::SingleLine,
                with_clause: "a text field bound to draft, single line:",
                bare: "a text field bound to draft:",
                compiles: true,
            },
            Case {
                prop: LineProp::Lines(3),
                with_clause: "a text field bound to draft, in 3 lines:",
                bare: "a text field bound to draft:",
                compiles: true,
            },
            Case {
                prop: LineProp::EachRow(56.0),
                with_clause: "a list of names, each row 56:",
                bare: "a list of names:",
                compiles: true,
            },
            // Refused by the compiler, and so not offered: same clause,
            // wrong kind.
            Case {
                prop: LineProp::Spaced(4.0),
                with_clause: "a text \"x\", spaced 4:",
                bare: "a text \"x\":",
                compiles: false,
            },
            Case {
                prop: LineProp::ChildrenAligned("the start".to_owned()),
                with_clause: "a card, children aligned to the start:",
                bare: "a card:",
                compiles: false,
            },
            Case {
                prop: LineProp::Size(14.0),
                with_clause: "a card, size 14:",
                bare: "a card:",
                compiles: false,
            },
            Case {
                prop: LineProp::Bold,
                with_clause: "a card, bold:",
                bare: "a card:",
                compiles: false,
            },
            Case {
                prop: LineProp::Aligned("the start".to_owned()),
                with_clause: "a text \"x\", aligned to the start:",
                bare: "a text \"x\":",
                compiles: false,
            },
            Case {
                prop: LineProp::Placeholder("…".to_owned()),
                with_clause: "a text \"x\", with placeholder \"…\":",
                bare: "a text \"x\":",
                compiles: false,
            },
            Case {
                prop: LineProp::SingleLine,
                with_clause: "a text \"x\", single line:",
                bare: "a text \"x\":",
                compiles: false,
            },
            Case {
                prop: LineProp::Lines(3),
                with_clause: "a text \"x\", in 3 lines:",
                bare: "a text \"x\":",
                compiles: false,
            },
            Case {
                prop: LineProp::EachRow(56.0),
                with_clause: "a column, each row 56:",
                bare: "a column:",
                compiles: false,
            },
            Case {
                prop: LineProp::DescribedAs("y".to_owned()),
                with_clause: "a text \"x\", described as \"y\":",
                bare: "a text \"x\":",
                compiles: false,
            },
            Case {
                prop: LineProp::Labelled("y".to_owned()),
                with_clause: "a text \"x\", labelled \"y\":",
                bare: "a text \"x\":",
                compiles: false,
            },
        ];
        for case in &cases {
            let file = format!("{head}{}\n", case.with_clause);
            let compiled = vieww_say_codegen::compile("t.say", &file);
            assert_eq!(
                compiled.is_ok(),
                case.compiles,
                "{:?} should {}: {:?}",
                case.with_clause,
                if case.compiles {
                    "compile"
                } else {
                    "be refused"
                },
                compiled.err(),
            );
            // And the grid's write path produces exactly the clause the
            // acceptance was tested on: rewriting the bare line yields the
            // clause line.
            let rewritten = line::rewrite_prop(case.bare, &case.prop);
            assert_eq!(
                rewritten.as_deref(),
                Some(case.with_clause),
                "the grid writes the clause it offers for {:?}",
                case.prop
            );
        }
    }
}
