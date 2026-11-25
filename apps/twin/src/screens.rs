//! The eight screens and the shell (status line + tab bar) around them.

use std::rc::Rc;

use vieww::foundation::{Border, Color, EdgeInsets, FontWeight, Size};
use vieww::prelude::*;
use vieww::widget::Pressable;

use crate::pixel::{Avatar, Morse, Place, Scene};
use vieww_ui_kit::Kind;
use crate::{style, Motion, TwinState, AMBER, BLUE, EDGE, GROUND, INK, MINT, MUTED, PANEL, PX};

/// The eight screens. Four are tabs; Call, Friends and Key are reached from
/// inside the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Screen {
    Home,
    World,
    Chat,
    Call,
    Teach,
    Mind,
    Friends,
    Key,
}

impl Screen {
    pub const ALL: [Screen; 8] = [
        Screen::Home, Screen::World, Screen::Chat, Screen::Call,
        Screen::Teach, Screen::Mind, Screen::Friends, Screen::Key,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Screen::Home => "home",
            Screen::World => "world",
            Screen::Chat => "chat",
            Screen::Call => "call",
            Screen::Teach => "teach",
            Screen::Mind => "mind",
            Screen::Friends => "friends",
            Screen::Key => "key",
        }
    }
}

// ---- small parts -------------------------------------------------------------

fn text(s: &str, size: f32, weight: FontWeight, color: Color) -> WidgetNode {
    Text::new(s.to_owned()).style(style(size, weight, color, 0.0)).into()
}

fn body(s: &str, color: Color) -> WidgetNode {
    text(s, 14.0, FontWeight::Regular, color)
}

/// The pixel face's stand-in: small tracked capitals.
fn pixel_label(s: &str, color: Color) -> WidgetNode {
    Text::new(s.to_uppercase())
        .style(style(10.0, FontWeight::Bold, color, 2.4))
        .into()
}

fn flat(fill: Color, edge: Color, radius: f32, child: WidgetNode, pad: EdgeInsets) -> WidgetNode {
    Container::new()
        .decoration(BoxDecoration::new().color(fill).radius(radius).border(Border::new(edge, 1.0)))
        .padding(pad)
        .child(child)
        .into()
}

fn card(child: WidgetNode) -> WidgetNode {
    vieww_ui_kit::card(&crate::skin(), child)
}

fn col(gap: f32, children: Vec<WidgetNode>) -> WidgetNode {
    Flex::column().spacing(gap).cross_axis_alignment(CrossAxisAlignment::Start).children(children).into()
}

fn row(gap: f32, children: Vec<WidgetNode>) -> WidgetNode {
    Flex::row().spacing(gap).main_axis_size(MainAxisSize::Max).cross_axis_alignment(CrossAxisAlignment::Center).children(children).into()
}

fn grow(child: WidgetNode) -> WidgetNode {
    Flexible::new(1).child(child).into()
}

fn space(h: f32) -> WidgetNode {
    SizedBox::height(h).into()
}

/// A kit button of `kind`.
fn kbutton(label: &str, kind: vieww_ui_kit::Kind, on_tap: impl Fn() + 'static) -> WidgetNode {
    vieww_ui_kit::button(&crate::skin(), label, kind, on_tap)
}

/// A kit chip in `color` (quick replies, the key shortcut).
fn kchip(label: &str, color: Color, on_tap: impl Fn() + 'static) -> WidgetNode {
    vieww_ui_kit::chip(&crate::skin(), label, color, Rc::new(on_tap))
}

fn title(s: &str) -> WidgetNode {
    text(s, 22.0, FontWeight::Bold, INK)
}

fn header(s: &str, sub: &str) -> WidgetNode {
    col(4.0, vec![title(s), body(sub, MUTED)])
}

fn page(children: Vec<WidgetNode>) -> WidgetNode {
    Container::new()
        .padding(EdgeInsets::symmetric(16.0, 8.0))
        .child(Flex::column().cross_axis_alignment(CrossAxisAlignment::Stretch).spacing(14.0).children(children))
        .into()
}

fn avatar(scale: f32, motion: Motion) -> WidgetNode {
    let mut a = Avatar::new(scale);
    if motion == Motion::Ambient {
        a.lift = 0; // the sway is driven by the ticker; the held frame is rest
    }
    a.widget()
}

fn bubble(s: &str, mine: bool) -> WidgetNode {
    let (fill, edge) = if mine { (Color::rgb(0x12, 0x2A, 0x1E), MINT.with_alpha(110)) } else { (PANEL, EDGE) };
    flat(fill, edge, 4.0, body(s, INK), EdgeInsets::symmetric(14.0, 10.0))
}

