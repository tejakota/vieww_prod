//! Soma — a health explorer.
//!
//! One library, four entry points: [`run`] for desktop, `android_main` for
//! an Android activity, `vieww_ios_main` for `ios/App/main.m`, and
//! [`start`] for the browser (`web/index.html`). Everything they share is
//! in [`app`] and [`mount`], so the window cannot end up configured
//! differently depending on where it was started from — and neither can
//! the canvas.
//!
//! # What the app is
//!
//! A dark imaging suite with a 3D muscle body standing in it, blurred at
//! rest. A search box floats at the bottom. Typing runs the local search
//! engine (a regex engine, a grep and a ranker, in `search.rs`) over
//! 11,043 Cleveland Clinic health topics embedded in the binary (`db.rs`),
//! and as the leading hit changes the camera glides to that organ and the
//! blur clears. Choosing a result opens the bottom third of the screen —
//! the topic's overview, related conditions, and where it came from —
//! while the body holds the top two thirds, x-rayed down to the organ.
//! Tapping the body searches back: the tap ray-picks a muscle group or an
//! organ and brings up its page.
//!
//! Everything is offline. The APK carries no network permission at all.

use std::cell::RefCell;
use std::rc::Rc;

use vieww::foundation::{Color, Gradient, Size, TextEditingValue};
use vieww::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use vieww_platform_winit::App;
use vieww::{ScrollController};
use vieww_3d::{RenderStats, Scene};

pub mod anatomy;
pub mod db;
pub mod organs;
pub mod search;
pub mod screens;

use anatomy::{Parts, Placement, Pose, Rig};
use db::Db;
use search::Hit;

/// The window's size at startup. Phone-shaped, because the screen in
/// `screens/home.rs` is laid out for one — change both together.
///
/// Ignored on a phone, where the window is whatever the device is.
pub const SURFACE: Size = Size {
    width: 402.0,
    height: 874.0,
};

// ---------------------------------------------------------------------------
// Palette — one deep ink field, one clinical teal, one warm coral.
// ---------------------------------------------------------------------------

/// The page's deepest ground.
pub const GROUND: Color = Color::rgb(8, 11, 18);
/// A raised card.
pub const CARD: Color = Color::rgb(19, 26, 40);
/// A card that floats above the body, slightly translucent in feel.
pub const CARD_GLASS: Color = Color::rgba(16, 22, 35, 242);
/// Primary ink.
pub const INK: Color = Color::rgb(237, 241, 247);
/// Secondary text.
pub const MUTED: Color = Color::rgb(147, 160, 180);
/// Hairlines.
pub const EDGE: Color = Color::rgba(255, 255, 255, 20);
/// The clinical teal the app points with.
pub const ACCENT: Color = Color::rgb(89, 235, 209);
/// The warm coral for conditions and symptoms.
pub const CORAL: Color = Color::rgb(240, 130, 116);

/// The ground wash behind everything, dithered so an eight-bit screen
/// cannot band it.
pub fn ground_gradient() -> Gradient {
    Gradient::vertical().with_dither().with_stops(&[
        (0.0, Color::rgb(16, 23, 37)),
        (0.52, Color::rgb(9, 13, 21)),
        (1.0, GROUND),
    ])
}

/// The app's theme.
#[must_use]
pub fn theme() -> ThemeData {
    let dark = ThemeData::dark();
    ThemeData {
        colors: ColorScheme {
            primary: ACCENT,
            on_primary: Color::rgb(6, 20, 18),
            surface: GROUND,
            on_surface: INK,
            surface_variant: CARD,
            on_surface_variant: MUTED,
            outline: EDGE,
            ..dark.colors
        },
        text: Typography::scale(INK),
        ..dark
    }
}

