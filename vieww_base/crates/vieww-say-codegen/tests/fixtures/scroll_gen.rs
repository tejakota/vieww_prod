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
    // say: scroll.say:4  keep names
    names: Vec<String>,
    // say: scroll.say:7  a scroll view's offset and how far it goes
    ui_scroll_7: f32,
    ui_scroll_7_max: f32,
    dirty: bool,
}

#[allow(clippy::derivable_impls)] // the defaults come from the keep lines
impl Default for SayState {
    fn default() -> Self {
        Self {
            names: vec!["Ada".to_owned(), "Grace".to_owned(), "Hedy".to_owned(), "Katherine".to_owned(), "Margaret".to_owned(), "Radia".to_owned(), "Barbara".to_owned(), "Frances".to_owned(), "Joan".to_owned(), "Karen".to_owned(), "Mary".to_owned(), "Evelyn".to_owned()],
            ui_scroll_7: 0.0,
            ui_scroll_7_max: 0.0,
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

    /// The state as a string, so it can survive the recompile a Render
    /// performs. Fields are joined with U+001E, list items with U+001F.
    fn snapshot(&self) -> Option<String> {
        Some(format!(
            "names={}",
                say::join(&self.names),
            )
        )
    }

    /// The kept values, for the studio's inspector.
    fn debug_fields(&self) -> Vec<(String, String)> {
        vec![
            ("names".to_owned(), format!("{:?}", self.names)),
        ]
    }

    /// Put back what `snapshot` saved. The input is untrusted — it may be
    /// from a version of this file that kept different things — so this
    /// reads into locals and applies only when all of it made sense.
    fn restore(&mut self, saved: &str) -> bool {
        let mut names = self.names.clone();
        for field in saved.split('\u{1e}') {
            let Some((key, value)) = field.split_once('=') else {
                return false;
            };
            match key {
                "names" => names = say::split_list(value).collect(),
                _ => return false,
            }
        }
        self.names = names;
        true
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
        let page = screen_names(ctx, &handle);
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

// -- the names screen ---------------------------------------------------------------

fn screen_names(ctx: &BuildContext, handle: &Handle) -> WidgetNode {
    // say: scroll.say:6  screen "Names"
    let names = ctx.state::<SayState, _>(|s| s.names.clone()).unwrap_or(vec!["Ada".to_owned(), "Grace".to_owned(), "Hedy".to_owned(), "Katherine".to_owned(), "Margaret".to_owned(), "Radia".to_owned(), "Barbara".to_owned(), "Frances".to_owned(), "Joan".to_owned(), "Karen".to_owned(), "Mary".to_owned(), "Evelyn".to_owned()]);
    let ui_scroll_7 = ctx.state::<SayState, _>(|s| s.ui_scroll_7).unwrap_or(0.0);
    let theme = ThemeData::of(ctx);
    let mut children: Vec<WidgetNode> = Vec::new();
    // say: scroll.say:7  a scroll view
    children.push(Scrollable::vertical(ui_scroll_7)
.on_drag(Rc::new(say::on(handle, |s: &mut SayState, d: vieww::foundation::DragDetails| { s.ui_scroll_7 = (s.ui_scroll_7 - d.delta.dy).clamp(0.0, s.ui_scroll_7_max); })))
.on_extents(Rc::new(say::on(handle, |s: &mut SayState, e: ScrollExtents| { s.ui_scroll_7_max = e.max_offset(); s.ui_scroll_7 = s.ui_scroll_7.min(s.ui_scroll_7_max); })))
.child(Flex::column().main_axis_size(MainAxisSize::Min).cross_axis_alignment(CrossAxisAlignment::Stretch).children({
            let mut children: Vec<WidgetNode> = Vec::new();
            // say: scroll.say:8  for each name in names
            {
                let rows_1: Vec<WidgetNode> = names.iter().enumerate().flat_map(|(_i, name)| {
                    let mut children: Vec<WidgetNode> = Vec::new();
                    // say: scroll.say:9  a card
                    children.push(Container::new().padding(EdgeInsets::all(24.0)).child({
                            let mut children: Vec<WidgetNode> = Vec::new();
                            // say: scroll.say:10  a text
                            children.push(Text::new(format!("{}", name)).style(theme.text.body).into());
                            if children.len() == 1 {
                                children.remove(0)
                            } else {
                                Flex::column().main_axis_size(MainAxisSize::Min).children(children).into()
                            }
                        }
).into());
                    children
                }).collect();
                children.extend(rows_1);
            }
            children
        }
)).into());
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