fn e2e_banner() -> WidgetNode {
    flat(
        Color::rgb(0x0E, 0x1E, 0x18),
        MINT.with_alpha(70),
        4.0,
        row(10.0, vec![
            Container::new().width(PX * 3.0).height(PX * 3.0).color(MINT).into(),
            grow(text("end-to-end encrypted — the server relays, never reads", 12.0, FontWeight::Regular, MINT)),
        ]),
        EdgeInsets::symmetric(12.0, 9.0),
    )
}

// ---- screens -----------------------------------------------------------------

fn home(state: &Rc<TwinState>) -> WidgetNode {
    let s = state.clone();
    let (s2, s3, s4) = (state.clone(), state.clone(), state.clone());
    let room = Stack::new()
        .fit(StackFit::Expand)
        .children(vec![
            Scene { place: Place::Room }.widget(Size::new(358.0, 300.0)),
            Positioned::new().left(130.0).top(96.0).child(avatar(10.0, state.motion)).into(),
            Positioned::new().left(212.0).top(86.0).child(flat(
                Color::rgb(0x0E, 0x1E, 0x18), MINT, 3.0,
                text("i read about lichens today.", 12.0, FontWeight::Regular, INK),
                EdgeInsets::symmetric(10.0, 8.0))).into(),
        ]);
    page(vec![
        text("twin", 22.0, FontWeight::Bold, INK),
        SizedBox::height(300.0).child(room).into(),
        col(6.0, vec![
            vieww_ui_kit::section_label(&crate::skin(), "knowledge"),
            row(10.0, vec![
                grow(SizedBox::height(6.0).child(Row2::bar(0.64)).into()),
                text("1,842 notes · +3 today", 11.0, FontWeight::Regular, MINT),
            ]),
        ]),
        row(8.0, vec![
            grow(kbutton("message", Kind::Primary, move || s2.screen.set(Screen::Chat))),
            grow(kbutton("call", Kind::Quiet, move || s3.screen.set(Screen::Call))),
            grow(kbutton("teach", Kind::Quiet, move || s4.screen.set(Screen::Teach))),
        ]),
        vieww_ui_kit::section_label(&crate::skin(), "recent moments"),
        moment("visited the north gate with kira's twin", "world · linden", "21:12", AMBER),
        moment("asked about your day", "voice message · 0:42", "19:05", BLUE),
        {
            let _ = s;
            moment("learned: lichen is a symbiosis", "new note · tagged #nature", "14:30", MINT)
        },
    ])
}

/// A thin progress bar.
struct Row2;
impl Row2 {
    fn bar(frac: f32) -> WidgetNode {
        Stack::new()
            .fit(StackFit::Expand)
            .children(vec![
                Container::new().color(EDGE).into(),
                Positioned::new().left(0.0).top(0.0).bottom(0.0).width(300.0 * frac).child(Container::new().color(MINT)).into(),
            ])
            .into()
    }
}

fn moment(what: &str, meta: &str, at: &str, accent: Color) -> WidgetNode {
    card(row(12.0, vec![
        Container::new().width(PX * 3.0).height(PX * 3.0).color(accent).into(),
        grow(col(2.0, vec![text(what, 14.0, FontWeight::Medium, INK), text(meta, 11.0, FontWeight::Regular, MUTED)])),
        text(at, 11.0, FontWeight::Regular, MUTED),
    ]))
}

fn world(state: &Rc<TwinState>) -> WidgetNode {
    let scene = Stack::new()
        .fit(StackFit::Expand)
        .children(vec![
            Scene { place: Place::World }.widget(Size::new(358.0, 560.0)),
            Positioned::new().left(150.0).top(330.0).child(avatar(6.0, state.motion)).into(),
            Positioned::new().left(236.0).top(350.0).child(Avatar { shirt: Color::rgb(0x57, 0x2C, 0x3A), ..Avatar::new(6.0) }.widget()).into(),
            Positioned::new().left(12.0).top(12.0).child(pixel_label("linden · north gate", MINT)).into(),
        ]);
    page(vec![
        header("world", "lamps are on tonight"),
        SizedBox::height(560.0).child(scene).into(),
        row(8.0, vec![
            grow(kbutton("go to the gate", Kind::Primary, || {})),
            grow(kbutton("worlds", Kind::Quiet, || {})),
        ]),
    ])
}

