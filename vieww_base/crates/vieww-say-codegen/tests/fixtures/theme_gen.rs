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
    dirty: bool,
}

#[allow(clippy::derivable_impls)] // the defaults come from the keep lines
impl Default for SayState {
    fn default() -> Self {
        Self {
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

// -- the theme ----------------------------------------------------------------

/// The `the theme:` block, over whatever theme the host supplies.
#[allow(unreachable_pub)] // reachable when the file is included as a module
pub fn say_theme(base: &ThemeData) -> ThemeData {
    let mut theme = *base;
    // say: theme.say:6  primary
    theme.colors.primary = Color::hex(0xFF6750A4);
    // say: theme.say:7  on primary
    theme.colors.on_primary = Color::hex(0xFFFFFFFF);
    // say: theme.say:8  surface
    theme.colors.surface = Color::hex(0xFFFFFBFE);
    theme
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
        let page = screen_home(ctx, &handle);
        let mut layers: Vec<WidgetNode> = vec![SafeArea::new().child(page).into()];
        let built: WidgetNode = if layers.len() == 1 {
            layers.remove(0)
        } else {
            Stack::new().children(layers).into()
        };
        // The theme block, for every widget that reads the theme itself.
        Theme::new(say_theme(&ThemeData::of(ctx))).child(built).into()
    }
}

/// The entry point the preview looks for.
#[allow(unreachable_pub)] // pub is the contract when the file compiles standalone
pub fn screen() -> impl Widget {
    SayApp
}

// -- the home screen ---------------------------------------------------------------

fn screen_home(ctx: &BuildContext, handle: &Handle) -> WidgetNode {
    // say: theme.say:10  screen "Home"
    let theme = say_theme(&ThemeData::of(ctx));
    let _ = handle; // this screen has no events
    let mut children: Vec<WidgetNode> = Vec::new();
    // say: theme.say:11  a column
    children.push(Flex::column().spacing(12.0).children({
            let mut children: Vec<WidgetNode> = Vec::new();
            // say: theme.say:12  a card
            children.push(Container::new().color(theme.colors.primary).padding(EdgeInsets::all(16.0)).child({
                    let mut children: Vec<WidgetNode> = Vec::new();
                    // say: theme.say:13  a text
                    children.push(Text::new("Themed").style(theme.text.body).color(Color::hex(0xFFFFFFFF)).into());
                    if children.len() == 1 {
                        children.remove(0)
                    } else {
                        Flex::column().main_axis_size(MainAxisSize::Min).children(children).into()
                    }
                }
).into());
            // say: theme.say:14  a button
            children.push(Button::new("Continue").into());
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