/// The typeface: Geist, embedded, so every screen on every platform has
/// the same voice.
#[must_use]
pub fn fonts() -> vieww::text::FontStore {
    const REGULAR: &[u8] = include_bytes!("../assets/fonts/vw-Geist-Regular.ttf");
    const MEDIUM: &[u8] = include_bytes!("../assets/fonts/vw-Geist-Medium.ttf");
    const BOLD: &[u8] = include_bytes!("../assets/fonts/vw-Geist-Bold.ttf");
    vieww::text::FontStore::with_application_faces(
        [REGULAR.to_vec(), MEDIUM.to_vec(), BOLD.to_vec()],
        Some("Geist"),
        Some("Geist"),
    )
}

// ---------------------------------------------------------------------------
// The state
// ---------------------------------------------------------------------------

/// Everything the screen needs, held together so the widget tree can reach
/// all of it through one `Rc`.
pub struct AppState {
    /// The corpus.
    pub db: Rc<Db>,
    /// The live camera pose, written by the rig every animated frame.
    pub pose: Signal<Pose>,
    /// Which organ region is targeted, if any (index into `db.organs`).
    pub selected: Signal<Option<usize>>,
    /// Which topic is open in the panel, if any (index into `db.topics`).
    pub topic: Signal<Option<usize>>,
    /// The search field.
    pub query: Signal<TextEditingValue>,
    /// The ranked results for the current query (empty when idle).
    pub results: Signal<Rc<Vec<Hit>>>,
    /// Whether the regex fell back to literal matching.
    pub regex_fallback: Signal<bool>,
    /// The panel's scroll.
    pub panel_scroll: ScrollController,
    /// 3D scene handles.
    pub scene: Rc<RefCell<Scene>>,
    pub parts: Parts,
    pub placements: Vec<Placement>,
    /// The camera rig; also a ticker.
    pub rig: Rc<RefCell<Rig>>,
    /// The last frame's 3D statistics, for the caption.
    pub stats: Rc<RefCell<RenderStats>>,
    /// The viewport's logical size, for projecting organ callouts.
    pub viewport: Signal<(f32, f32)>,
}

/// Build the whole app state against a runtime: the database, the scene,
/// the rig, the signals.
#[must_use]
pub fn build_state(runtime: &vieww::element::Runtime) -> Rc<AppState> {
    let database = db::shared();
    let pose = runtime.signal(Pose {
        camera: vieww_3d::Camera::perspective(
            vieww_3d::Vec3::new(0.0, 1.35, 3.9),
            vieww_3d::Vec3::new(0.0, 1.02, 0.0),
            0.62,
        ),
        blur: 1.0,
        xray: 0.0,
        panel: 0.0,
        pill: 0.0,
    });
    let selected = runtime.signal(None::<usize>);
    let (scene, parts, placements, rig) = Rig::new(&database, pose.clone(), selected.clone());
    Rc::new(AppState {
        db: database,
        pose,
        selected,
        topic: runtime.signal(None),
        query: runtime.signal(TextEditingValue::new(String::new())),
        results: runtime.signal(Rc::new(Vec::new())),
        regex_fallback: runtime.signal(false),
        panel_scroll: ScrollController::new(runtime, ScrollPhysics::android()),
        scene,
        parts,
        placements,
        rig,
        stats: Rc::new(RefCell::new(RenderStats::default())),
        viewport: runtime.signal((SURFACE.width, SURFACE.height)),
    })
}

/// Where the floating search should sit for a target at `height` (world
/// y): high targets leave it low, low targets lift it high, and the band
/// between glides. This is the "answer and box never fight for pixels"
/// rule the pose's `pill` spring carries.
fn pill_for_height(height: f32) -> f32 {
    // Head at ~1.6 answers low; knees at ~0.4 answer high.
    (1.0 - (height - 0.95) / 0.45).clamp(0.0, 1.0)
}