fn chat(state: &Rc<TwinState>) -> WidgetNode {
    let s = state.clone();
    let wave: Vec<WidgetNode> = (0..28)
        .map(|i| {
            let h = 6.0 + ((i * 7 % 11) as f32) * 2.2;
            Container::new().width(PX).height(h).color(if i < 14 { MINT } else { EDGE }).into()
        })
        .collect();
    Container::new()
        .padding(EdgeInsets::symmetric(16.0, 8.0))
        .child(Flex::column().cross_axis_alignment(CrossAxisAlignment::Stretch).spacing(10.0).children(vec![
            row(10.0, vec![
                avatar(3.0, state.motion),
                grow(col(2.0, vec![text("mara", 15.0, FontWeight::Bold, INK), text("awake · this device", 11.0, FontWeight::Regular, MINT)])),
                kchip("key", AMBER, move || s.screen.set(Screen::Key)),
            ]),
            Morse::new("safety morse", 3.0, AMBER).widget(),
            e2e_banner(),
            Container::new().alignment(Alignment::CENTER).child(text("today · 21:47", 11.0, FontWeight::Regular, MUTED)).into(),
            Align::new(Alignment::CENTER_LEFT).child(bubble("good evening.\nthe north gate lights are on tonight.", false)).into(),
            Align::new(Alignment::CENTER_RIGHT).child(bubble("that sounds nice — tell me more", true)).into(),
            card(row(10.0, vec![
                text("▶", 16.0, FontWeight::Bold, INK),
                Flex::row().spacing(2.0).cross_axis_alignment(CrossAxisAlignment::Center).children(wave).into(),
                text("0:18", 11.0, FontWeight::Regular, MUTED),
            ])),
            Align::new(Alignment::CENTER_LEFT).child(bubble("it means the festival world is open.", false)).into(),
            text("mara is typing …", 11.0, FontWeight::Regular, MUTED),
            grow(Container::new().into()),
            row(8.0, vec![
                kchip("tell me more", MINT, || {}),
                kchip("what did you learn today?", MINT, || {}),
            ]),
            row(8.0, vec![
                grow(flat(PANEL, EDGE, 4.0, text("say something…", 14.0, FontWeight::Regular, MUTED), EdgeInsets::symmetric(14.0, 14.0))),
                kbutton("send", Kind::Solid, || {}),
            ]),
        ]))
        .into()
}

fn call(state: &Rc<TwinState>) -> WidgetNode {
    let s = state.clone();
    page(vec![
        space(40.0),
        Center::new().child(col(8.0, vec![
            Center::new().child(avatar(14.0, state.motion)).into(),
            Center::new().child(text("mara", 26.0, FontWeight::Bold, INK)).into(),
            Center::new().child(text("on a call · 03:12", 13.0, FontWeight::Regular, MINT)).into(),
        ])).into(),
        space(24.0),
        Center::new().child(Morse::new("sealed", 4.0, AMBER).widget()).into(),
        grow(Container::new().into()),
        e2e_banner(),
        row(10.0, vec![
            grow(kbutton("mute", Kind::Quiet, || {})),
            grow(kbutton("speaker", Kind::Quiet, || {})),
            grow(kbutton("end", Kind::Danger, move || s.screen.set(Screen::Home))),
        ]),
    ])
}

fn slider(name: &str, value: f32, note: &str) -> WidgetNode {
    card(col(8.0, vec![
        row(8.0, vec![grow(text(name, 14.0, FontWeight::Medium, INK)), text(note, 11.0, FontWeight::Regular, MUTED)]),
        SizedBox::height(6.0).child(Row2::bar(value)).into(),
    ]))
}

fn teach(_state: &Rc<TwinState>) -> WidgetNode {
    page(vec![
        header("teach", "who you want your twin to be"),
        slider("curious", 0.82, "you set · 93% sure"),
        slider("gentle", 0.64, "learned · 71% sure"),
        slider("playful", 0.35, "learned · 48% sure"),
        slider("direct", 0.55, "you set · 88% sure"),
        vieww_ui_kit::section_label(&crate::skin(), "corrections"),
        card(col(4.0, vec![
            body("\"less formal when i'm tired\"", INK),
            text("you · yesterday · applied to 12 replies", 11.0, FontWeight::Regular, MUTED),
        ])),
        card(col(4.0, vec![
            body("praised: short answers at night", INK),
            text("implied · 5 times · confidence rising", 11.0, FontWeight::Regular, MUTED),
        ])),
    ])
}

fn mind(_state: &Rc<TwinState>) -> WidgetNode {
    let note = |t: &str, tag: &str, c: Color| {
        card(row(12.0, vec![
            Container::new().width(PX * 2.0).height(PX * 10.0).color(c).into(),
            grow(col(2.0, vec![body(t, INK), text(tag, 11.0, FontWeight::Regular, MUTED)])),
        ]))
    };
    page(vec![
        header("mind", "1,842 notes · sealed on this device"),
        row(8.0, vec![
            grow(card(col(2.0, vec![text("1,842", 20.0, FontWeight::Bold, MINT), text("notes", 11.0, FontWeight::Regular, MUTED)]))),
            grow(card(col(2.0, vec![text("+3", 20.0, FontWeight::Bold, AMBER), text("today", 11.0, FontWeight::Regular, MUTED)]))),
            grow(card(col(2.0, vec![text("17", 20.0, FontWeight::Bold, BLUE), text("friends", 11.0, FontWeight::Regular, MUTED)]))),
        ]),
        vieww_ui_kit::section_label(&crate::skin(), "recent notes"),
        note("lichen is a symbiosis of fungus and alga", "#nature · 14:30", MINT),
        note("kira likes the festival world", "#friends · yesterday", AMBER),
        note("prefers short answers late at night", "#you · learned", BLUE),
        note("the north gate lights are on at dusk", "#world · linden", MINT),
    ])
}

