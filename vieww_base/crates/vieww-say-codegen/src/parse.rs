//! Parsing, part one: the program tree and the top level — the pragma, the
//! app lines, `keep` declarations, screens and blocks. Widget lines, props,
//! expressions, conditions and actions live in `parse_line`, which continues
//! the same `Parser`.

use std::rc::Rc;

use crate::diag::{codes, Diagnostic};
use crate::lex::{Line, Tok};

// ---------------------------------------------------------------------------
// The tree

#[derive(Debug)]
pub(crate) struct Program {
    pub app: AppMeta,
    pub states: Vec<StateDecl>,
    pub screens: Vec<Screen>,
    /// Fields of the generated `SayModal` — the bound values a dialog's
    /// content edits. Empty unless some `open a dialog` block binds one.
    pub modal_fields: Vec<String>,
    /// The `the theme:` block: colour slots over the host's theme.
    pub theme: Vec<ThemeEntry>,
    /// Top-level `rust:` blocks, passed into the file verbatim.
    pub rust_items: Vec<RustBlock>,
    /// Compiles fine; the author would still want to know.
    pub warnings: Vec<Diagnostic>,
}

/// One line of `the theme:` — `primary is #3366FF`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ThemeEntry {
    pub slot: ThemeSlot,
    pub color: u32,
    pub line: u32,
}

/// Rust written inside a `.say` file, verbatim, with the line it starts on.
#[derive(Debug, Clone)]
pub(crate) struct RustBlock {
    pub code: String,
    pub line: u32,
}

#[derive(Debug, Default)]
pub(crate) struct AppMeta {
    pub name: Option<String>,
    pub reads_rtl: bool,
}

