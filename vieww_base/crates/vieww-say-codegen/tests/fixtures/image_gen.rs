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

    /// `an image "…"`: read and decoded once, then kept. A file that is
    /// missing or not a picture shows what went wrong in its place rather
    /// than failing the screen.
    pub(crate) fn image(
        path: &str,
        label: Option<&str>,
        width: Option<f32>,
        height: Option<f32>,
    ) -> WidgetNode {
        thread_local! {
            static CACHE: vieww::ImageCache = vieww::ImageCache::new();
        }
        let bundle = vieww::DirectoryBundle::at(super::SAY_ASSET_ROOT);
        match CACHE.with(|cache| cache.load(&bundle, path)) {
            Ok(data) => {
                let mut image = Image::new(data);
                if let Some(label) = label {
                    image = image.label(label);
                }
                if let Some(width) = width {
                    image = image.width(width);
                }
                if let Some(height) = height {
                    image = image.height(height);
                }
                image.into()
            }
            Err(error) => vieww::widget::ErrorPlaceholder::new("Image", format!("{path}: {error}")).into(),
        }
    }
}

/// Where `an image` paths are read from. Empty means the working folder;
/// the studio sets it to the folder the `.say` file is in.
const SAY_ASSET_ROOT: &str = "";

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
        let page = screen_gallery(ctx, &handle);
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

// -- the gallery screen ---------------------------------------------------------------

fn screen_gallery(ctx: &BuildContext, handle: &Handle) -> WidgetNode {
    // say: image.say:5  screen "Gallery"
    let theme = ThemeData::of(ctx);
    let _ = handle; // this screen has no events
    let mut children: Vec<WidgetNode> = Vec::new();
    // say: image.say:6  a column
    children.push(Flex::column().spacing(12.0).children({
            let mut children: Vec<WidgetNode> = Vec::new();
            // say: image.say:7  a heading
            children.push(Text::new("Gallery").style(theme.text.headline).into());
            // say: image.say:8  an image
            children.push(say::image("tests/fixtures/dot.png", Some("A red dot"), Some(64.0), Some(64.0)).into());
            // say: image.say:9  an image
            children.push(say::image("tests/fixtures/missing.png", None, None, None).into());
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