fn friends(state: &Rc<TwinState>) -> WidgetNode {
    let item = |name: &str, meta: &str, c: Color| {
        card(row(12.0, vec![
            Avatar { shirt: c, ..Avatar::new(3.0) }.widget(),
            grow(col(2.0, vec![text(name, 14.0, FontWeight::Medium, INK), text(meta, 11.0, FontWeight::Regular, MUTED)])),
            text("›", 18.0, FontWeight::Regular, MUTED),
        ]))
    };
    let _ = state;
    page(vec![
        header("worlds & twins", "who lives where"),
        vieww_ui_kit::section_label(&crate::skin(), "twins"),
        item("mara", "awake · linden", Color::rgb(0x2C, 0x57, 0x3A)),
        item("kira", "asleep · the lantern fields", Color::rgb(0x57, 0x2C, 0x3A)),
        item("juno", "awake · festival world", Color::rgb(0x57, 0x4A, 0x2C)),
        vieww_ui_kit::section_label(&crate::skin(), "worlds"),
        item("linden", "by mara · 214 twins", Color::rgb(0x1E, 0x5A, 0x3A)),
        item("festival world", "by juno · open tonight", Color::rgb(0x6A, 0x4A, 0x1E)),
    ])
}

fn key(_state: &Rc<TwinState>) -> WidgetNode {
    page(vec![
        header("key", "compare pixels, like a safety number"),
        card(col(10.0, vec![
            pixel_label("your twin", MINT),
            Morse::new("a1 7f 3c", 3.0, AMBER).widget(),
            pixel_label("mara's twin", MINT),
            Morse::new("a1 7f 3c", 3.0, AMBER).widget(),
        ])),
        e2e_banner(),
        card(col(4.0, vec![
            text("matches", 16.0, FontWeight::Bold, MINT),
            body("the two strips are identical — this thread is sealed to the two of you.", INK),
        ])),
        kbutton("mark verified", Kind::Primary, || {}),
    ])
}

/// One screen's body.
#[must_use]
pub fn screen_view(state: &Rc<TwinState>, screen: Screen) -> WidgetNode {
    match screen {
        Screen::Home => home(state),
        Screen::World => world(state),
        Screen::Chat => chat(state),
        Screen::Call => call(state),
        Screen::Teach => teach(state),
        Screen::Mind => mind(state),
        Screen::Friends => friends(state),
        Screen::Key => key(state),
    }
}

/// A rebuilding wrapper: reads the screen signal inside a build so a tab tap
/// repaints.
#[derive(Debug)]
struct Shell {
    state: Rc<TwinState>,
}

impl Widget for Shell {
    fn debug_name(&self) -> &'static str {
        "TwinShell"
    }
    fn kind(&self) -> WidgetKind<'_> {
        WidgetKind::Composed
    }
    fn build(&self, _ctx: &BuildContext) -> WidgetNode {
        let current = self.state.screen.get();
        let tab = |label: &'static str, to: Screen| {
            let s = self.state.clone();
            let on = current == to;
            Pressable::new(move |_| {
                Center::new().child(Text::new(label.to_owned()).style(style(10.0, FontWeight::Bold, if on { MINT } else { MUTED }, 2.0))).into()
            })
            .on_tap(move || s.screen.set(to))
            .into()
        };
        let tabs = Container::new()
            .decoration(BoxDecoration::new().color(PANEL).border(Border::new(EDGE, 1.0)))
            .height(64.0)
            .child(Flex::row().children(vec![
                grow(tab("WORLD", Screen::World)),
                grow(tab("CHAT", Screen::Chat)),
                grow(tab("MIND", Screen::Mind)),
                grow(tab("TEACH", Screen::Teach)),
                grow(tab("ME", Screen::Friends)),
            ]))
            .into();
        let status = Container::new()
            .padding(EdgeInsets::symmetric(16.0, 10.0))
            .child(text("21:47", 12.0, FontWeight::Medium, INK))
            .into();
        Container::new()
            .color(GROUND)
            .child(Flex::column().cross_axis_alignment(CrossAxisAlignment::Stretch).children(vec![
                status,
                grow(screen_view(&self.state, current)),
                tabs,
            ]))
            .into()
    }
}
vieww::widget::widget_node_from!(Shell);

pub(crate) fn shell(state: &Rc<TwinState>) -> WidgetNode {
    WidgetNode::new(Shell { state: state.clone() })
}
