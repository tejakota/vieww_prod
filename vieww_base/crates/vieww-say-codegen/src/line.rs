//! A structured, public view of one `.say` file's widget lines.
//!
//! # Why this exists
//!
//! The parser and the generator are private on purpose: their types are the
//! compiler's own, and letting the studio reach into them would make every
//! refactor of Say a studio refactor too. But two studio tools need to know
//! what a widget line *says* without compiling it — ST-01's canvas handles
//! (map a picked render node back to the `.say` line and rewrite one number
//! on it) and ST-02's property inspector (typed fields parsed from and
//! written back to the same line). Both want the same thing: the phrase, the
//! leading string literal if there is one, and the properties with their
//! values — as *written*, so a write-back can round-trip.
//!
//! So this module is a mirror, not a window: it re-states the subset of the
//! AST those tools need in types that can stay stable while the parser
//! changes underneath. It is also deliberately forgiving —
//! [`parse_widget_lines`] returns whatever parsed, ignoring diagnostics,
//! because an editor tool must work on a file that does not compile: that is
//! exactly when somebody is editing it.

use std::rc::Rc;

use crate::parse::{AKind, ColorSpec, Node, Prop, Value, WKind, Widget};
use crate::{lex, parse};

/// Which widget a line declares. One name per phrase the vocabulary has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetKindName {
    Card,
    Row,
    Column,
    Stack,
    Center,
    Fixed,
    Text,
    Heading,
    Label,
    Button,
    TextField,
    Switch,
    Checkbox,
    Slider,
    Fab,
    ListView,
    EmptyState,
    Icon,
    Image,
    ScrollView,
    Tabs,
    Tab,
}

impl WidgetKindName {
    /// The render-object `debug_name` the generated Rust produces for this
    /// widget, where it produces exactly one — the bridge ST-01 matches a
    /// picked render node against.
    ///
    /// `None` for the kinds that compile to something composite or wrapped
    /// (`a card` is a `Container` the studio's own chrome also uses, a
    /// `a set of tabs` is a column of a bar and a page): those cannot be told
    /// apart by name and are not candidates for canvas matching.
    #[must_use]
    pub fn render_name(self) -> Option<&'static str> {
        Some(match self {
            Self::Card => "RenderDecoratedBox",
            Self::Row => "RenderRow",
            Self::Column => "RenderColumn",
            Self::Stack => "RenderStack",
            Self::Center => "RenderAlign",
            Self::Fixed => "RenderConstrainedBox",
            Self::Text | Self::Heading | Self::Label => "RenderText",
            Self::Image => "RenderImage",
            Self::ScrollView => "RenderScrollable",
            Self::Icon => "RenderIcon",
            // Composite, wrapped, or otherwise not name-unique.
            Self::Button
            | Self::TextField
            | Self::Switch
            | Self::Checkbox
            | Self::Slider
            | Self::Fab
            | Self::ListView
            | Self::EmptyState
            | Self::Tabs
            | Self::Tab => return None,
        })
    }
}

/// One property as written, with its value where the phrase carries one.
///
/// The string-valued variants carry the *source* text (the quoted literal),
/// not a parsed form, so a write-back can compare and re-emit exactly what
/// was there.
#[derive(Debug, Clone, PartialEq)]
pub enum LineProp {
    Padded(f32),
    Spaced(f32),
    Wide(f32),
    Tall(f32),
    Size(f32),
    Radius(f32),
    Bold,
    /// A colour as written: `#RRGGBB`, a theme slot, or a named colour word.
    Color(String),
    Aligned(String),
    ChildrenAligned(String),
    Placeholder(String),
    SingleLine,
    Lines(u32),
    EachRow(f32),
    DescribedAs(String),
    Labelled(String),
    From(f32, f32),
    Pinned(String, f32),
}

impl LineProp {
    /// The numeric value, for the four properties a canvas handle edits.
    ///
    /// `None` for every property that is not one number, so a caller that
    /// only wants a value to nudge does not match eight arms to find it.
    #[must_use]
    pub const fn number(&self) -> Option<f32> {
        match *self {
            Self::Padded(n) | Self::Spaced(n) | Self::Wide(n) | Self::Tall(n) => Some(n),
            Self::Size(n) | Self::Radius(n) | Self::EachRow(n) => Some(n),
            _ => None,
        }
    }

    /// What the property is called in a sentence, for the inspector's labels.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Padded(_) => "padding",
            Self::Spaced(_) => "spacing",
            Self::Wide(_) => "width",
            Self::Tall(_) => "height",
            Self::Size(_) => "text size",
            Self::Radius(_) => "corner radius",
            Self::Bold => "bold",
            Self::Color(_) => "color",
            Self::Aligned(_) => "alignment",
            Self::ChildrenAligned(_) => "children alignment",
            Self::Placeholder(_) => "placeholder",
            Self::SingleLine => "single line",
            Self::Lines(_) => "lines",
            Self::EachRow(_) => "row height",
            Self::DescribedAs(_) => "description",
            Self::Labelled(_) => "label",
            Self::From(_, _) => "range",
            Self::Pinned(_, _) => "pin",
        }
    }
}

/// One widget line of a `.say` file, re-stated for editor tools.
#[derive(Debug, Clone, PartialEq)]
pub struct WidgetLine {
    /// The 1-based line in the source, as the `// say:` markers carry it.
    pub line: u32,
    /// The canonical phrase, `a column` — properties not included.
    pub phrase: &'static str,
    /// Which widget the phrase declares.
    pub kind: WidgetKindName,
    /// The leading string literal, `Add one` for `a button "Add one"`.
    ///
    /// `None` for the phrases that carry no literal. For interpolated
    /// values this is the literal *prefix* — `Tapped ` for
    /// `a label "Tapped \(count) times"` — because that prefix is the part
    /// a rendered text can be matched against at run time.
    pub value: Option<String>,
    /// The properties, in written order.
    pub props: Vec<LineProp>,
    /// The nesting depth of the line in the say tree: 0 under the screen's
    /// body, one more per level of block indentation. The studio's
    /// render-tree ↔ say-tree bridge matches on this; it is editor-tool
    /// information the compiler itself never needs.
    pub depth: u32,
}

