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
use vieww::foundation::TextEditingValue;

/// The handle a handler reaches its screen's state through.
type Handle = Option<Rc<RefCell<dyn ElementState>>>;

// -- state ------------------------------------------------------------------

/// Everything the screens keep, one field per `keep` line, plus the
/// navigation stack. This is the state that survives a Render: it lives on
/// the screen's own element, the studio carries it across every
/// recompile, and a handler writes it through the element's handle.
#[derive(Debug)]
struct SayState {
    // say: profile.say:3  keep posts
    posts: Vec<String>,
    // say: profile.say:4  keep likes
    likes: i64,
    // say: profile.say:5  keep draft
    draft: TextEditingValue,
    modal: Option<SayModal>,
    // say: profile.say:15  the selected tab
    ui_tab_15: usize,
    // say: profile.say:17  a scroll view's offset and how far it goes
    ui_scroll_17: f32,
    ui_scroll_17_max: f32,
    dirty: bool,
}

#[allow(clippy::derivable_impls)] // the defaults come from the keep lines
impl Default for SayState {
    fn default() -> Self {
        Self {
            posts: vec!["a".to_owned(), "b".to_owned()],
            likes: 0,
            draft: TextEditingValue::new(""),
            modal: None,
            ui_tab_15: 0,
            ui_scroll_17: 0.0,
            ui_scroll_17_max: 0.0,
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

    /// `close this dialog`
    fn close_dialog(&mut self) {
        self.modal = None;
        self.mark();
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
            "posts={}\u{1e}likes={}\u{1e}draft={}",
                say::join(&self.posts),
                self.likes,
                say::esc(&self.draft.text),
            )
        )
    }

    /// The kept values, for the studio's inspector.
    fn debug_fields(&self) -> Vec<(String, String)> {
        vec![
            ("posts".to_owned(), format!("{:?}", self.posts)),
            ("likes".to_owned(), self.likes.to_string()),
            ("draft".to_owned(), self.draft.text.clone()),
        ]
    }

    /// Put back what `snapshot` saved. The input is untrusted — it may be
    /// from a version of this file that kept different things — so this
    /// reads into locals and applies only when all of it made sense.
    fn restore(&mut self, saved: &str) -> bool {
        let mut posts = self.posts.clone();
        let mut likes = self.likes;
        let mut draft = self.draft.clone();
        for field in saved.split('\u{1e}') {
            let Some((key, value)) = field.split_once('=') else {
                return false;
            };
            match key {
                "posts" => posts = say::split_list(value).collect(),
                "likes" => match value.parse() { Ok(v) => likes = v, Err(_) => return false },
                "draft" => draft = TextEditingValue::new(value.to_owned()),
                _ => return false,
            }
        }
        self.posts = posts;
        self.likes = likes;
        self.draft = draft;
        true
    }
}

/// The value a dialog edits while it is open. Transient by design:
/// closing a dialog throws the draft away, which is what Cancel means
/// and what Save has already copied into state.
#[derive(Debug, Clone)]
struct SayModal {
    title: String,
    dialog_name: TextEditingValue,
}

