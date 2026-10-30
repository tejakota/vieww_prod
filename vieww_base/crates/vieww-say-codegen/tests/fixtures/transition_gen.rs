// Generated from a `.say` file by Say — edit the `.say` file, not this.
// This pane is what it compiles to; every line below points back at the
// `.say` line that produced it with a `// say:` marker.
//
// Say: A Say app
//
// say-language: 1 · say-codegen 0.0.1 · the same file, byte for byte, on every Render

use std::cell::RefCell;
use std::rc::Rc;

use vieww::prelude::*;

/// The handle a handler reaches its screen's state through.
type Handle = Option<Rc<RefCell<dyn ElementState>>>;

// -- state ------------------------------------------------------------------

/// Everything the screens keep, one field per `keep` line, plus the
/// navigation stack. This is the state that survives a Render: it lives on
/// the screen's own element, the studio carries it across every
/// recompile, and a handler writes it through the element's handle.
#[derive(Debug)]
struct SayState {
    stack: Vec<SayScreens>,
    /// How far the screen transition has run, 0 to 1.
    nav_t: f32,
    nav_start: Option<std::time::Duration>,
    nav_running: bool,
    dirty: bool,
}

#[allow(clippy::derivable_impls)] // the defaults come from the keep lines
impl Default for SayState {
    fn default() -> Self {
        Self {
            stack: Vec::new(),
            nav_t: 1.0,
            nav_start: None,
            nav_running: false,
            dirty: false,
        }
    }
}

#[allow(dead_code)] // helpers a screen may not call, kept for uniformity
impl SayState {
    /// A handler finished writing; ask the tree for one rebuild. The tree
    /// takes this flag once a frame — see `take_pending` below.
    fn mark(&mut self) {
        self.dirty = true;
    }

    /// The screen on top of the stack; the home screen is the floor.
    fn current(&self) -> SayScreens {
        *self.stack.last().unwrap_or(&SayScreens::Home)
    }

    /// `open the screen "X"`
    fn open_screen(&mut self, screen: SayScreens) {
        self.stack.push(screen);
        self.start_transition();
        self.mark();
    }

    /// `go back` — closes a dialog first; the home screen is the floor.
    fn go_back(&mut self) {
        if !self.stack.is_empty() {
            self.stack.pop();
            self.start_transition();
        }
        self.mark();
    }

    /// The screen that just came up plays its transition from the start;
    /// the clock starts at the next frame, in `tick`.
    fn start_transition(&mut self) {
        self.nav_t = 0.0;
        self.nav_start = None;
        self.nav_running = true;
    }
}

impl ElementState for SayState {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn take_pending(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// The state as a string, so it can survive the recompile a Render
    /// performs. Fields are joined with U+001E, list items with U+001F.
    fn snapshot(&self) -> Option<String> {
        Some(format!(
            "stack={}",
                self.stack.iter().map(|s| s.tag()).collect::<Vec<_>>().join("|"),
            )
        )
    }

    /// The kept values, for the studio's inspector.
    fn debug_fields(&self) -> Vec<(String, String)> {
        vec![
            ("screen".to_owned(), self.stack.iter().map(|s| s.tag()).collect::<Vec<_>>().join(" → ")),
        ]
    }

    /// Put back what `snapshot` saved. The input is untrusted — it may be
    /// from a version of this file that kept different things — so this
    /// reads into locals and applies only when all of it made sense.
    fn restore(&mut self, saved: &str) -> bool {
        let mut stack = self.stack.clone();
        for field in saved.split('\u{1e}') {
            let Some((key, value)) = field.split_once('=') else {
                return false;
            };
            match key {
                "stack" => {
                    stack = Vec::new();
                    for tag in value.split('|') {
                        match SayScreens::from_tag(tag) {
                            Some(screen) => stack.push(screen),
                            None => return false,
                        }
                    }
                }
                _ => return false,
            }
        }
        self.stack = stack;
        true
    }

    /// A snackbar and a screen transition ask for frames until they are
    /// done. The handler that started them cannot know the time — the
    /// tree supplies it here, in the frame's animate phase.
    fn tick(&mut self, now: std::time::Duration) -> bool {
        let mut busy = false;
        if self.nav_running {
            let start = *self.nav_start.get_or_insert(now);
            let t = now.saturating_sub(start).as_secs_f32() / NAV_FOR.as_secs_f32();
            if t >= 1.0 {
                self.nav_t = 1.0;
                self.nav_start = None;
                self.nav_running = false;
            } else {
                self.nav_t = t;
            }
            self.dirty = true;
            busy = true;
        }
        busy
    }

    fn is_animating(&self) -> bool {
        self.nav_running
    }
}

/// How long a screen transition runs.
const NAV_FOR: std::time::Duration = std::time::Duration::from_millis(300);

/// The screens of this file, in the order they are declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SayScreens {
    Home,
    Details,
    Settings,
}