/// Every widget line in `source`, in document order, ignoring diagnostics.
///
/// A file that does not parse yields whatever did: the studio's canvas and
/// inspector have to work on a half-edited file, not refuse because the
/// compiler would.
#[must_use]
pub fn parse_widget_lines(source: &str) -> Vec<WidgetLine> {
    let file = Rc::new("say".to_owned());
    let lines = lex::lex(source);
    // The parser collects diagnostics and keeps going where it can; on error
    // it still holds the partial tree, and that tree is what we want.
    let mut parser = parse::Parser::for_tools(&file, &lines);
    parser.top_level();
    let mut out = Vec::new();
    for screen in &parser.screens {
        walk_nodes(&screen.body, 0, &mut out);
    }
    out
}

/// Walk every node, collecting widget lines and descending into the places
/// widget lines hide: children, `for each` bodies, dialog bodies, tabs.
fn walk_nodes(nodes: &[Node], depth: u32, out: &mut Vec<WidgetLine>) {
    for node in nodes {
        match node {
            Node::Widget(widget) => {
                out.push(line_of(widget, depth));
                walk_nodes(&widget.children, depth + 1, out);
                if let Some(event) = &widget.event {
                    for action in &event.actions {
                        if let AKind::OpenDialog { body, .. } = &action.kind {
                            walk_nodes(body, depth + 1, out);
                        }
                    }
                }
            }
            Node::ForEach { body, .. } => walk_nodes(body, depth + 1, out),
        }
    }
}

/// One `Widget`, mirrored.
fn line_of(widget: &Widget, depth: u32) -> WidgetLine {
    let kind = match &widget.kind {
        WKind::Card => WidgetKindName::Card,
        WKind::Row => WidgetKindName::Row,
        WKind::Column => WidgetKindName::Column,
        WKind::Stack => WidgetKindName::Stack,
        WKind::Center => WidgetKindName::Center,
        WKind::Fixed { .. } => WidgetKindName::Fixed,
        WKind::Text { heading: true, .. } => WidgetKindName::Heading,
        WKind::Text { heading: false, .. } => WidgetKindName::Label,
        WKind::Button { .. } => WidgetKindName::Button,
        WKind::TextField { .. } => WidgetKindName::TextField,
        WKind::Switch { .. } => WidgetKindName::Switch,
        WKind::Checkbox { .. } => WidgetKindName::Checkbox,
        WKind::Slider { .. } => WidgetKindName::Slider,
        WKind::Fab { .. } => WidgetKindName::Fab,
        WKind::ListView { .. } => WidgetKindName::ListView,
        WKind::EmptyState { .. } => WidgetKindName::EmptyState,
        WKind::IconOnly { .. } => WidgetKindName::Icon,
        WKind::Image { .. } => WidgetKindName::Image,
        WKind::ScrollView => WidgetKindName::ScrollView,
        WKind::Tabs => WidgetKindName::Tabs,
        WKind::Tab { .. } => WidgetKindName::Tab,
        WKind::Rust { .. } => WidgetKindName::Text, // unreachable: not a phrase
    };
    let value = match &widget.kind {
        WKind::Text { value, .. } | WKind::Button { label: value, .. } => literal_prefix(value),
        WKind::Tab { label } => Some(label.clone()),
        _ => None,
    };
    let props = widget.props.iter().map(prop_of).collect();
    WidgetLine {
        line: widget.line,
        phrase: widget.phrase,
        kind,
        value,
        props,
        depth,
    }
}

/// The literal prefix of a `Value`: everything before the first `\(…)`.
fn literal_prefix(value: &Value) -> Option<String> {
    match value {
        Value::Lit(text) => Some(text.clone()),
        Value::Parts(parts) => {
            let mut out = String::new();
            for part in parts {
                match part {
                    crate::parse::Part::Lit(text) => out.push_str(text),
                    crate::parse::Part::Expr(_) => break,
                }
            }
            Some(out)
        }
    }
}

/// One `Prop`, mirrored.
fn prop_of(prop: &Prop) -> LineProp {
    match prop {
        Prop::Padded(n) => LineProp::Padded(*n),
        Prop::Spaced(n) => LineProp::Spaced(*n),
        Prop::Wide(n) => LineProp::Wide(*n),
        Prop::Tall(n) => LineProp::Tall(*n),
        Prop::Size(n) => LineProp::Size(*n),
        Prop::Radius(n) => LineProp::Radius(*n),
        Prop::EachRow(n) => LineProp::EachRow(*n),
        Prop::Lines(n) => LineProp::Lines(*n),
        Prop::Bold => LineProp::Bold,
        Prop::SingleLine => LineProp::SingleLine,
        Prop::Placeholder(text) => LineProp::Placeholder(text.clone()),
        Prop::Description(text) => LineProp::DescribedAs(text.clone()),
        Prop::Labelled(text) => LineProp::Labelled(text.clone()),
        Prop::Color(spec) => LineProp::Color(color_of(spec)),
        Prop::Aligned(align) => LineProp::Aligned(align_word(align)),
        Prop::ChildrenAligned(cross) => LineProp::ChildrenAligned(cross_word(cross)),
        Prop::From { from, to } => LineProp::From(*from, *to),
        Prop::Pin(pin) => match pin {
            crate::parse::Pin::Top(n) => LineProp::Pinned("top".to_owned(), *n),
            crate::parse::Pin::Bottom(n) => LineProp::Pinned("bottom".to_owned(), *n),
            crate::parse::Pin::Left(n) => LineProp::Pinned("left".to_owned(), *n),
            crate::parse::Pin::Right(n) => LineProp::Pinned("right".to_owned(), *n),
        },
    }
}