/// Run a query and publish the results, retargeting the camera at the
/// leading hit so the body follows the search live, and floating the
/// search to the side of the body the answer is not on.
pub fn run_search(state: &AppState) {
    let text = state.query.get().text.clone();
    let (hits, fell_back) = search::search(&state.db, &text, 24);
    state.regex_fallback.set(fell_back);
    state.results.set(Rc::new(hits));
    if text.trim().is_empty() {
        // Back to the resting body — unless a topic is open, in which case
        // the open topic keeps the camera.
        if state.topic.get().is_none() {
            state.selected.set(None);
            state.rig.borrow_mut().rest();
            state.rig.borrow_mut().set_panel(false);
            state.rig.borrow_mut().set_pill(0.0);
        }
        return;
    }
    // The camera follows the leading hit. How far it commits depends on how
    // well the query is matched: a strong title hit pulls a close, crisp
    // preview; a weak description-only graze keeps its distance.
    // Typing is a new question: whatever topic was open gives way to the
    // results, and the panel rises to hold them.
    state.topic.set(None);
    state.rig.borrow_mut().set_panel(true);
    let results = state.results.get();
    if let Some(top) = results.first() {
        if let Some(topic) = state.db.topics.get(top.topic) {
            if let Some(organ) = topic.organ {
                let organ = &state.db.organs[organ];
                let anchor =
                    vieww_3d::Vec3::new(organ.anchor[0], organ.anchor[1], organ.anchor[2]);
                let strength = (top.score / 3.4).clamp(0.35, 1.0);
                // The leading hit's organ is already the target: it takes
                // the accent and the marker while the person is still
                // typing, so the body answers the query live.
                state.selected.set(Some(organ_index(&state, organ.key)));
                state.rig.borrow_mut().preview(anchor, organ.zoom, strength);
                state.rig.borrow_mut().set_pill(pill_for_height(organ.anchor[1]));
            } else {
                // No organ to fly to; hold the whole body but clear up.
                state.selected.set(None);
                state
                    .rig
                    .borrow_mut()
                    .preview(vieww_3d::Vec3::new(0.0, 1.02, 0.0), 1.45, 0.45);
                state.rig.borrow_mut().set_pill(0.35);
            }
        }
    }
}

/// An organ's index in the table, by key.
fn organ_index(state: &AppState, key: &str) -> usize {
    state
        .db
        .organs
        .iter()
        .position(|o| o.key == key)
        .unwrap_or(0)
}

/// Open a topic: fly to its organ, x-ray the shell, raise the panel, and
/// float the search clear of the organ the answer lives at.
pub fn select_topic(state: &AppState, index: usize) {
    let Some(topic) = state.db.topics.get(index) else {
        return;
    };
    state.topic.set(Some(index));
    state.panel_scroll.jump_to(0.0);
    match topic.organ {
        Some(organ) => {
            let o = &state.db.organs[organ];
            let anchor = vieww_3d::Vec3::new(o.anchor[0], o.anchor[1], o.anchor[2]);
            state.selected.set(Some(organ));
            state.rig.borrow_mut().focus(anchor, o.zoom);
            state.rig.borrow_mut().set_pill(pill_for_height(o.anchor[1]));
        }
        None => {
            state.selected.set(None);
            state
                .rig
                .borrow_mut()
                .focus(vieww_3d::Vec3::new(0.0, 1.02, 0.0), 1.45);
            state.rig.borrow_mut().set_pill(0.35);
        }
    }
    state.rig.borrow_mut().set_panel(true);
}

/// Everything back to the resting body.
pub fn clear(state: &AppState) {
    state.topic.set(None);
    state.selected.set(None);
    state.query.set(TextEditingValue::new(String::new()));
    state.results.set(Rc::new(Vec::new()));
    state.regex_fallback.set(false);
    state.rig.borrow_mut().set_panel(false);
    state.rig.borrow_mut().set_pill(0.0);
    state.rig.borrow_mut().rest();
}