impl SayScreens {
    fn tag(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Details => "details",
            Self::Settings => "settings",
        }
    }

    /// An unreadable tag means the snapshot is not for this file any
    /// more — `restore` refuses rather than landing somewhere arbitrary.
    fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "home" => Some(Self::Home),
            "details" => Some(Self::Details),
            "settings" => Some(Self::Settings),
            _ => None,
        }
    }
}

// -- the say helpers ---------------------------------------------------------

/// The two moves that make Say work inside a previewed screen. A handler
/// cannot reach a runtime — there is none across the preview boundary —
/// so it reaches the screen's own state instead, mutates it, and raises
/// the flag the element tree polls once a frame.
#[allow(dead_code)] // helpers a small file may not call, kept for uniformity
mod say {
    use super::SayState;
    use std::cell::RefCell;
    use std::rc::Rc;
    use vieww::prelude::*;

    type Handle = Option<Rc<RefCell<dyn ElementState>>>;

    /// Run `f` against the screen's state from a tap handler.
    pub(crate) fn act<F: Fn(&mut SayState) + 'static>(handle: &Handle, f: F) -> impl Fn() + 'static {
        let handle = handle.clone();
        move || {
            let Some(state) = handle.as_ref() else { return };
            let mut borrowed = state.borrow_mut();
            if let Some(s) = borrowed.as_any_mut().downcast_mut::<SayState>() {
                f(s);
                s.dirty = true;
            }
        }
    }

    /// The same, for handlers that receive a value: a field's new text, a
    /// switch's new position.
    pub(crate) fn on<T: 'static, F: Fn(&mut SayState, T) + 'static>(
        handle: &Handle,
        f: F,
    ) -> impl Fn(T) + 'static {
        let handle = handle.clone();
        move |value| {
            let Some(state) = handle.as_ref() else { return };
            let mut borrowed = state.borrow_mut();
            if let Some(s) = borrowed.as_any_mut().downcast_mut::<SayState>() {
                f(s, value);
                s.dirty = true;
            }
        }
    }

    /// Escape one value for the snapshot string.
    pub(crate) fn esc(text: &str) -> String {
        text.replace(['\u{1e}', '\u{1f}', '|'], " ")
    }

    /// Join list values for the snapshot string.
    pub(crate) fn join(items: &[String]) -> String {
        items.iter().map(|i| esc(i)).collect::<Vec<_>>().join("\u{1f}")
    }

    /// Split list values back out.
    pub(crate) fn split_list(saved: &str) -> impl Iterator<Item = String> + '_ {
        saved.split('\u{1f}').map(str::to_owned)
    }
}

// -- the app -----------------------------------------------------------------

/// The root widget: it owns the state (see `create_state`), builds the
/// screen on top of the stack, and lays the dialog and the snackbar over
/// it when they are up.
#[derive(Debug)]
struct SayApp;