/// A colour as written. The parser stores hex as `AARRGGBB` with the
/// six-digit form promoted to an opaque `FF` alpha, so writing back means
/// un-promoting: `#3366FF`, not `#FF3366FF` — the shorter spelling is what
/// the vocabulary's own fixtures use, and what a rewritten line should say.
fn color_of(spec: &ColorSpec) -> String {
    match spec {
        ColorSpec::Hex(value) => {
            if value >> 24 == 0xFF {
                format!("#{:06X}", value & 0x00FF_FFFF)
            } else {
                format!("#{value:08X}")
            }
        }
        ColorSpec::Named(named) => match named {
            crate::parse::NamedColor::Red => "red".to_owned(),
            crate::parse::NamedColor::Green => "green".to_owned(),
            crate::parse::NamedColor::Blue => "blue".to_owned(),
            crate::parse::NamedColor::Black => "black".to_owned(),
            crate::parse::NamedColor::White => "white".to_owned(),
            crate::parse::NamedColor::Transparent => "transparent".to_owned(),
        },
        ColorSpec::Theme(slot) => match slot {
            crate::parse::ThemeSlot::Primary => "the primary color".to_owned(),
            crate::parse::ThemeSlot::OnPrimary => "the on primary color".to_owned(),
            crate::parse::ThemeSlot::Surface => "the surface color".to_owned(),
            crate::parse::ThemeSlot::OnSurface => "the on surface color".to_owned(),
            crate::parse::ThemeSlot::SurfaceVariant => "the surface variant color".to_owned(),
            crate::parse::ThemeSlot::OnSurfaceVariant => "the on surface variant color".to_owned(),
            crate::parse::ThemeSlot::Outline => "the outline color".to_owned(),
            crate::parse::ThemeSlot::Error => "the error color".to_owned(),
            crate::parse::ThemeSlot::OnError => "the on error color".to_owned(),
        },
    }
}

/// An alignment as the phrase writes it. `Align` and `Cross` carry the same
/// three words for different clauses, so each gets its own little lookup —
/// a macro would save four lines and cost a reader a detour.
fn align_word(align: &crate::parse::Align) -> String {
    match align {
        crate::parse::Align::Start => "the start".to_owned(),
        crate::parse::Align::Center => "the center".to_owned(),
        crate::parse::Align::End => "the end".to_owned(),
    }
}