/// A tap on the body: ray-pick through the live camera, then bring up the
/// organ or region that was hit.
pub fn pick_body(state: &AppState, x: f32, y: f32) {
    use vieww_3d::raycast::{raycast, Ray};

    let pose = state.pose.get();
    let (vw, vh) = state.viewport.get();
    // The 3D buffer runs at RENDER_SCALE of the logical size; the tap is in
    // logical coordinates, so scale it into buffer pixels.
    let bw = (vw * anatomy::RENDER_SCALE).min(anatomy::MAX_INTERNAL_WIDTH as f32).max(2.0);
    let bh = bw * vh / vw.max(1.0);
    let sx = x * bw / vw.max(1.0);
    let sy = y * bh / vh.max(1.0);
    let ray = Ray::from_screen(&pose.camera, bw, bh, sx, sy);
    let hit = raycast(&mut state.scene.borrow_mut(), &ray)
        .into_iter()
        .next();
    let Some(hit) = hit else {
        return;
    };
    let name = state.scene.borrow().node(hit.node).name.clone();
    // The body is one picture-grown shell: which pixel was tapped names the
    // region, in the picture's own coordinates. Organ meshes inside the
    // shell keep their own names.
    let key = if name == "body" {
        anatomy::region_at(hit.uv[0], hit.uv[1]).to_owned()
    } else {
        match name.as_str() {
            "head" => "brain".to_owned(),
            "neck" => "thyroid".to_owned(),
            "chest" => "lungs".to_owned(),
            "abdomen" | "pelvis" => "intestines".to_owned(),
            other => other.to_owned(),
        }
    };
    let Some(organ) = state.db.organs.iter().position(|o| o.key == key) else {
        return;
    };
    // The organ's own anatomy page, when the corpus has one.
    let page = state
        .db
        .topics
        .iter()
        .position(|t| t.kind == 'B' && t.organ == Some(organ));
    match page {
        Some(index) => select_topic(state, index),
        None => {
            // No article: still fly there and show what the region is.
            let o = &state.db.organs[organ];
            state.topic.set(None);
            state.selected.set(Some(organ));
            let anchor = vieww_3d::Vec3::new(o.anchor[0], o.anchor[1], o.anchor[2]);
            state.rig.borrow_mut().focus(anchor, o.zoom);
            state.rig.borrow_mut().set_panel(true);
        }
    }
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// The application, configured but not started.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn app() -> App {
    App::new().title("Soma").size(SURFACE).theme(theme())
}

/// The root widget: the theme wrapped around the home screen.
#[must_use]
pub fn root(state: &Rc<AppState>) -> WidgetNode {
    Theme::new(theme()).child(screens::home::root(state)).into()
}

/// Mount the app: build the state, attach the tickers, set the root.
pub fn mount(driver: &mut vieww::FrameDriver) -> Rc<AppState> {
    let runtime = driver.elements().runtime().clone();
    let state = build_state(&runtime);
    driver.tickers().add(&state.rig);
    state.panel_scroll.attach(driver.tickers());
    driver.set_root(root(&state));
    state
}

/// Start on a desktop, where the process owns its own event loop.
#[cfg(not(target_arch = "wasm32"))]
pub fn run() {
    match app().run(|driver| {
        mount(driver);
    }) {
        Ok(report) => println!("{report}"),
        Err(error) => eprintln!("soma failed to start: {error}"),
    }
}

/// The browser's entry point, on the `<canvas id="vieww-canvas">` that
/// `web/index.html` carries.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() -> Result<(), wasm_bindgen::JsValue> {
    use vieww_platform_web::{WebApp, WebSurface};

    let surface = WebSurface::by_id("vieww-canvas")
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))?;
    WebApp::new(surface)
        .background(theme().colors.surface)
        .mount_with(|driver| {
            mount(driver);
        })
        .map(std::mem::forget)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))
}

/// Android's entry point: the activity loads the library and calls this.
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(android: vieww_platform_winit::AndroidApp) {
    // The platform wants a mount that returns nothing; the state handle is
    // ours to keep or drop, and on a phone nothing else needs it.
    if let Err(error) = app().run_android(android, |driver| {
        mount(driver);
    }) {
        log::error!("soma failed to start: {error}");
    }
}

/// iOS's entry point, called from `ios/App/main.m`.
#[cfg(target_os = "ios")]
#[no_mangle]
pub extern "C" fn vieww_ios_main() {
    run();
}