#[derive(Debug)]
pub(crate) struct StateDecl {
    pub name: String,
    pub kind: StateKind,
    pub init: Init,
    pub line: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StateKind {
    Whole,
    Number,
    Flag,
    /// Backed by a `TextEditingValue`, so the field's caret survives a
    /// rebuild — a bare `String` round-trip throws the caret to the end of
    /// the box while somebody types mid-word.
    Text,
    ListText,
    ListNumber,
}

#[derive(Debug)]
pub(crate) enum Init {
    Whole(i64),
    Number(f32),
    Flag(bool),
    Text(String),
    ListText(Vec<String>),
    ListNumber(Vec<f32>),
}

#[derive(Debug)]
pub(crate) struct Screen {
    pub title: String,
    pub transition: Option<Transition>,
    pub body: Vec<Node>,
    pub line: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Transition {
    Fade,
    Slide,
}

#[derive(Debug)]
pub(crate) enum Node {
    Widget(Widget),
    ForEach {
        var: String,
        list: String,
        body: Vec<Node>,
        line: u32,
    },
}

#[derive(Debug)]
pub(crate) struct Widget {
    pub kind: WKind,
    pub props: Vec<Prop>,
    pub event: Option<EventBlock>,
    pub only_if: Option<Cond>,
    pub children: Vec<Node>,
    pub line: u32,
    /// The canonical phrase, for the `// say:` marker and the registry docs.
    pub phrase: &'static str,
}

#[derive(Debug)]
pub(crate) struct EventBlock {
    pub kind: EventKind,
    /// The empty state's action label: `with the action "Add one" which when
    /// tapped:`.
    pub label: Option<String>,
    pub actions: Vec<ActionLine>,
}

#[derive(Debug)]
pub(crate) enum WKind {
    Card,
    Row,
    Column,
    Stack,
    Center,
    Fixed {
        width: Option<f32>,
        height: Option<f32>,
    },
    Text {
        value: Value,
        heading: bool,
    },
    Button {
        label: Value,
    },
    Fab {
        icon: Icon,
    },
    TextField {
        bound: String,
    },
    Switch {
        bound: String,
    },
    Checkbox {
        bound: String,
    },
    Slider {
        bound: String,
        from: f32,
        to: f32,
    },
    ListView {
        list: String,
        row: f32,
    },
    EmptyState {
        title: Value,
        icon: Icon,
    },
    IconOnly {
        icon: Icon,
    },
    /// `an image "photos/cat.png"` — a file, decoded at run time.
    Image {
        path: String,
    },
    /// `a scroll view:` — its children in a column that scrolls.
    ScrollView,
    /// `a set of tabs:` — a tab bar over the selected tab's children.
    Tabs,
    /// `a tab "Name":` — one page of a set of tabs.
    Tab {
        label: String,
    },
    /// `rust:` inside a block — an expression that becomes a widget.
    Rust {
        code: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Icon {
    Check,
    Close,
    Add,
    Remove,
    ChevronLeft,
    ChevronRight,
    ChevronUp,
    ChevronDown,
    ChevronBack,
    ChevronForward,
}

impl Icon {
    /// The `vieww::widget::icons` constructor, exactly as the review's C2 fix
    /// demands: these are *functions*, and every call site carries the parens.
    pub(crate) fn rust(self) -> &'static str {
        match self {
            Icon::Check => "vieww::widget::icons::check()",
            Icon::Close => "vieww::widget::icons::close()",
            Icon::Add => "vieww::widget::icons::add()",
            Icon::Remove => "vieww::widget::icons::remove()",
            Icon::ChevronLeft => "vieww::widget::icons::chevron_left()",
            Icon::ChevronRight => "vieww::widget::icons::chevron_right()",
            Icon::ChevronUp => "vieww::widget::icons::chevron_up()",
            Icon::ChevronDown => "vieww::widget::icons::chevron_down()",
            // Directional chevrons resolve against the reading direction; v1
            // screens read left-to-right (the app line warns otherwise), so
            // the LTR spelling is the honest one to generate.
            Icon::ChevronBack => "vieww::widget::icons::chevron_left()",
            Icon::ChevronForward => "vieww::widget::icons::chevron_right()",
        }
    }
}

#[derive(Debug)]
pub(crate) enum Prop {
    Padded(f32),
    Spaced(f32),
    ChildrenAligned(Cross),
    Aligned(Align),
    Size(f32),
    Bold,
    Color(ColorSpec),
    Radius(f32),
    Wide(f32),
    Tall(f32),
    Placeholder(String),
    SingleLine,
    Lines(u32),
    EachRow(f32),
    /// `from A to B` — a slider's range, written after the phrase.
    From {
        from: f32,
        to: f32,
    },
    Description(String),
    Labelled(String),
    Pin(Pin),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cross {
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Align {
    Start,
    Center,
    End,
}

#[derive(Debug)]
pub(crate) enum ColorSpec {
    Theme(ThemeSlot),
    Hex(u32),
    Named(NamedColor),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThemeSlot {
    Primary,
    OnPrimary,
    Surface,
    OnSurface,
    SurfaceVariant,
    OnSurfaceVariant,
    Outline,
    Error,
    OnError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NamedColor {
    Red,
    Green,
    Blue,
    Black,
    White,
    Transparent,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Pin {
    Top(f32),
    Bottom(f32),
    Left(f32),
    Right(f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EventKind {
    Tapped,
    Submitted,
    Action,
}

#[derive(Debug)]
pub(crate) struct ActionLine {
    pub kind: AKind,
    pub only_if: Option<Cond>,
    pub line: u32,
}

#[derive(Debug)]
pub(crate) enum AKind {
    Set { target: String, value: Expr },
    Add { target: String, value: Expr },
    Sub { target: String, value: Expr },
    Toggle { target: String },
    Append { target: String, value: Expr },
    RemoveAt { target: String, index: Expr },
    Clear { target: String },
    Open { screen: String },
    GoBack,
    Snackbar { msg: Value },
    CloseDialog,
    OpenDialog { title: String, body: Vec<Node> },
}

/// A string that may interpolate state: `"Tapped \(count) times"`.
#[derive(Debug, Clone)]
pub(crate) enum Value {
    Lit(String),
    Parts(Vec<Part>),
}

#[derive(Debug, Clone)]
pub(crate) enum Part {
    Lit(String),
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub(crate) enum Expr {
    Whole(i64),
    Num(f32),
    Str(String),
    Flag(bool),
    Ref {
        name: String,
        line: u32,
    },
    Len {
        list: String,
        line: u32,
    },
    Bin {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        line: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinOp {
    Add,
    Sub,
}

#[derive(Debug, Clone)]
pub(crate) enum Cond {
    Cmp { lhs: Expr, op: CmpOp, rhs: Expr },
    Flag { name: String },
    Contains { of: String, what: String },
    And(Box<Cond>, Box<Cond>),
    Or(Box<Cond>, Box<Cond>),
    Not(Box<Cond>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CmpOp {
    Eq,
    Ne,
    Gt,
    Lt,
    Ge,
    Le,
}

// ---------------------------------------------------------------------------
// The parser

pub(crate) struct Parser<'a> {
    pub(crate) file: &'a Rc<String>,
    pub(crate) lines: &'a [Line],
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) warnings: Vec<Diagnostic>,
    pub(crate) states: Vec<StateDecl>,
    pub(crate) screens: Vec<Screen>,
    pub(crate) app: AppMeta,
    pub(crate) screen_titles: Vec<String>,
    /// Loop variables in scope, enclosing-first.
    pub(crate) loops: Vec<(String, StateKind)>,
    /// Bound values opened by `open a dialog` blocks, in declaration order.
    pub(crate) modal_fields: Vec<String>,
    /// `open the screen "X"` seen before all screens are known.
    pub(crate) pending_screen_refs: Vec<(String, u32)>,
    pub(crate) has_go_back: bool,
    pub(crate) inside_dialog: usize,
    pub(crate) theme: Vec<ThemeEntry>,
    pub(crate) rust_items: Vec<RustBlock>,
    /// The indentation a `a tab` line may sit at: one deeper than each open
    /// `a set of tabs`.
    pub(crate) open_tabs: Vec<u32>,
}

/// Parse a lexed file. Collects every error it can before failing, so a
/// beginner sees their whole file's problems in one Render rather than one.
pub(crate) fn parse(file: &Rc<String>, lines: &[Line]) -> Result<Program, Vec<Diagnostic>> {
    let mut parser = Parser {
        file,
        lines,
        diagnostics: Vec::new(),
        warnings: Vec::new(),
        states: Vec::new(),
        screens: Vec::new(),
        app: AppMeta::default(),
        screen_titles: Vec::new(),
        loops: Vec::new(),
        modal_fields: Vec::new(),
        pending_screen_refs: Vec::new(),
        has_go_back: false,
        inside_dialog: 0,
        theme: Vec::new(),
        rust_items: Vec::new(),
        open_tabs: Vec::new(),
    };
    parser.top_level();
    parser.cross_checks();
    if parser.diagnostics.is_empty() {
        Ok(Program {
            app: parser.app,
            states: parser.states,
            screens: parser.screens,
            modal_fields: parser.modal_fields,
            theme: parser.theme,
            rust_items: parser.rust_items,
            warnings: parser.warnings,
        })
    } else {
        Err(parser.diagnostics)
    }
}

impl<'a> Parser<'a> {
    pub(crate) fn error(
        &mut self,
        line: u32,
        column: u32,
        code: &'static str,
        message: impl Into<String>,
    ) {
        self.diagnostics
            .push(Diagnostic::error(self.file, line, column, code, message));
    }

    pub(crate) fn warn(
        &mut self,
        line: u32,
        column: u32,
        code: &'static str,
        message: impl Into<String>,
    ) {
        self.warnings
            .push(Diagnostic::warning(self.file, line, column, code, message));
    }

    pub(crate) fn reserved(&mut self, line: u32, column: u32, what: &str, instead: &str) {
        self.error(
            line,
            column,
            codes::E050,
            format!("{what} is reserved for a later version of Say. {instead}"),
        );
    }

    /// The type of a name in scope: state, a loop variable, or nothing.
    pub(crate) fn kind_of(&self, name: &str) -> Option<StateKind> {
        if let Some(decl) = self.states.iter().find(|s| s.name == name) {
            return Some(decl.kind);
        }
        self.loops
            .iter()
            .rev()
            .find(|(var, _)| var == name)
            .map(|(_, kind)| *kind)
    }

    pub(crate) fn known_list(&self) -> String {
        let mut names: Vec<&str> = self.states.iter().map(|s| s.name.as_str()).collect();
        names.extend(self.loops.iter().map(|(v, _)| v.as_str()));
        if names.is_empty() {
            "none yet".to_owned()
        } else {
            names.join(", ")
        }
    }

    /// Checks that need the whole file: forward screen references, `go back`
    /// with nowhere to go, and dialog fields that were never bound.
    fn cross_checks(&mut self) {
        let titles = self.screen_titles.clone();
        for (title, line) in std::mem::take(&mut self.pending_screen_refs) {
            if !titles.contains(&title) {
                let nearest = titles
                    .iter()
                    .min_by_key(|known| edit_distance(known, &title))
                    .map_or_else(String::new, |known| {
                        format!(" Did you mean the screen \"{known}\"?")
                    });
                self.error(
                    line,
                    1,
                    codes::E090,
                    format!("there is no screen \"{title}\" in this file.{nearest}"),
                );
            }
        }
        if self.has_go_back && self.screens.len() < 2 {
            self.error(
                self.screens.first().map_or(1, |s| s.line),
                1,
                codes::E090,
                "`go back` needs a second screen to go back to",
            );
        }
    }

    // -- top level ---------------------------------------------------------

    fn top_level(&mut self) {
        let mut i;

        // The pragma is the first non-blank, non-comment line. It is
        // comment-shaped, so the lexer recognises it *before* comment
        // stripping and hands it over as a flag (§3 vs §8.2, settled in the
        // lexer's favour: comments may precede it, nothing else may).
        let first = self.lines.iter().position(|l| l.indent.is_some());
        match first {
            Some(index) if self.lines[index].pragma => i = index + 1,
            Some(index) => {
                let number = self.lines[index].number;
                self.error(
                    number,
                    1,
                    codes::E001,
                    "a Say file starts with the line `-- say-language: 1`",
                );
                return;
            }
            None => {
                self.error(1, 1, codes::E001, "this file is empty");
                return;
            }
        }

        while i < self.lines.len() {
            let (number, indent, words) = match self.lines.get(i) {
                Some(l) => (l.number, l.indent, line_words(l)),
                None => break,
            };
            let Some(indent) = indent else {
                i += 1;
                continue;
            };
            if indent > 0 {
                self.error(
                    number,
                    1,
                    codes::E010,
                    "this line is indented, but nothing above it opens a block",
                );
                i = self.skip_block(i + 1, indent);
                continue;
            }
            match words.first().copied() {
                Some("keep") => {
                    if let Some((decl, next)) = self.parse_keep(i) {
                        self.states.push(decl);
                        i = next;
                    } else {
                        i += 1;
                    }
                }
                Some("screen") => {
                    if let Some((screen, next)) = self.parse_screen(i) {
                        self.screen_titles.push(screen.title.clone());
                        self.screens.push(screen);
                        i = next;
                    } else {
                        i += 1;
                    }
                }
                Some("the") if words.get(1).copied() == Some("app") => {
                    self.parse_app_line(i);
                    i += 1;
                }
                Some("the") if words.get(1).copied() == Some("theme") => {
                    i = self.parse_theme(i);
                }
                Some("widget") => {
                    self.reserved(
                        number,
                        1,
                        "a parameterised widget (`widget \"Name\" needing …`)",
                        "Write the screen out, or repeat the lines you need.",
                    );
                    i = self.skip_block(i + 1, 1);
                }
                _ if self.opens_rust(i) => {
                    if let Some((code, next)) = self.rust_block(i, 0) {
                        self.rust_items.push(RustBlock { code, line: number });
                        i = next;
                    } else {
                        i += 1;
                    }
                }
                _ => {
                    self.error(
                        number,
                        1,
                        codes::E060,
                        "a file holds `keep` state, `the app` lines, `the theme:`, `rust:` blocks and `screen` blocks — this line is none of them",
                    );
                    i += 1;
                }
            }
        }

        if self.screens.is_empty() && self.diagnostics.is_empty() {
            self.error(1, 1, codes::E010, "this file has no screen in it");
        }
    }

    /// Skip a block whose body starts at `next` with indentation deeper than
    /// `indent`, so one bad top-level line does not cascade.
    pub(crate) fn skip_block(&mut self, next: usize, indent: u32) -> usize {
        let mut i = next;
        while i < self.lines.len() {
            match self.lines[i].indent {
                Some(depth) if depth >= indent => i += 1,
                _ => break,
            }
        }
        i
    }

    // -- the app line ------------------------------------------------------

    fn parse_app_line(&mut self, index: usize) {
        let (number, tokens) = match self.lines.get(index) {
            Some(l) => (l.number, l.tokens.clone()),
            None => return,
        };
        let words: Vec<&str> = tokens.iter().filter_map(Tok::word).collect();
        // `the app is called "Name"` or `the app reads left to right`.
        if words.get(2).copied() == Some("is") && words.get(3).copied() == Some("called") {
            if let Some(Tok::Str { text, .. }) = tokens.get(4) {
                self.app.name = Some(text.clone());
            }
        } else if words.get(2).copied() == Some("reads") {
            let dir = words.get(3..).unwrap_or(&[]).join(" ");
            if dir == "right to left" {
                self.app.reads_rtl = true;
                self.warn(
                    number,
                    1,
                    codes::W130,
                    "right-to-left interfaces render left-to-right in this version; the switch arrives with Say 1.1",
                );
            }
        }
    }

    // -- keep --------------------------------------------------------------

    fn parse_keep(&mut self, index: usize) -> Option<(StateDecl, usize)> {
        let (number, toks) = self
            .lines
            .get(index)
            .map(|l| (l.number, l.tokens.clone()))?;
        let word = |i: usize| toks.get(i).and_then(Tok::word);

        // keep [a] <type words> called <name> starting at <values>
        let mut i = 1usize;
        if word(i) == Some("a") || word(i) == Some("an") {
            i += 1;
        }
        let mut type_words: Vec<&str> = Vec::new();
        while word(i).is_some() && word(i) != Some("called") {
            type_words.push(word(i)?);
            i += 1;
        }
        let kind = match type_words.join(" ").as_str() {
            "whole number" | "integer" => Some(StateKind::Whole),
            "number" | "decimal" => Some(StateKind::Number),
            "yes-or-no" | "flag" => Some(StateKind::Flag),
            "text" | "string" => Some(StateKind::Text),
            "list of text" | "list of strings" => Some(StateKind::ListText),
            "list of numbers" => Some(StateKind::ListNumber),
            _ => None,
        };
        let Some(kind) = kind else {
            self.error(
                number,
                1,
                codes::E060,
                "keep a whole number / a number / a yes-or-no / text / a list of text / a list of numbers, called <name>, starting at <value>",
            );
            return None;
        };
        // The scan above already sits on `called`.
        if word(i) != Some("called") {
            self.error(
                number,
                1,
                codes::E060,
                "`keep` reads `keep a <kind> called <name> starting at <value>`",
            );
            return None;
        }
        i += 1;
        let name = self.state_name(toks.get(i), number)?;
        i += 1;

        // `starting at …` is optional only for text, which may start empty.
        let has_start = word(i) == Some("starting") && word(i + 1) == Some("at");
        if !has_start && !matches!(kind, StateKind::Text) {
            self.error(
                number,
                1,
                codes::E060,
                "state starts somewhere: `starting at <value>`",
            );
            return None;
        }
        let init = if has_start {
            self.parse_init_values(kind, &toks, i + 2, number)?
        } else {
            Init::Text(String::new())
        };

        if self.states.iter().any(|s| s.name == name) {
            self.error(
                number,
                1,
                codes::E031,
                format!("two pieces of state are both called `{name}`"),
            );
            return None;
        }
        Some((
            StateDecl {
                name,
                kind,
                init,
                line: number,
            },
            index + 1,
        ))
    }

    fn state_name(&mut self, tok: Option<&Tok>, line: u32) -> Option<String> {
        match tok {
            Some(Tok::Word(w))
                if w.chars().next().is_some_and(|c| c.is_ascii_lowercase())
                    && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') =>
            {
                Some(w.clone())
            }
            other => {
                self.error(
                    line,
                    1,
                    codes::E031,
                    format!(
                        "a state name is lower-case letters, digits and `_` — got {}",
                        other.map_or("nothing".to_owned(), Tok::describe)
                    ),
                );
                None
            }
        }
    }

    fn parse_init_values(
        &mut self,
        kind: StateKind,
        toks: &[Tok],
        start: usize,
        line: u32,
    ) -> Option<Init> {
        let mut i = start;
        match kind {
            StateKind::Whole => match toks.get(i) {
                Some(Tok::Int(v)) => Some(Init::Whole(*v)),
                _ => {
                    self.error(
                        line,
                        1,
                        codes::E04X,
                        "a whole number starts at a whole number",
                    );
                    None
                }
            },
            StateKind::Number => match toks.get(i) {
                Some(Tok::Float(f)) => Some(Init::Number(*f)),
                Some(Tok::Int(v)) => Some(Init::Number(*v as f32)),
                _ => {
                    self.error(line, 1, codes::E04X, "a number starts at a number");
                    None
                }
            },
            StateKind::Flag => match toks.get(i).and_then(Tok::word) {
                Some("yes") | Some("true") => Some(Init::Flag(true)),
                Some("no") | Some("false") => Some(Init::Flag(false)),
                _ => {
                    self.error(line, 1, codes::E04X, "a yes-or-no starts at `yes` or `no`");
                    None
                }
            },
            StateKind::Text => match toks.get(i) {
                Some(Tok::Str { text, .. }) => Some(Init::Text(text.clone())),
                _ => {
                    self.error(line, 1, codes::E04X, "text starts at a quoted string");
                    None
                }
            },
            StateKind::ListText => {
                let mut items = Vec::new();
                while i < toks.len() {
                    match &toks[i] {
                        Tok::Str { text, .. } => items.push(text.clone()),
                        Tok::Comma => {}
                        other => {
                            self.error(
                                line,
                                1,
                                codes::E04X,
                                format!(
                                    "a list of text starts at quoted strings, separated by commas — got {}",
                                    other.describe()
                                ),
                            );
                            return None;
                        }
                    }
                    i += 1;
                }
                Some(Init::ListText(items))
            }
            StateKind::ListNumber => {
                let mut items = Vec::new();
                while i < toks.len() {
                    match toks[i] {
                        Tok::Float(f) => items.push(f),
                        Tok::Int(v) => items.push(v as f32),
                        Tok::Comma => {}
                        ref other => {
                            self.error(
                                line,
                                1,
                                codes::E04X,
                                format!(
                                    "a list of numbers starts at numbers, separated by commas — got {}",
                                    other.describe()
                                ),
                            );
                            return None;
                        }
                    }
                    i += 1;
                }
                Some(Init::ListNumber(items))
            }
        }
    }

    // -- screens -----------------------------------------------------------

    fn parse_screen(&mut self, index: usize) -> Option<(Screen, usize)> {
        let (number, toks) = self
            .lines
            .get(index)
            .map(|l| (l.number, l.tokens.clone()))?;
        let mut i = 1usize;
        let Some(Tok::Str { text: title, .. }) = toks.get(i) else {
            self.error(
                number,
                1,
                codes::E060,
                "a screen has a name: `screen \"Home\":`",
            );
            return None;
        };
        let title = title.clone();
        i += 1;

        let mut transition = None;
        let mut saw_colon = false;
        while i < toks.len() {
            match &toks[i] {
                Tok::Comma => i += 1,
                Tok::Colon => {
                    saw_colon = true;
                    break;
                }
                Tok::Word(w) if w == "with" => {
                    let words: Vec<String> = toks[i..]
                        .iter()
                        .map(|t| match t {
                            Tok::Word(w) => w.clone(),
                            other => other.describe(),
                        })
                        .collect();
                    let joined = words.join(" ");
                    if joined.starts_with("with a fade transition") {
                        transition = Some(Transition::Fade);
                    } else if joined.starts_with("with a slide transition") {
                        transition = Some(Transition::Slide);
                    } else if joined.starts_with("with no transition") {
                        transition = None;
                    } else {
                        self.error(
                            number,
                            1,
                            codes::E070,
                            "a screen takes `with a fade transition`, `with a slide transition` or `with no transition`",
                        );
                        return None;
                    }
                    // `with a <fade|slide> transition` is four words; `with
                    // no transition` is three.
                    i += if joined.starts_with("with no transition") {
                        3
                    } else {
                        4
                    };
                    if !matches!(&toks.get(i), Some(Tok::Colon) | Some(Tok::Comma) | None) {
                        self.error(
                            number,
                            1,
                            codes::E070,
                            "unexpected words after the screen's transition",
                        );
                        return None;
                    }
                }
                other => {
                    self.error(
                        number,
                        1,
                        codes::E070,
                        format!("unexpected {} after the screen's name", other.describe()),
                    );
                    return None;
                }
            }
        }
        if !saw_colon {
            self.error(
                number,
                1,
                codes::E010,
                "a screen block ends with `:` — everything indented under it is the screen",
            );
            return None;
        }

        let (body, next) = self.parse_block(index + 1, 1);
        Some((
            Screen {
                title,
                transition,
                body,
                line: number,
            },
            next,
        ))
    }

    /// Parse the lines of a block at `indent`, from `start`. Returns the
    /// nodes and the index of the first line that dedents out.
    pub(crate) fn parse_block(&mut self, start: usize, indent: u32) -> (Vec<Node>, usize) {
        let mut nodes = Vec::new();
        let mut i = start;
        while i < self.lines.len() {
            let (number, depth, words) = match self.lines.get(i) {
                Some(l) => (l.number, l.indent, line_words(l)),
                None => break,
            };
            let Some(depth) = depth else {
                i += 1;
                continue;
            };
            if depth < indent {
                break;
            }
            if depth > indent {
                self.error(
                    number,
                    1,
                    codes::E010,
                    "this line is indented further than the block it is in",
                );
                i += 1;
                continue;
            }
            if words.first().copied() == Some("for") && words.get(1).copied() == Some("each") {
                if let Some((node, next)) = self.parse_for_each(i, indent) {
                    nodes.push(node);
                    i = next;
                } else {
                    i += 1;
                }
                continue;
            }
            if self.opens_rust(i) {
                if let Some((code, next)) = self.rust_block(i, indent) {
                    nodes.push(Node::Widget(Widget {
                        kind: WKind::Rust { code },
                        props: Vec::new(),
                        event: None,
                        only_if: None,
                        children: Vec::new(),
                        line: number,
                        phrase: "rust",
                    }));
                    i = next;
                } else {
                    i += 1;
                }
                continue;
            }
            match self.parse_widget_line(i, indent) {
                Some((widget, next)) => {
                    nodes.push(Node::Widget(widget));
                    i = next;
                }
                None => i += 1,
            }
        }
        (nodes, i)
    }

    /// Whether line `index` opens a Rust block: `rust:` or a fence.
    pub(crate) fn opens_rust(&self, index: usize) -> bool {
        let Some(line) = self.lines.get(index) else {
            return false;
        };
        let body = line.raw.trim();
        body == "rust:" || body == "```rust"
    }

    /// The Rust in the block line `index` opens, dedented, and the index of
    /// the first line after it.
    ///
    /// Two spellings: `rust:` with the code indented under it, ended by the
    /// first line back at the block's own depth; or a fence, ```` ```rust ````
    /// down to a closing ```` ``` ````, whose lines may sit at the fence's
    /// own depth. Either way the code is passed through as written — Say
    /// does not read it — and rustc's errors point into the generated file
    /// beside the `// say:` marker for this line.
    pub(crate) fn rust_block(&mut self, index: usize, indent: u32) -> Option<(String, usize)> {
        let number = self.lines.get(index)?.number;
        let fenced = self.lines[index].raw.trim() == "```rust";
        let (strip, mut i) = if fenced {
            (indent as usize * 4, index + 1)
        } else {
            ((indent as usize + 1) * 4, index + 1)
        };
        let mut code: Vec<String> = Vec::new();
        let mut closed = !fenced;
        while i < self.lines.len() {
            let raw = &self.lines[i].raw;
            if fenced {
                if raw.trim() == "```" {
                    closed = true;
                    i += 1;
                    break;
                }
            } else if !raw.trim().is_empty() {
                let depth = (raw.len() - raw.trim_start_matches(' ').len()) / 4;
                if depth <= indent as usize {
                    break;
                }
            }
            let cut = raw.len() - raw.trim_start_matches(' ').len();
            code.push(raw[cut.min(strip)..].to_owned());
            i += 1;
        }
        while code.last().is_some_and(|l| l.trim().is_empty()) {
            code.pop();
        }
        if !closed {
            self.error(
                number,
                1,
                codes::E010,
                "this ```rust block is never closed — end it with a line holding only ```",
            );
            return None;
        }
        if code.is_empty() {
            self.error(
                number,
                1,
                codes::E010,
                "this Rust block is empty — indent the code under `rust:`",
            );
            return None;
        }
        Some((code.join("\n"), i))
    }

    /// `the theme:` and its lines: `<slot> is #RRGGBB`.
    fn parse_theme(&mut self, index: usize) -> usize {
        let number = self.lines[index].number;
        if self.lines[index].tokens.last() != Some(&Tok::Colon) {
            self.error(
                number,
                1,
                codes::E010,
                "the theme is a block: `the theme:`, with a colour per line under it",
            );
            return index + 1;
        }
        if !self.theme.is_empty() {
            self.error(
                number,
                1,
                codes::E031,
                "this file already has a `the theme:` block",
            );
            return self.skip_block(index + 1, 1);
        }
        let mut i = index + 1;
        while i < self.lines.len() {
            let line = &self.lines[i];
            let Some(depth) = line.indent else {
                i += 1;
                continue;
            };
            if depth == 0 {
                break;
            }
            let number = line.number;
            let toks = line.tokens.clone();
            let mut words: Vec<&str> = toks.iter().filter_map(Tok::word).collect();
            if words.first() == Some(&"the") {
                words.remove(0);
            }
            let is = words.iter().position(|w| *w == "is");
            let color = toks.iter().find_map(|t| match t {
                Tok::Hex(h) => Some(*h),
                _ => None,
            });
            let slot_words = is.map(|at| {
                let mut slot = words[..at].to_vec();
                if slot.last() == Some(&"color") || slot.last() == Some(&"colour") {
                    slot.pop();
                }
                slot.join(" ")
            });
            let slot = match slot_words.as_deref() {
                Some("primary") => Some(ThemeSlot::Primary),
                Some("on primary") => Some(ThemeSlot::OnPrimary),
                Some("surface") => Some(ThemeSlot::Surface),
                Some("on surface") => Some(ThemeSlot::OnSurface),
                Some("surface variant") => Some(ThemeSlot::SurfaceVariant),
                Some("on surface variant") => Some(ThemeSlot::OnSurfaceVariant),
                Some("outline") => Some(ThemeSlot::Outline),
                Some("error") => Some(ThemeSlot::Error),
                Some("on error") => Some(ThemeSlot::OnError),
                _ => None,
            };
            match (slot, color) {
                (Some(slot), Some(color)) => {
                    if self.theme.iter().any(|e| e.slot == slot) {
                        self.error(
                            number,
                            1,
                            codes::E031,
                            "this colour is already set earlier in the theme",
                        );
                    } else {
                        self.theme.push(ThemeEntry {
                            slot,
                            color,
                            line: number,
                        });
                    }
                }
                (None, _) => self.error(
                    number,
                    1,
                    codes::E060,
                    "a theme line names a colour slot — primary, on primary, surface, on surface, surface variant, on surface variant, outline, error or on error — then `is #RRGGBB`",
                ),
                (Some(_), None) => self.error(
                    number,
                    1,
                    codes::E04X,
                    "a theme colour is written `#RRGGBB`",
                ),
            }
            i += 1;
        }
        if self.theme.is_empty() {
            self.error(number, 1, codes::E010, "this theme sets no colours");
        }
        i
    }

    fn parse_for_each(&mut self, index: usize, indent: u32) -> Option<(Node, usize)> {
        let (number, toks) = self
            .lines
            .get(index)
            .map(|l| (l.number, l.tokens.clone()))?;
        // for each <var> in <list>:
        if toks.len() != 6
            || toks.get(5) != Some(&Tok::Colon)
            || toks.get(3).and_then(Tok::word) != Some("in")
        {
            self.error(
                number,
                1,
                codes::E060,
                "a repetition reads `for each <name> in <list>:`",
            );
            return None;
        }
        let var = toks.get(2).and_then(Tok::word)?.to_owned();
        if var.chars().next().is_some_and(|c| !c.is_ascii_lowercase()) {
            self.error(number, 1, codes::E031, "a loop variable is lower-case");
            return None;
        }
        let list = toks.get(4).and_then(Tok::word)?.to_owned();
        let list_kind = self
            .kind_of(&list)
            .filter(|k| matches!(k, StateKind::ListText | StateKind::ListNumber));
        let elem = match list_kind {
            Some(StateKind::ListText) => StateKind::Text,
            Some(StateKind::ListNumber) => StateKind::Number,
            _ => {
                self.error(
                    number,
                    1,
                    codes::E031,
                    format!("`{list}` is not a list — known: {}", self.known_list()),
                );
                return None;
            }
        };
        self.loops.push((var.clone(), elem));
        let (body, next) = self.parse_block(index + 1, indent + 1);
        self.loops.pop();
        Some((
            Node::ForEach {
                var,
                list,
                body,
                line: number,
            },
            next,
        ))
    }
}

/// The words of a line, cloned out so the parser never holds a borrow across
/// a `&mut self` call.
pub(crate) fn line_words(line: &Line) -> Vec<&str> {
    line.tokens.iter().filter_map(Tok::word).collect()
}

/// Levenshtein distance, small enough to hand-roll and good enough to point
/// at the screen the author almost certainly meant.
pub(crate) fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut table = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for (i, row) in table.iter_mut().enumerate() {
        row[0] = i as u32;
    }
    for (j, cell) in table[0].iter_mut().enumerate() {
        *cell = j as u32;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = u32::from(a[i - 1] != b[j - 1]);
            table[i][j] = (table[i - 1][j] + 1)
                .min(table[i][j - 1] + 1)
                .min(table[i - 1][j - 1] + cost);
        }
    }
    table[a.len()][b.len()] as usize
}