/// The children's cross-axis alignment as the phrase writes it.
fn cross_word(cross: &crate::parse::Cross) -> String {
    match cross {
        crate::parse::Cross::Start => "the start".to_owned(),
        crate::parse::Cross::Center => "the center".to_owned(),
        crate::parse::Cross::End => "the end".to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Writing back
//
// The reader above mirrors the parser; the writer below edits one line's
// *text*, because that text is the file — the studio rewrites the `.say`
// line in the buffer and the normal compile pipeline turns it back into
// pixels. The writer never re-serialises the whole line (that would
// reformat what the user wrote); it finds the one property's span and
// replaces exactly that, so a diff of the file is a diff of the value.

/// Replace, or insert, one property on a widget line's text.
///
/// `line` is the line exactly as written (indentation included is fine; it
/// is preserved). When the line already carries a property of the same
/// shape, its value is replaced in place — position, order and every other
/// byte untouched. When it does not, the property is inserted after the
/// phrase and any properties, before the event clause or block colon:
/// `a column:` becomes `a column, padded 12:`.
///
/// `None` when the line has no place a property could go — no recognisable
/// phrase, or the property's clause could not be located — which is the
/// caller's cue to refuse rather than guess.
///
/// # The honest limits
///
/// The property's span is found by scanning the line's words outside
/// string literals, so a property keyword cannot be confused with text
/// inside a `"…"`. It *can*, in principle, be confused with a state name in
/// an `only if …` clause that happens to read like a property — a variable
/// called `padded`, say. Say's own parser has the same ambiguity and
/// resolves it the same way: nobody names a state that, and the round-trip
/// tests hold the line.
#[must_use]
pub fn rewrite_prop(line: &str, new: &LineProp) -> Option<String> {
    let atoms = scan(line);
    match prop_span(&atoms, new) {
        Some((start, end)) => {
            let replacement = prop_text(new);
            Some(replace_span(
                line,
                atoms[start].span.0,
                atoms[end - 1].span.1,
                &replacement,
            ))
        }
        None => insert_prop(line, &atoms, new),
    }
}

/// Put `new` on the line where no property of its shape is yet: at the end
/// of the property region, before the event clause or the block colon,
/// separated by a comma.
fn insert_prop(line: &str, atoms: &[Atom], new: &LineProp) -> Option<String> {
    phrase_end(atoms)?;
    // The clause's end: `which …` starts the event, `:` opens the block.
    let stop = atoms
        .iter()
        .position(|a| {
            matches!(&a.kind, AtomKind::Word(w) if w == "which")
                || matches!(a.kind, AtomKind::Colon)
        })
        .unwrap_or(atoms.len());
    let mut at = atoms
        .get(stop)
        .map_or_else(|| line.trim_end().len(), |a| a.span.0);
    // `a column :` — do not leave a space before the comma we add.
    if at > 0 && line[..at].ends_with(' ') {
        at -= 1;
    }
    Some(replace_span(line, at, at, &format!(", {}", prop_text(new))))
}

/// Remove one property from a widget line's text.
///
/// The clause and one adjacent comma go with it, so `a column, padded 12,
/// spaced 4:` loses `padded 12` and one comma, not both. `None` when the
/// property is not on the line — the caller refuses rather than inventing
/// an edit.
#[must_use]
pub fn remove_prop(line: &str, which: &LineProp) -> Option<String> {
    let atoms = scan(line);
    let (start, end) = prop_span(&atoms, which)?;
    Some(remove_span(line, &atoms, start, end))
}

/// Replace a widget line's leading string literal — the words of a `a text
/// "…"`, the label of a button, the path of an image.
///
/// `None` when the line carries no literal to replace, or when the literal
/// interpolates (`"Tapped \(count) times"`): rewriting an interpolated
/// string with a plain one would silently drop the `\(…)`, so the writer
/// refuses and the caller keeps the field read-only.
#[must_use]
pub fn rewrite_value(line: &str, text: &str) -> Option<String> {
    let atoms = scan(line);
    let phrase_end = phrase_end(&atoms)?;
    let literal = (0..phrase_end).find(|&i| matches!(atoms[i].kind, AtomKind::Str(_)))?;
    let AtomKind::Str(old) = &atoms[literal].kind else {
        return None;
    };
    if old.contains("\\(") {
        return None; // interpolated: not ours to rewrite
    }
    let (start, end) = (atoms[literal].span.0, atoms[literal].span.1);
    let quoted = format!("\"{}\"", escape(text));
    Some(replace_span(line, start, end, &quoted))
}

/// One scanned word of a line: what it is, and where it is.
///
/// A private re-lex of the single line, because the real lexer records no
/// positions and the writer's whole contract is *where* the words are.
/// Mirrors `lex::tokenize` for the shapes that can appear on a widget
/// line: words, numbers, `#RRGGBB`, quoted strings, `,` and `:`.
#[derive(Debug)]
struct Atom {
    /// Byte range in the line's text.
    span: (usize, usize),
    kind: AtomKind,
}

#[derive(Debug, PartialEq)]
enum AtomKind {
    Word(String),
    /// Digits with an optional `.`; Say writes no signed numbers (the lexer
    /// makes `-7` a word, and the parser rejects it — so neither does this).
    Number(String),
    Hex(String),
    /// The decoded text of a `"…"`.
    Str(String),
    Comma,
    Colon,
}

impl Atom {
    fn word(&self) -> Option<&str> {
        match &self.kind {
            AtomKind::Word(w) => Some(w.as_str()),
            _ => None,
        }
    }
    fn is_number(&self) -> bool {
        matches!(self.kind, AtomKind::Number(_))
    }
}

/// Scan one line into atoms, skipping nothing — the caller slices regions.
fn scan(line: &str) -> Vec<Atom> {
    let body_start = line.len() - line.trim_start_matches(' ').len();
    let body = &line[body_start..];
    let bytes = body.as_bytes();
    let mut atoms = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        let start = i;
        match b {
            b' ' => i += 1,
            b',' => {
                atoms.push(Atom {
                    span: (body_start + start, body_start + start + 1),
                    kind: AtomKind::Comma,
                });
                i += 1;
            }
            b':' => {
                atoms.push(Atom {
                    span: (body_start + start, body_start + start + 1),
                    kind: AtomKind::Colon,
                });
                i += 1;
            }
            b'"' => {
                let (text, next) = read_string(body, i);
                atoms.push(Atom {
                    span: (body_start + start, body_start + next),
                    kind: AtomKind::Str(text),
                });
                i = next;
            }
            b'#' => {
                let digits: String = body[i + 1..]
                    .chars()
                    .take_while(|c| c.is_ascii_hexdigit())
                    .collect();
                if digits.len() == 6 || digits.len() == 8 {
                    atoms.push(Atom {
                        span: (body_start + start, body_start + start + 1 + digits.len()),
                        kind: AtomKind::Hex(format!("#{digits}")),
                    });
                    i = start + 1 + digits.len();
                } else {
                    let word = take_word(body, i);
                    atoms.push(Atom {
                        span: (body_start + start, body_start + word.1),
                        kind: AtomKind::Word(word.0),
                    });
                    i = word.1;
                }
            }
            b'0'..=b'9' => {
                let rest = &body[i..];
                let mut end = 0usize;
                let mut seen_dot = false;
                let rbytes = rest.as_bytes();
                while end < rbytes.len() {
                    let c = rbytes[end];
                    if c.is_ascii_digit() {
                        end += 1;
                    } else if c == b'.'
                        && !seen_dot
                        && end + 1 < rbytes.len()
                        && rbytes[end + 1].is_ascii_digit()
                    {
                        seen_dot = true;
                        end += 1;
                    } else {
                        break;
                    }
                }
                atoms.push(Atom {
                    span: (body_start + start, body_start + start + end),
                    kind: AtomKind::Number(rest[..end].to_owned()),
                });
                i = start + end;
            }
            _ => {
                let word = take_word(body, i);
                atoms.push(Atom {
                    span: (body_start + start, body_start + word.1),
                    kind: AtomKind::Word(word.0),
                });
                i = word.1;
            }
        }
    }
    atoms
}

/// A word: letters (any UTF-8), digits after a letter, `-`, `—`, anything
/// up to a space, comma, colon or quote — the same stops `tokenize` uses.
fn take_word(body: &str, start: usize) -> (String, usize) {
    let bytes = body.as_bytes();
    let mut i = start;
    let mut word = String::new();
    while i < bytes.len() {
        let b = bytes[i];
        if b == b' ' || b == b',' || b == b':' || b == b'"' {
            break;
        }
        let ch = body[i..].chars().next().unwrap_or(' ');
        word.push(ch);
        i += ch.len_utf8();
    }
    (word, i)
}

/// Read a `"…"`, decoding exactly as `lex::read_string` does. The decoding
/// matters for [`escape`]'s round trip, not just for finding the end.
fn read_string(body: &str, start: usize) -> (String, usize) {
    let mut text = String::new();
    let mut i = start + 1;
    let bytes = body.as_bytes();
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return (text, i + 1),
            b'\\' if i + 1 < bytes.len() => match bytes[i + 1] {
                // `\\(` is a literal paren; both consumed together.
                b'\\' if i + 2 < bytes.len() && bytes[i + 2] == b'(' => {
                    text.push('(');
                    i += 3;
                }
                b'\\' => {
                    text.push('\\');
                    i += 2;
                }
                b'(' => {
                    text.push('\\');
                    text.push('(');
                    i += 2;
                }
                b'n' => {
                    text.push('\n');
                    i += 2;
                }
                b't' => {
                    text.push('\t');
                    i += 2;
                }
                b'"' => {
                    text.push('"');
                    i += 2;
                }
                other => {
                    text.push('\\');
                    text.push(other as char);
                    i += 2;
                }
            },
            _ => {
                let ch = body[i..].chars().next().unwrap_or(' ');
                text.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    (text, bytes.len())
}

/// Encode `text` as a `"…"` literal, so it reads back as the same string.
///
/// The `\(` pair is the subtle one: in a literal it *starts an
/// interpolation*, so a decoded backslash-before-paren must survive as
/// `\(` while a lone backslash becomes `\\`. Everything else is the usual
/// quoting.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'(') => out.push_str("\\("),
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

/// The atom index where the phrase ends and the properties may begin.
///
/// The first `,`, `:`, event word or property word after the article —
/// `a text field with placeholder "…"` ends at `with`, `a column, …` at the
/// comma, `a button "Add one" which when tapped:` at `which`.
fn phrase_end(atoms: &[Atom]) -> Option<usize> {
    let first = atoms.first()?;
    if !matches!(&first.kind, AtomKind::Word(w) if w == "a" || w == "an" || w == "the") {
        return None;
    }
    let is_prop_word = |w: &str| {
        matches!(
            w,
            "padded"
                | "spaced"
                | "size"
                | "bold"
                | "radius"
                | "color"
                | "children"
                | "aligned"
                | "from"
                | "each"
                | "with"
                | "single"
                | "in"
                | "described"
                | "labelled"
                | "pinned"
                | "only"
                | "which"
        )
    };
    for (i, atom) in atoms.iter().enumerate().skip(1) {
        match &atom.kind {
            AtomKind::Comma | AtomKind::Colon => return Some(i),
            AtomKind::Word(w) if is_prop_word(w) => return Some(i),
            // `<N> wide` / `<N> tall` open the properties without a comma.
            AtomKind::Number(_)
                if atoms
                    .get(i + 1)
                    .and_then(|a| match &a.kind {
                        AtomKind::Word(w) => Some(w == "wide" || w == "tall"),
                        _ => None,
                    })
                    .unwrap_or(false) =>
            {
                return Some(i)
            }
            _ => {}
        }
    }
    Some(atoms.len())
}

/// Where `prop`'s clause sits in `atoms`, as `(start, end)` atom indices.
///
/// `None` when the line does not carry it. The search runs over the whole
/// property region — a `only if …` clause can sit between properties — but
/// stops at the event clause, whose `:` ends the line's own words.
fn prop_span(atoms: &[Atom], prop: &LineProp) -> Option<(usize, usize)> {
    let head = phrase_end(atoms)?;
    let word = |i: usize| atoms.get(i).and_then(Atom::word);
    let number = |i: usize| atoms.get(i).filter(|a| a.is_number());
    // The event clause: `which … :` — everything from `which` belongs to it.
    let stop = atoms
        .iter()
        .position(|a| matches!(&a.kind, AtomKind::Word(w) if w == "which"))
        .unwrap_or(atoms.len());
    let region = head..stop;
    let mut i = head;
    while i < stop {
        let found = match prop {
            LineProp::Padded(_) => word(i)
                .filter(|w| *w == "padded")
                .zip(number(i + 1))
                .map(|_| (i, i + 2)),
            LineProp::Spaced(_) => word(i)
                .filter(|w| *w == "spaced")
                .zip(number(i + 1))
                .map(|_| (i, i + 2)),
            LineProp::Size(_) => word(i)
                .filter(|w| *w == "size")
                .zip(number(i + 1))
                .map(|_| (i, i + 2)),
            LineProp::Radius(_) => word(i)
                .filter(|w| *w == "radius")
                .zip(number(i + 1))
                .map(|_| (i, i + 2)),
            LineProp::Wide(_) => number(i)
                .filter(|_| word(i + 1) == Some("wide"))
                .map(|_| (i, i + 2)),
            LineProp::Tall(_) => number(i)
                .filter(|_| word(i + 1) == Some("tall"))
                .map(|_| (i, i + 2)),
            LineProp::EachRow(_) => (word(i) == Some("each") && word(i + 1) == Some("row"))
                .then(|| number(i + 2).map(|_| (i, i + 3)))
                .flatten(),
            LineProp::Lines(_) => {
                // Both spellings parse — the documented `in N lines` and the
                // transposed `in lines N` — so both can be found to be
                // rewritten; the write always emits the documented one.
                if word(i) == Some("in") && word(i + 2) == Some("lines") && number(i + 1).is_some()
                {
                    Some((i, i + 3))
                } else if word(i) == Some("in")
                    && word(i + 1) == Some("lines")
                    && number(i + 2).is_some()
                {
                    Some((i, i + 3))
                } else {
                    None
                }
            }
            LineProp::From(_, _) => (word(i) == Some("from") && word(i + 2) == Some("to"))
                .then(|| match (number(i + 1), number(i + 3)) {
                    (Some(_), Some(_)) => Some((i, i + 4)),
                    _ => None,
                })
                .flatten(),
            LineProp::Pinned(side, _) => (word(i) == Some("pinned")
                && word(i + 1).map(|w| w == side) == Some(true))
            .then(|| number(i + 2).map(|_| (i, i + 3)))
            .flatten(),
            LineProp::Bold => (word(i) == Some("bold")).then_some((i, i + 1)),
            LineProp::SingleLine => {
                (word(i) == Some("single") && word(i + 1) == Some("line")).then_some((i, i + 2))
            }
            LineProp::Color(_) => color_span(atoms, i),
            LineProp::Aligned(_) => (word(i) == Some("aligned"))
                .then(|| align_span(atoms, i))
                .flatten(),
            LineProp::ChildrenAligned(_) => (word(i) == Some("children")
                && word(i + 1) == Some("aligned"))
            .then(|| align_span(atoms, i + 1).map(|(_, end)| (i, end)))
            .flatten(),
            LineProp::Placeholder(_) => (word(i) == Some("with")
                && word(i + 1) == Some("placeholder"))
            .then(|| matches!(&atoms.get(i + 2)?.kind, AtomKind::Str(_)).then_some((i, i + 3)))
            .flatten(),
            LineProp::DescribedAs(_) => (word(i) == Some("described") && word(i + 1) == Some("as"))
                .then(|| matches!(&atoms.get(i + 2)?.kind, AtomKind::Str(_)).then_some((i, i + 3)))
                .flatten(),
            LineProp::Labelled(_) => (word(i) == Some("labelled"))
                .then(|| matches!(&atoms.get(i + 1)?.kind, AtomKind::Str(_)).then_some((i, i + 2)))
                .flatten(),
        };
        if let Some(span) = found {
            if region.contains(&span.0) {
                return Some(span);
            }
        }
        i += 1;
    }
    None
}

/// The span of a colour clause at `i`: `color #RRGGBB`, `color red`, or
/// `with the <slot> color`.
fn color_span(atoms: &[Atom], i: usize) -> Option<(usize, usize)> {
    let w = |j: usize| {
        atoms.get(j).and_then(|a| match &a.kind {
            AtomKind::Word(word) => Some(word.as_str()),
            _ => None,
        })
    };
    if w(i) == Some("color") {
        return match &atoms.get(i + 1)?.kind {
            AtomKind::Hex(_) | AtomKind::Word(_) => Some((i, i + 2)),
            _ => None,
        };
    }
    if w(i) == Some("with") && w(i + 1) == Some("the") {
        let mut j = i + 2;
        while atoms
            .get(j)
            .is_some_and(|a| !matches!(a.kind, AtomKind::Comma | AtomKind::Colon))
        {
            if w(j) == Some("color") {
                return Some((i, j + 1));
            }
            j += 1;
        }
    }
    None
}

/// The span of an alignment clause starting at its keyword: `aligned (to)
/// (the) start`.
fn align_span(atoms: &[Atom], i: usize) -> Option<(usize, usize)> {
    let w = |j: usize| {
        atoms.get(j).and_then(|a| match &a.kind {
            AtomKind::Word(word) => Some(word.as_str()),
            _ => None,
        })
    };
    let mut j = i + 1;
    if w(j) == Some("to") {
        j += 1;
        if w(j) == Some("the") {
            j += 1;
        }
    }
    match w(j) {
        Some("start") | Some("left") | Some("top") | Some("center") | Some("middle")
        | Some("end") | Some("right") | Some("bottom") => Some((i, j + 1)),
        _ => None,
    }
}

/// The canonical spelling of `prop` with its value, as the parser reads it.
fn prop_text(prop: &LineProp) -> String {
    match prop {
        LineProp::Padded(n) => format!("padded {}", say_f32(*n)),
        LineProp::Spaced(n) => format!("spaced {}", say_f32(*n)),
        LineProp::Size(n) => format!("size {}", say_f32(*n)),
        LineProp::Radius(n) => format!("radius {}", say_f32(*n)),
        LineProp::Wide(n) => format!("{} wide", say_f32(*n)),
        LineProp::Tall(n) => format!("{} tall", say_f32(*n)),
        LineProp::EachRow(n) => format!("each row {}", say_f32(*n)),
        LineProp::Lines(n) => format!("in {} lines", n),
        LineProp::Bold => "bold".to_owned(),
        LineProp::SingleLine => "single line".to_owned(),
        LineProp::Color(c) => color_text(c),
        LineProp::Aligned(a) => format!("aligned to {}", a),
        LineProp::ChildrenAligned(a) => format!("children aligned to {}", a),
        LineProp::Placeholder(s) => format!("with placeholder \"{}\"", escape(s)),
        LineProp::DescribedAs(s) => format!("described as \"{}\"", escape(s)),
        LineProp::Labelled(s) => format!("labelled \"{}\"", escape(s)),
        LineProp::From(a, b) => format!("from {} to {}", say_f32(*a), say_f32(*b)),
        LineProp::Pinned(side, n) => format!("pinned {} {}", side, say_f32(*n)),
    }
}

/// A colour's clause: a hex or named colour carries `color`, a theme slot
/// carries `with`.
fn color_text(color: &str) -> String {
    if color.starts_with('#') {
        format!("color {color}")
    } else if color.starts_with("the ") {
        format!("with {color}")
    } else {
        format!("color {color}")
    }
}

/// A number as Say writes it: `16`, not `16.0` — whole values stay whole.
fn say_f32(n: f32) -> String {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "a property value outside i64 is not a dimension anybody edits"
    )]
    let whole = n as i64;
    if whole as f32 == n {
        whole.to_string()
    } else {
        n.to_string()
    }
}