impl Widget for SayApp {
    fn debug_name(&self) -> &'static str {
        "SayApp"
    }

    fn kind(&self) -> WidgetKind<'_> {
        WidgetKind::Composed
    }

    fn create_state(&self) -> Option<Box<dyn ElementState>> {
        Some(Box::new(SayState::default()))
    }

    fn build(&self, ctx: &BuildContext) -> WidgetNode {
        let handle = ctx.state_handle();
        let current = ctx.state::<SayState, _>(|s| s.current()).unwrap_or(SayScreens::Home);
        let page: WidgetNode = match current {
            SayScreens::Home => screen_home(ctx, &handle),
            SayScreens::Details => screen_details(ctx, &handle),
            SayScreens::Settings => screen_settings(ctx, &handle),
        };
        let nav_t = ctx.state::<SayState, _>(|s| s.nav_t).unwrap_or(1.0);
        let eased = 1.0 - (1.0 - nav_t.clamp(0.0, 1.0)).powi(3);
        let page: WidgetNode = match current {
            SayScreens::Details => {
                let width = ctx.inherit_or(ViewMetrics::default()).size.width;
                Transformed::translate(Offset::new((1.0 - eased) * width, 0.0))
                    .child(page)
                    .into()
            }
            SayScreens::Settings => Opacity::new(eased).child(page).into(),
            _ => page,
        };
        let mut layers: Vec<WidgetNode> = vec![SafeArea::new().child(page).into()];
        if layers.len() == 1 {
            layers.remove(0)
        } else {
            Stack::new().children(layers).into()
        }
    }
}

/// The entry point the preview looks for.
#[allow(unreachable_pub)] // pub is the contract when the file compiles standalone
pub fn screen() -> impl Widget {
    SayApp
}

// -- the home screen ---------------------------------------------------------------

fn screen_home(ctx: &BuildContext, handle: &Handle) -> WidgetNode {
    // say: transition.say:4  screen "Home"
    let _ = ctx; // this screen reads nothing from its element
    let mut children: Vec<WidgetNode> = Vec::new();
    // say: transition.say:5  a column
    children.push(Flex::column().spacing(8.0).children({
            let mut children: Vec<WidgetNode> = Vec::new();
            // say: transition.say:6  a button
            children.push(Button::new("Details").on_pressed(say::act(handle, |s: &mut SayState| {
                s.open_screen(SayScreens::Details);
    })).into());
            // say: transition.say:8  a button
            children.push(Button::new("Settings").on_pressed(say::act(handle, |s: &mut SayState| {
                s.open_screen(SayScreens::Settings);
    })).into());
            children
        }
).into());
    // One root line returns itself; more wrap in a column, which is what
    // a stack of root lines almost always means.
    if children.len() == 1 {
        children.remove(0)
    } else {
        Flex::column()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children(children)
            .into()
    }
}

// -- the details screen ---------------------------------------------------------------

fn screen_details(ctx: &BuildContext, handle: &Handle) -> WidgetNode {
    // say: transition.say:11  screen "Details"
    // comes up with a slide transition — see `SayApp::build`
    let theme = ThemeData::of(ctx);
    let mut children: Vec<WidgetNode> = Vec::new();
    // say: transition.say:12  a column
    children.push(Flex::column().children({
            let mut children: Vec<WidgetNode> = Vec::new();
            // say: transition.say:13  a text
            children.push(Text::new("Details").style(theme.text.body).into());
            // say: transition.say:14  a button
            children.push(Button::new("Back").on_pressed(say::act(handle, |s: &mut SayState| {
                s.go_back();
    })).into());
            children
        }
).into());
    // One root line returns itself; more wrap in a column, which is what
    // a stack of root lines almost always means.
    if children.len() == 1 {
        children.remove(0)
    } else {
        Flex::column()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children(children)
            .into()
    }
}

// -- the settings screen ---------------------------------------------------------------

fn screen_settings(ctx: &BuildContext, handle: &Handle) -> WidgetNode {
    // say: transition.say:17  screen "Settings"
    // comes up with a fade transition — see `SayApp::build`
    let _ = ctx; // this screen reads nothing from its element
    let mut children: Vec<WidgetNode> = Vec::new();
    // say: transition.say:18  a button
    children.push(Button::new("Done").on_pressed(say::act(handle, |s: &mut SayState| {
                s.go_back();
    })).into());
    // One root line returns itself; more wrap in a column, which is what
    // a stack of root lines almost always means.
    if children.len() == 1 {
        children.remove(0)
    } else {
        Flex::column()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children(children)
            .into()
    }
}