impl SayModal {
    fn new(title: &str) -> Self {
        Self {
            title: title.to_owned(),
            dialog_name: TextEditingValue::new(String::new()),
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
        let page = screen_profile(ctx, &handle);
        let mut layers: Vec<WidgetNode> = vec![SafeArea::new().child(page).into()];
        if let Some(m) = ctx.state::<SayState, _>(|s| s.modal.clone()).flatten() {
            layers.push(vieww::widget::ModalBarrier::new().into());
            layers.push(Center::new().child(dialog(ctx, &handle, &m)).into());
        }
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

// -- the profile screen ---------------------------------------------------------------

fn screen_profile(ctx: &BuildContext, handle: &Handle) -> WidgetNode {
    // say: profile.say:6  screen "Profile"
    let posts = ctx.state::<SayState, _>(|s| s.posts.clone()).unwrap_or(vec!["a".to_owned(), "b".to_owned()]);
    let likes = ctx.state::<SayState, _>(|s| s.likes).unwrap_or(0);
    let ui_tab_15 = ctx.state::<SayState, _>(|s| s.ui_tab_15).unwrap_or(0);
    let ui_scroll_17 = ctx.state::<SayState, _>(|s| s.ui_scroll_17).unwrap_or(0.0);
    let theme = ThemeData::of(ctx);
    let mut children: Vec<WidgetNode> = Vec::new();
    // say: profile.say:7  a column
    children.push(Flex::column().children({
            let mut children: Vec<WidgetNode> = Vec::new();
            // say: profile.say:8  an icon
            children.push(Icon::new(vieww::widget::icons::check()).into());
            // say: profile.say:9  a fixed area
            children.push(SizedBox::from_size(Size::new(80.0, 40.0)).child({
                    let mut children: Vec<WidgetNode> = Vec::new();
                    if children.len() == 1 {
                        children.remove(0)
                    } else {
                        Flex::column().main_axis_size(MainAxisSize::Min).children(children).into()
                    }
                }
).into());
            // say: profile.say:10  a card
            children.push(Container::new().padding(EdgeInsets::all(16.0)).radius(8.0).child({
                    let mut children: Vec<WidgetNode> = Vec::new();
                    if children.len() == 1 {
                        children.remove(0)
                    } else {
                        Flex::column().main_axis_size(MainAxisSize::Min).children(children).into()
                    }
                }
).into());
            // say: profile.say:11  a text
            children.push(Text::new("hi").style(theme.text.body).into());
            // say: profile.say:12  a button
            children.push(Button::new("Ask").on_pressed(say::act(handle, |s: &mut SayState| {
                s.modal = Some(SayModal::new("Name"));
    })).into());
            // say: profile.say:15  a set of tabs
            children.push({
                let page: WidgetNode = match ui_tab_15 {
                    // say: profile.say:16  a tab
                    0 => {
                        let mut children: Vec<WidgetNode> = Vec::new();
                        // say: profile.say:17  a scroll view
                        children.push(Scrollable::vertical(ui_scroll_17)
.on_drag(Rc::new(say::on(handle, |s: &mut SayState, d: vieww::foundation::DragDetails| { s.ui_scroll_17 = (s.ui_scroll_17 - d.delta.dy).clamp(0.0, s.ui_scroll_17_max); })))
.on_extents(Rc::new(say::on(handle, |s: &mut SayState, e: ScrollExtents| { s.ui_scroll_17_max = e.max_offset(); s.ui_scroll_17 = s.ui_scroll_17.min(s.ui_scroll_17_max); })))
.child(Flex::column().main_axis_size(MainAxisSize::Min).cross_axis_alignment(CrossAxisAlignment::Stretch).children({
                                let mut children: Vec<WidgetNode> = Vec::new();
                                // say: profile.say:18  for each post in posts
                                {
                                    let rows_1: Vec<WidgetNode> = posts.iter().enumerate().flat_map(|(_i, post)| {
                                        let mut children: Vec<WidgetNode> = Vec::new();
                                        // say: profile.say:19  a card
                                        children.push(Container::new().padding(EdgeInsets::all(16.0)).child({
                                                let mut children: Vec<WidgetNode> = Vec::new();
                                                // say: profile.say:20  a text
                                                children.push(Text::new(format!("{}", post)).style(theme.text.body).into());
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
                        if children.len() == 1 {
                            children.remove(0)
                        } else {
                            Flex::column().main_axis_size(MainAxisSize::Min).children(children).into()
                        }
                    }
                    ,
                    // say: profile.say:21  a tab
                    1 => {
                        let mut children: Vec<WidgetNode> = Vec::new();
                        // say: profile.say:22  a text
                        children.push(Text::new(format!("{} likes", likes)).style(theme.text.body).into());
                        if children.len() == 1 {
                            children.remove(0)
                        } else {
                            Flex::column().main_axis_size(MainAxisSize::Min).children(children).into()
                        }
                    }
                    ,
                    _ => SizedBox::shrink().into(),
                };
                Flex::column().main_axis_size(MainAxisSize::Min).cross_axis_alignment(CrossAxisAlignment::Stretch).children(vec![
                    TabBar::new(["Posts", "Likes"], ui_tab_15).on_selected(Rc::new(say::on(handle, |s: &mut SayState, i: usize| { s.ui_tab_15 = i; }))).into(),
                    page,
                ])
            }.into());
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

// -- dialogs -----------------------------------------------------------------

/// The dialog on top, by title. A dialog is state, not a route: it is
/// laid over the screen by `SayApp::build` and closed by writing None.
fn dialog(ctx: &BuildContext, handle: &Handle, m: &SayModal) -> WidgetNode {
    match m.title.as_str() {
        "Name" => dialog_name(ctx, handle, m),
        _ => SizedBox::shrink().into(),
    }
}

/// The "Name" dialog. Its bound fields live on `SayModal` for as long
/// as the dialog is open; a Save action copies them into state and a
/// Cancel closes without reading them.
fn dialog_name(ctx: &BuildContext, handle: &Handle, m: &SayModal) -> WidgetNode {
    // say: profile.say:13  open a dialog titled "Name"
    let dialog_name = m.dialog_name.clone();
    let _ = ctx; // this dialog reads nothing from its element
    let mut children: Vec<WidgetNode> = Vec::new();
    // say: profile.say:14  a text field
    children.push(TextField::new(dialog_name.clone()).on_changed(Rc::new(say::on(handle, |s: &mut SayState, v: TextEditingValue| if let Some(m) = s.modal.as_mut() { m.dialog_name = v; }))).into());
    let content: WidgetNode = if children.len() == 1 {
        children.remove(0)
    } else {
        Flex::column()
            .main_axis_size(MainAxisSize::Min)
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .spacing(12.0)
            .children(children)
            .into()
    };
    vieww::widget::Dialog::new()
        .title(m.title.clone())
        .content(content)
        .actions(Vec::new())
        .into()
}