/// Replace the byte range `start..end` with `replacement` — and nothing
/// else. The caller owns the separator logic; this owns the bytes.
fn replace_span(line: &str, start: usize, end: usize, replacement: &str) -> String {
    let mut out = String::with_capacity(line.len() + replacement.len() + 2);
    out.push_str(&line[..start]);
    out.push_str(replacement);
    out.push_str(&line[end..]);
    out
}

/// Remove the byte range of atoms `start..end`, with one adjacent comma and
/// the space it brought.
fn remove_span(line: &str, atoms: &[Atom], start: usize, end: usize) -> String {
    let (mut from, mut to) = (atoms[start].span.0, atoms[end - 1].span.1);
    // Swallow the comma before the clause; if none, the one after.
    if start > 0 && matches!(atoms[start - 1].kind, AtomKind::Comma) {
        from = atoms[start - 1].span.0;
    } else if matches!(atoms.get(end).map(|a| &a.kind), Some(AtomKind::Comma)) {
        to = atoms[end].span.1;
    }
    let mut out = String::with_capacity(line.len());
    out.push_str(&line[..from]);
    // One space of separation may now be doubled; take one back.
    let tail = &line[to..];
    if out.ends_with(' ') && tail.starts_with(' ') {
        out.pop();
    }
    out.push_str(tail);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const COUNTER: &str = include_str!("../tests/fixtures/counter.say");

    /// The counter, as the studio's tools should read it: four widget lines,
    /// the column's spacing, the heading/label/button values, in order.
    #[test]
    fn the_counter_reads_back_as_widget_lines() {
        let lines = parse_widget_lines(COUNTER);
        let phrases: Vec<&str> = lines.iter().map(|l| l.phrase).collect();
        assert_eq!(
            phrases,
            vec!["a column", "a heading", "a text", "a button"],
            "document order, properties excluded from the phrase"
        );
        assert_eq!(lines[0].line, 8);
        assert_eq!(
            lines[0].props,
            vec![
                LineProp::Spaced(16.0),
                LineProp::ChildrenAligned("the start".to_owned()),
            ]
        );
        assert_eq!(lines[1].value.as_deref(), Some("Counter"));
        assert_eq!(lines[2].value.as_deref(), Some("Tapped "));
        assert_eq!(lines[3].value.as_deref(), Some("Add one"));
    }

    /// A partially edited file still yields the lines that parse: the tool
    /// works while the compiler would refuse.
    #[test]
    fn a_broken_file_yields_what_parsed() {
        let source =
            "-- say-language: 1\nscreen \"A\":\n    a column:\n        a text \"x\"\n    ???\n";
        let lines = parse_widget_lines(source);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].value.as_deref(), Some("x"));
    }

    /// Every numeric property the canvas handles read, and the label
    /// vocabulary the inspector shows.
    #[test]
    fn the_numeric_properties_carry_their_values() {
        let source = concat!(
            "-- say-language: 1\n",
            "screen \"A\":\n",
            "    a card, padded 12, radius 8, 200 wide, 120 tall:\n",
            "        a row, spaced 4:\n",
            "            a text \"x\", size 14, bold, color #3366FF\n",
        );
        let lines = parse_widget_lines(source);
        let card = &lines[0];
        assert_eq!(
            card.props,
            vec![
                LineProp::Padded(12.0),
                LineProp::Radius(8.0),
                LineProp::Wide(200.0),
                LineProp::Tall(120.0),
            ]
        );
        assert_eq!(card.props[0].label(), "padding");
        assert_eq!(card.props[2].label(), "width");
        assert_eq!(lines[1].props, vec![LineProp::Spaced(4.0)]);
        assert_eq!(
            lines[2].props,
            vec![
                LineProp::Size(14.0),
                LineProp::Bold,
                LineProp::Color("#3366FF".to_owned()),
            ]
        );
    }

    /// The render names are the ones the generated code produces, so a
    /// picked render node can be matched by name.
    #[test]
    fn render_names_match_the_generated_widgets() {
        assert_eq!(WidgetKindName::Column.render_name(), Some("RenderColumn"));
        assert_eq!(WidgetKindName::Row.render_name(), Some("RenderRow"));
        assert_eq!(WidgetKindName::Text.render_name(), Some("RenderText"));
        assert_eq!(WidgetKindName::Button.render_name(), None);
    }

    // -- writing back -------------------------------------------------------

    /// Wrap one widget line into a compiling file, so a rewrite can be
    /// checked against the real parser rather than against expectations.
    fn file_with(line: &str) -> String {
        format!(
            "-- say-language: 1\nkeep a flag called on starting at true\nscreen \"A\":\n    {line}\n"
        )
    }

    /// The rewritten line, read back, says what it was rewritten to say —
    /// and the line it came from is untouched where it was not edited.
    #[test]
    fn a_rewritten_property_round_trips_through_the_parser() {
        let cases: [(&str, LineProp, &str); 10] = [
            (
                "a card, padded 12, radius 8:",
                LineProp::Padded(24.0),
                "a card, padded 24, radius 8:",
            ),
            (
                "a column, spaced 16, children aligned to the start:",
                LineProp::Spaced(8.0),
                "a column, spaced 8, children aligned to the start:",
            ),
            (
                "a card, 200 wide, 120 tall:",
                LineProp::Wide(300.0),
                "a card, 300 wide, 120 tall:",
            ),
            (
                "a card, 200 wide, 120 tall:",
                LineProp::Tall(240.0),
                "a card, 200 wide, 240 tall:",
            ),
            (
                "a text \"x\", size 14, bold:",
                LineProp::Size(21.0),
                "a text \"x\", size 21, bold:",
            ),
            (
                "a text \"x\", color #3366FF:",
                LineProp::Color("#CC00CC".to_owned()),
                "a text \"x\", color #CC00CC:",
            ),
            (
                "a text \"x\", with the primary color:",
                LineProp::Color("the error color".to_owned()),
                "a text \"x\", with the error color:",
            ),
            (
                "a column, children aligned to the center:",
                LineProp::ChildrenAligned("the end".to_owned()),
                "a column, children aligned to the end:",
            ),
            (
                "a card, aligned to the start:",
                LineProp::Aligned("the center".to_owned()),
                "a card, aligned to the center:",
            ),
            (
                "a text \"x\", padded 8:",
                LineProp::Padded(0.5),
                "a text \"x\", padded 0.5:",
            ),
        ];
        for (before, prop, after) in cases {
            let rewritten = rewrite_prop(before, &prop)
                .unwrap_or_else(|| panic!("rewriting {before:?} with {prop:?}"));
            assert_eq!(rewritten, after, "rewriting {before:?} with {prop:?}");
            // The parser agrees: the file compiles, the line carries the new
            // value, and only one line exists — the `// say:` line numbers
            // the studio's rewrite depends on are stable by construction.
            let file = file_with(&rewritten);
            let compiled = crate::compile("t.say", &file)
                .unwrap_or_else(|d| panic!("{before:?} -> {after:?} does not compile: {d:?}"));
            let reread = parse_widget_lines(&file);
            let found = reread[0]
                .props
                .iter()
                .any(|p| *p == prop)
                .then(|| format!("{prop:?} is on the line"))
                .unwrap_or_else(|| format!("{:?} in {:?}", prop, reread[0].props));
            assert!(found.contains("is on the line"), "{rewritten:?}: {found}");
            let _ = compiled;
        }
    }

    /// `in N lines` is the spelling the parser documents, and `in lines N`
    /// the one its first release actually accepted — both parse, both are
    /// found on the line, and both rewrite to the documented spelling, so
    /// a file written against either keeps editing.
    #[test]
    fn both_in_lines_spellings_parse_and_normalize() {
        let file = |line: &str| {
            format!("-- say-language: 1\nkeep text called draft\nscreen \"A\":\n    {line}\n")
        };
        for before in [
            "a text field bound to draft, in 3 lines:",
            "a text field bound to draft, in lines 3:",
        ] {
            let rewritten = rewrite_prop(before, &LineProp::Lines(5))
                .unwrap_or_else(|| panic!("rewriting {before:?}"));
            assert_eq!(rewritten, "a text field bound to draft, in 5 lines:");
            crate::compile("t.say", &file(&rewritten))
                .unwrap_or_else(|d| panic!("{rewritten:?} does not compile: {d:?}"));
        }
        for line in [
            "a text field bound to draft, in 5 lines:",
            "a text field bound to draft, in lines 5:",
        ] {
            crate::compile("t.say", &file(line))
                .unwrap_or_else(|d| panic!("{line:?} should compile: {d:?}"));
        }
    }

    /// A property the line does not carry is inserted, not refused — a
    /// canvas handle can grow a padding that was never written.
    #[test]
    fn an_absent_property_is_inserted_before_the_block_colon() {
        let cases: [(&str, LineProp, &str); 5] = [
            ("a column:", LineProp::Padded(12.0), "a column, padded 12:"),
            (
                "a column, spaced 4:",
                LineProp::Padded(12.0),
                "a column, spaced 4, padded 12:",
            ),
            ("a card:", LineProp::Wide(200.0), "a card, 200 wide:"),
            (
                "a button \"Add one\" which when tapped:",
                LineProp::Padded(8.0),
                "a button \"Add one\", padded 8 which when tapped:",
            ),
            (
                "a text \"x\", bold",
                LineProp::Size(14.0),
                "a text \"x\", bold, size 14",
            ),
        ];
        for (before, prop, after) in cases {
            let rewritten = rewrite_prop(before, &prop)
                .unwrap_or_else(|| panic!("inserting {prop:?} into {before:?}"));
            assert_eq!(rewritten, after, "inserting {prop:?} into {before:?}");
            assert!(
                crate::compile("t.say", &file_with(&rewritten)).is_ok(),
                "{rewritten:?} still compiles"
            );
        }
    }

    /// Removing a property takes one comma with it, never both.
    #[test]
    fn a_removed_property_takes_one_comma() {
        assert_eq!(
            remove_prop("a card, padded 12, radius 8:", &LineProp::Padded(12.0)).as_deref(),
            Some("a card, radius 8:")
        );
        assert_eq!(
            remove_prop("a card, padded 12, radius 8:", &LineProp::Radius(8.0)).as_deref(),
            Some("a card, padded 12:")
        );
        assert_eq!(
            remove_prop("a card, padded 12:", &LineProp::Padded(12.0)).as_deref(),
            Some("a card:")
        );
        assert_eq!(
            remove_prop("a card:", &LineProp::Padded(12.0)),
            None,
            "nothing to remove: refuse rather than invent"
        );
        let gone = remove_prop("a card, padded 12:", &LineProp::Padded(12.0)).unwrap();
        assert!(crate::compile("t.say", &file_with(&gone)).is_ok());
    }

    /// Replacing a line's words rewrites the literal and nothing else, and
    /// refuses an interpolated one rather than dropping the `\(…)`.
    #[test]
    fn a_value_rewrite_replaces_the_literal() {
        assert_eq!(
            rewrite_value("a text \"hello\":", "goodbye").as_deref(),
            Some("a text \"goodbye\":")
        );
        assert_eq!(
            rewrite_value("a button \"Add one\" which when tapped:", "Add two").as_deref(),
            Some("a button \"Add two\" which when tapped:")
        );
        // Escaping survives the round trip.
        let quoted = rewrite_value("a text \"x\":", "say \"hi\" \\ back").unwrap();
        assert_eq!(quoted, "a text \"say \\\"hi\\\" \\\\ back\":");
        let reread = parse_widget_lines(&file_with(&quoted));
        assert_eq!(reread[0].value.as_deref(), Some("say \"hi\" \\ back"));
        // Interpolated values are refused.
        assert_eq!(
            rewrite_value("a label \"Tapped \\(count) times\":", "x"),
            None
        );
        assert_eq!(rewrite_value("a column:", "x"), None);
    }

    /// The line→field→line identity the inspector promises: rewriting a
    /// property with the value it already has changes nothing.
    #[test]
    fn rewriting_with_the_written_value_is_the_identity() {
        let source = "a column, spaced 16, children aligned to the start, padded 12:";
        let read = parse_widget_lines(&file_with(source));
        for prop in &read[0].props {
            let again = rewrite_prop(source, prop).unwrap();
            assert_eq!(again, source, "rewriting {prop:?} back onto itself");
        }
    }

    /// Depths mirror the tree the render-tree bridge matches against.
    #[test]
    fn depths_mirror_the_nesting() {
        let source = concat!(
            "-- say-language: 1\n",
            "screen \"A\":\n",
            "    a column:\n",
            "        a row:\n",
            "            a text \"x\"\n",
            "        a text \"y\"\n",
        );
        let lines = parse_widget_lines(source);
        let depths: Vec<u32> = lines.iter().map(|l| l.depth).collect();
        assert_eq!(depths, vec![0, 1, 2, 1]);
        assert_eq!(lines[2].line, 5);
    }

    /// A property keyword inside the words of a literal is not a property.
    #[test]
    fn property_words_inside_a_literal_are_untouched() {
        let line = "a text \"padded 12 and 40 wide\":";
        assert_eq!(
            rewrite_prop(line, &LineProp::Padded(4.0)).as_deref(),
            Some("a text \"padded 12 and 40 wide\", padded 4:"),
            "the literal's words are not a clause; the property is inserted"
        );
    }
}
