//! `3` — reference social application shell built with Vieww.
//!
//! This is deliberately backed by the same `ThreeApp<SocialBackend>` product
//! state that a networked client uses. The bundled backend is offline-first,
//! so the app is runnable without credentials or a server.
//!
//! Every tab is a real screen in this pass. The earlier shell carried four
//! stubs — a title, an accent rule, one sentence — where Explore, Create,
//! Activity and Profile now carry people cards with wired follow buttons, the
//! capture pipeline as a timeline, the backend's own notification feed, and a
//! profile header with its moments. Nothing on any of them is a picture of a
//! control: every button routes through the product state the real client
//! would use.

use std::{cell::RefCell, collections::HashSet, fmt, rc::Rc};
use three_app::{HomeFeed, Route, Tab, ThreeApp};
use three_capture::{CaptureRequest, DepthVideoSource};
use three_format::LazyCapture;
use three_runtime::{sample_capture, DynRecording};
use three_social::{
    demo_profile, MemorySocialBackend, Notification, NotificationKind, Post, PostId, Profile,
    SocialBackend, UserId,
};
use vieww::prelude::*;
use vieww::widget::ElementState;
use vieww_integration::ViewerScreen;
use vieww_platform_winit::App;

// ── the glyphs this shell draws ─────────────────────────────────────────────
//
// The framework's icon set is deliberately six shapes; the social shell needs
// a few more, drawn on the same 24×24 grid with the same filled-outline
// convention `vieww_widget::icons` documents.
mod icons {
    use vieww::foundation::{IconData, Offset, Path};

    /// The circle-to-Bézier constant for hand-drawn arcs.
    const KAPPA: f32 = 0.552_284_8;

    fn circle(path: &mut Path, cx: f32, cy: f32, r: f32, clockwise: bool) {
        let k = KAPPA * r;
        let s = if clockwise { 1.0 } else { -1.0 };
        path.move_to(Offset::new(cx + r, cy));
        path.cubic_to(
            Offset::new(cx + r, cy + s * k),
            Offset::new(cx + k, cy + s * r),
            Offset::new(cx, cy + s * r),
        );
        path.cubic_to(
            Offset::new(cx - k, cy + s * r),
            Offset::new(cx - r, cy + s * k),
            Offset::new(cx - r, cy),
        );
        path.cubic_to(
            Offset::new(cx - r, cy - s * k),
            Offset::new(cx - k, cy - s * r),
            Offset::new(cx, cy - s * r),
        );
        path.cubic_to(
            Offset::new(cx + k, cy - s * r),
            Offset::new(cx + r, cy - s * k),
            Offset::new(cx + r, cy),
        );
        path.close();
    }

    /// A heart, filled — the like. Four cubics: bottom point up the left
    /// side, around the left lobe, across the dip, around the right lobe, and
    /// home — each with two control points, as `cubic_to` takes them.
    pub fn heart() -> IconData {
        let mut path = Path::new();
        path.move_to(Offset::new(12.0, 20.4));
        path.cubic_to(
            Offset::new(4.2, 15.2),
            Offset::new(2.4, 9.6),
            Offset::new(5.6, 6.6),
        );
        path.cubic_to(
            Offset::new(8.2, 4.6),
            Offset::new(10.6, 5.8),
            Offset::new(12.0, 8.0),
        );
        path.cubic_to(
            Offset::new(13.4, 5.8),
            Offset::new(15.8, 4.6),
            Offset::new(18.4, 6.6),
        );
        path.cubic_to(
            Offset::new(21.6, 9.6),
            Offset::new(19.8, 15.2),
            Offset::new(12.0, 20.4),
        );
        path.close();
        IconData::square24(path)
    }

    /// A head and shoulders — a person.
    pub fn person() -> IconData {
        let mut path = Path::new();
        circle(&mut path, 12.0, 7.6, 3.9, true);
        // The shoulders: a wide shallow dome off the bottom of the grid.
        path.move_to(Offset::new(3.6, 20.4));
        path.cubic_to(
            Offset::new(3.6, 14.9),
            Offset::new(7.4, 12.4),
            Offset::new(12.0, 12.4),
        );
        path.cubic_to(
            Offset::new(16.6, 12.4),
            Offset::new(20.4, 14.9),
            Offset::new(20.4, 20.4),
        );
        path.close();
        IconData::square24(path)
    }

    /// A magnifier — search.
    pub fn search() -> IconData {
        let mut path = Path::new();
        circle(&mut path, 10.4, 10.4, 6.8, true);
        circle(&mut path, 10.4, 10.4, 4.9, false);
        let (x0, y0) = (15.0, 15.0);
        let (x1, y1) = (20.4, 20.4);
        let half = 1.2;
        path.move_to(Offset::new(x0 - half, y0 + half));
        path.line_to(Offset::new(x1 - half, y1 + half));
        path.line_to(Offset::new(x1 + half, y1 - half));
        path.line_to(Offset::new(x0 + half, y0 - half));
        path.close();
        IconData::square24(path)
    }

    /// A speech bubble — a comment. A rounded body plus a tail, both filled,
    /// overlapping: the nonzero rule takes their union.
    pub fn comment() -> IconData {
        let mut path = Path::new();
        path.extend(&Path::rounded_rect(
            vieww::foundation::Rect::new(2.6, 4.8, 21.4, 16.6),
            5.2,
        ));
        path.move_to(Offset::new(8.2, 15.8));
        path.line_to(Offset::new(11.8, 21.0));
        path.line_to(Offset::new(13.4, 15.8));
        path.close();
        IconData::square24(path)
    }

    /// A cube in three-quarter view — the `.3` format's own mark.
    pub fn cube() -> IconData {
        let mut path = Path::new();
        // The top face, the front face, and the right face — one silhouette
        // with two internal seams left as gaps, which is how an unfilled
        // wireframe has to spell itself when the painter only fills.
        let top = [(12.0, 3.0), (20.4, 7.6), (12.0, 12.2), (3.6, 7.6)];
        let front = [(3.6, 7.6), (12.0, 12.2), (12.0, 21.0), (3.6, 16.4)];
        let right = [(20.4, 7.6), (12.0, 12.2), (12.0, 21.0), (20.4, 16.4)];
        for face in [top, front, right] {
            path.move_to(Offset::new(face[0].0, face[0].1));
            for &(x, y) in &face[1..] {
                path.line_to(Offset::new(x, y));
            }
            path.close();
        }
        IconData::square24(path)
    }
}

// The platform's camera, when there is one — set once by the entry point
// that has the handle (Android's `android_main`, via `set_capture_source`),
// consumed by the first `SocialState::new()`.
//
// A thread-local rather than a parameter because the widget tree builds
// `SocialState` from a `create_state` with no arguments, and the platform
// handle exists before the tree does. One shot: the first state built
// takes it, and a rebuilt tree keeps its own source for the process's
// lifetime — which is also the camera's.
thread_local! {
    static CAPTURE_SOURCE: RefCell<Option<Box<dyn DepthVideoSource>>> =
        const { RefCell::new(None) };
}

/// Hand the platform's camera to the app. Call before the tree mounts.
pub fn set_capture_source(source: Box<dyn DepthVideoSource>) {
    CAPTURE_SOURCE.with(|slot| *slot.borrow_mut() = Some(source));
}

/// The seeded world: three people, one published moment, and a real
/// notification trail left behind by the other two acting on it — every like,
/// comment and follow in the Activity screen happened through the backend's
/// own API, as the people it says did them.
fn seeded_app() -> ThreeApp<MemorySocialBackend> {
    let mut teja = demo_profile("teja", "teja", "Teja");
    teja.bio = "Collecting moments, not photos. Circling the table since day one.".into();
    teja.posts = 1;
    let mut maya = demo_profile("maya", "maya3d", "Maya");
    maya.bio = "Sculptor. I remix other people's light.".into();
    maya.followers = 214;
    maya.following = 86;
    let mut noah = demo_profile("noah", "noah.space", "Noah");
    noah.bio = "Orbits, mostly.".into();
    noah.followers = 97;
    noah.following = 143;

    let mut backend = MemorySocialBackend::new(teja);
    backend.add_profile(maya);
    backend.add_profile(noah);
    let mut app = ThreeApp::new(backend);
    app.begin_create();
    app.composer.caption =
        "A real moment, in depth — pan it, pinch into it, hold to come home.".into();
    app.attach_capture(&sample_capture())
        .expect("the sample capture encodes");
    let post = app.publish_composer().expect("seed post publishes");

    // Two more people's moments, so the feed is a feed. Each is published by
    // its own author through the same composer.
    for (who, caption) in [
        ("maya", "Remixed the window light — rotate it slowly."),
        ("noah", "Orbit study no. 4: the desk, from underneath."),
    ] {
        let _ = app.backend_mut().set_active_user(UserId(who.to_owned()));
        app.begin_create();
        app.composer.caption = caption.into();
        app.attach_capture(&sample_capture())
            .expect("the sample capture encodes");
        app.publish_composer().expect("seed post publishes");
    }
    let _ = app.backend_mut().set_active_user(UserId("teja".into()));

    // The trail, left by real actors through the real actions. `set_active_user`
    // is the whole trick: the backend attributes every act to whoever is
    // active, exactly as a multi-user server would.
    let teja_id = UserId("teja".into());
    {
        let backend = app.backend_mut();
        let _ = backend.set_active_user(UserId("maya".into()));
        let _ = backend.like(&post.id);
        let _ = backend.follow(&teja_id);
        let _ = backend.set_active_user(UserId("noah".into()));
        let _ = backend.like(&post.id);
        let _ = backend.comment(&post.id, "The orbit read on this one is perfect.".into());
        let _ = backend.follow(&teja_id);
        let _ = backend.set_active_user(teja_id.clone());
    }
    app.refresh_home().expect("the feed refreshes");
    app
}

/// One row of the Activity feed, resolved for drawing: who did what, and the
/// glyph that says which kind of act it was.
#[derive(Clone, Debug)]
struct ActivityRow {
    actor: String,
    actor_handle: String,
    verb: String,
    unread: bool,
    kind: NotificationKind,
}

/// The Create tab's camera flow: what a recording is doing right now.
enum CapturePhase {
    /// No camera on this build — the honest state of a desktop run.
    NoSource,
    Idle,
    Recording(Box<DynRecording>),
    Failed(String),
}

impl fmt::Debug for CapturePhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Not the recording's Debug — it has none, and a dump wants the
        // phase and its progress, not the source.
        match self {
            Self::NoSource => f.write_str("CapturePhase::NoSource"),
            Self::Idle => f.write_str("CapturePhase::Idle"),
            Self::Recording(recording) => f
                .debug_struct("CapturePhase::Recording")
                .field("frames", &recording.frame_count())
                .field("progress", &recording.progress())
                .finish(),
            Self::Failed(error) => f.debug_tuple("CapturePhase::Failed").field(error).finish(),
        }
    }
}

thread_local! {
    static SHARE_HOOK: RefCell<Option<Rc<dyn Fn(String) -> bool>>> = const { RefCell::new(None) };
}

/// Install the platform's share sheet: it is handed the post's link. Until a
/// host installs one, Share says it is unavailable instead of doing nothing.
pub fn set_share_hook(hook: impl Fn(String) -> bool + 'static) {
    SHARE_HOOK.with(|slot| *slot.borrow_mut() = Some(Rc::new(hook)));
}

/// Route Share through the platform's `Share` service (UX-02) the first time a
/// build can see one. A host that registers none (or only the `Unsupported`
/// stub) leaves the hook uninstalled, so the app keeps saying "not available"
/// honestly instead of claiming "Shared".
fn wire_platform_share(ctx: &BuildContext) {
    use vieww::foundation::mobile::{Share, ShareItem};
    thread_local! {
        static WIRED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if WIRED.with(std::cell::Cell::get) {
        return;
    }
    let Some(services) = ctx.inherit::<vieww::foundation::SharedServices>() else {
        return;
    };
    let Some(share) = services.get::<dyn Share>() else {
        return;
    };
    WIRED.with(|w| w.set(true));
    set_share_hook(move |link| {
        share
            .share(ShareItem {
                text: "Look at this on 3".to_owned(),
                url: Some(link),
                ..ShareItem::default()
            })
            .is_ok()
    });
}

/// One feed card's height, the list's uniform row extent.
const FEED_CARD: f32 = 470.0;

struct SocialState {
    app: ThreeApp<MemorySocialBackend>,
    pending: bool,
    capture: CapturePhase,
    /// The Explore search field's live text.
    search: String,
    /// Who the signed-in user follows, mirrored from the backend's boolean
    /// answers so the buttons can say what they do. The backend keeps its own
    /// set privately; this is the view's copy, written only by the same call
    /// that writes the backend's.
    following: HashSet<UserId>,
    /// The snapshot's backend-derived part, by `ThreeApp::revision`.
    cache: std::cell::RefCell<Option<(u64, Rc<Heavy>)>>,
    /// The feed post whose capture is playing: one at a time (TH-02).
    active: Option<PostId>,
    /// A line the user is told: share result, errors.
    notice: Option<String>,
    /// The feed's scroll offset and viewport, driven by drag.
    feed_offset: f32,
    feed_viewport: f32,
}
/// AppError has no Display; the failure line wants a string.
fn self_capture_failed(phase: &mut CapturePhase, error: &three_app::AppError) {
    *phase = CapturePhase::Failed(format!("{error:?}"));
}

impl fmt::Debug for SocialState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SocialState")
            .field("route", &self.app.route)
            .field("feed_items", &self.app.feed.posts.len())
            .finish()
    }
}
impl SocialState {
    fn new() -> Self {
        // The platform's camera (set by the entry point, see
        // `set_capture_source`) is taken here, once: the first state built
        // owns it for the process's lifetime.
        let source = CAPTURE_SOURCE.with(|slot| slot.borrow_mut().take());
        Self {
            app: seeded_app(),
            pending: false,
            search: String::new(),
            following: HashSet::new(),
            cache: std::cell::RefCell::new(None),
            active: None,
            notice: None,
            feed_offset: 0.0,
            feed_viewport: 0.0,
            capture: match source {
                Some(_) => CapturePhase::Idle,
                None => CapturePhase::NoSource,
            },
        }
    }
    fn tab(&mut self, tab: Tab) {
        self.app.select_tab(tab);
        self.pending = true;
    }
    fn toggle_feed(&mut self) {
        let next = if self.app.home_feed == HomeFeed::ForYou {
            HomeFeed::Following
        } else {
            HomeFeed::ForYou
        };
        let _ = self.app.set_home_feed(next);
        self.pending = true;
    }
    fn like(&mut self, id: &PostId) {
        let _ = self.app.like(id);
        self.pending = true;
    }
    fn set_search(&mut self, text: String) {
        self.search = text;
        self.pending = true;
    }
    /// Follow or unfollow through the product state, keeping the view's mirror
    /// in step with the boolean the backend returns.
    fn follow(&mut self, id: &UserId) {
        match self.app.follow(id) {
            Ok(true) => {
                self.following.insert(id.clone());
            }
            Ok(false) => {
                self.following.remove(id);
            }
            Err(_) => {}
        }
        self.pending = true;
    }
    /// Play this post's capture; every other card shows its poster.
    fn play(&mut self, id: &PostId) {
        self.active = Some(id.clone());
        self.pending = true;
    }
    fn open_comments(&mut self, id: &PostId) {
        self.app.navigate(Route::Comments(id.clone()));
        self.pending = true;
    }
    fn open_post(&mut self, id: &PostId) {
        self.app.navigate(Route::Post(id.clone()));
        self.pending = true;
    }
    fn open_user(&mut self, id: &UserId) {
        self.app.navigate(Route::User(id.clone()));
        self.pending = true;
    }
    fn back(&mut self) {
        self.app.select_tab(Tab::Home);
        self.pending = true;
    }
    /// Remix: start a composer whose parent is this post, and go there.
    fn remix(&mut self, id: &PostId) {
        self.app.begin_remix(id.clone());
        self.app.navigate(Route::Composer);
        self.pending = true;
    }
    /// Share through the platform seam ([`set_share_hook`]); with none
    /// installed, say so rather than do nothing.
    fn share(&mut self, id: &PostId) {
        let link = format!("three://post/{}", id.0);
        let shared = SHARE_HOOK.with(|slot| match slot.borrow().as_ref() {
            Some(hook) => hook(link),
            None => false,
        });
        self.notice = Some(if shared {
            "Shared".to_owned()
        } else {
            "Sharing is not available on this device yet".to_owned()
        });
        self.pending = true;
    }
    fn scroll_feed(&mut self, dy: f32) {
        let max = (self.app.feed.posts.len() as f32 * FEED_CARD - self.feed_viewport).max(0.0);
        let next = (self.feed_offset - dy).clamp(0.0, max);
        if (next - self.feed_offset).abs() > f32::EPSILON {
            self.feed_offset = next;
            self.pending = true;
        }
    }
    fn discard_composer(&mut self) {
        self.app.select_tab(Tab::Create);
        self.pending = true;
    }

    /// Route to the composer (the caption/publish half of the flow).
    fn route_to_composer(&mut self) {
        self.app.navigate(Route::Composer);
        self.pending = true;
    }

    /// The one action the Create tab exists for: start recording.
    fn start_recording(&mut self) {
        if !matches!(self.capture, CapturePhase::Idle | CapturePhase::Failed(_)) {
            return;
        }
        let source = CAPTURE_SOURCE.with(|slot| slot.borrow_mut().take());
        let Some(source) = source else {
            self.capture = CapturePhase::NoSource;
            self.pending = true;
            return;
        };
        // The social default: three seconds, 24 fps, 4:3 at a size the warp
        // renderer is happy to run per frame on a phone.
        match DynRecording::start_boxed(
            source,
            CaptureRequest::social_default(),
            "A moment, in depth",
        ) {
            Ok(recording) => {
                self.capture = CapturePhase::Recording(Box::new(recording));
                self.pending = true;
            }
            Err(error) => {
                self.capture = CapturePhase::Failed(error.to_string());
                self.pending = true;
            }
        }
    }

    /// One frame tick of an in-flight recording. Finishing attaches the
    /// capture to the composer and routes there.
    fn poll_recording(&mut self) {
        let still_going = match &mut self.capture {
            CapturePhase::Recording(recording) => recording.poll(),
            _ => return,
        };
        if !still_going {
            let finished = std::mem::replace(&mut self.capture, CapturePhase::Idle);
            let CapturePhase::Recording(recording) = finished else {
                unreachable!("the borrow above guarantees the variant")
            };
            match recording.finish() {
                Ok(capture) => match self.app.attach_capture(&capture) {
                    Ok(()) => self.route_to_composer(),
                    Err(error) => {
                        self.capture = CapturePhase::Failed(format!("{error:?}"));
                    }
                },
                Err(error) => self.capture = CapturePhase::Failed(error.to_string()),
            }
        }
        self.pending = true;
    }
}
impl ElementState for SocialState {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn tick(&mut self, _now: std::time::Duration) -> bool {
        // One frame of an in-flight recording per frame tick: the poll
        // contract the camera backends are built around, and the reason a
        // capture never blocks the UI thread.
        if matches!(self.capture, CapturePhase::Recording(_)) {
            self.poll_recording();
        }
        self.take_pending()
    }
    fn is_animating(&self) -> bool {
        matches!(self.capture, CapturePhase::Recording(_))
    }
    fn take_pending(&mut self) -> bool {
        std::mem::take(&mut self.pending)
    }
}
#[derive(Clone)]
struct Handle(Rc<RefCell<dyn ElementState>>);
impl Handle {
    fn write(&self, f: impl FnOnce(&mut SocialState)) {
        let mut s = self.0.borrow_mut();
        if let Some(s) = s.as_any_mut().downcast_mut::<SocialState>() {
            f(s)
        }
    }
}
impl fmt::Debug for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Handle(<SocialState>)")
    }
}

/// What a build needs to know about the capture flow — a `Clone` of the
/// phase, because the live recording itself is not cloneable (and must not
/// be: exactly one state owns it).
#[derive(Clone, Debug)]
enum CaptureRead {
    NoSource,
    Idle,
    Recording { frames: usize, progress: f32 },
    Failed(String),
}

#[derive(Clone)]
struct Snapshot {
    route: Route,
    capture: CaptureRead,
    composer_attached: bool,
    composer_title: String,
    composer_caption: String,
    feed: HomeFeed,
    posts: Vec<Post>,
    /// Everyone the backend knows, self included — Explore filters the self
    /// out at draw time so the list is what the search actually returns.
    people: Vec<Profile>,
    me: Profile,
    activity: Vec<ActivityRow>,
    following: Vec<UserId>,
    search: String,
    /// The playing post's capture, opened lazily (and only that one).
    media: Option<Rc<LazyCapture>>,
    active: Option<PostId>,
    notice: Option<String>,
    feed_offset: f32,
    /// The route's own data: a post's comments as `(author, text)`.
    comments: Vec<(String, String)>,
    detail_post: Option<Post>,
}
/// What a snapshot reads from the backend that only a mutation changes.
struct Heavy {
    people: Vec<Profile>,
    me: Profile,
    activity: Vec<ActivityRow>,
}
impl Heavy {
    fn read(s: &SocialState) -> Self {
        let people = s.app.backend().search_profiles("", 30);
        let me = people
            .iter()
            .find(|p| p.id.0 == "teja")
            .cloned()
            .unwrap_or_else(|| demo_profile("teja", "teja", "Teja"));
        let name_of = |id: &UserId| people.iter().find(|p| &p.id == id);
        let activity = s
            .app
            .backend()
            .notifications()
            .expect("the backend serves its notifications")
            .into_iter()
            .map(
                |Notification {
                     actor, kind, read, ..
                 }| {
                    let who = name_of(&actor);
                    let verb = match &kind {
                        NotificationKind::Like { .. } => "liked your moment",
                        NotificationKind::Comment { .. } => "commented on your moment",
                        NotificationKind::Follow { .. } => "started following you",
                        NotificationKind::Remix { .. } => "remixed your moment",
                    }
                    .to_string();
                    ActivityRow {
                        actor: who
                            .map(|p| p.display_name.clone())
                            .unwrap_or_else(|| "Someone".into()),
                        actor_handle: who.map(|p| format!("@{}", p.handle)).unwrap_or_default(),
                        verb,
                        unread: !read,
                        kind,
                    }
                },
            )
            .collect::<Vec<ActivityRow>>();
        Self {
            people,
            me,
            activity,
        }
    }
}
/// The post whose capture should be open: the one the user started, else
/// the feed's first.
fn active_post(s: &SocialState) -> Option<Post> {
    let posts = &s.app.feed.posts;
    s.active
        .as_ref()
        .and_then(|id| posts.iter().find(|p| &p.id == id))
        .or_else(|| posts.first())
        .cloned()
}

impl Snapshot {
    fn read(s: &SocialState) -> Self {
        // The directory, "me" and the activity list only change when the app
        // mutates (`ThreeApp::revision`), so a keystroke or a like that does
        // not touch them reads them from the cache instead of re-querying the
        // backend (TH-01).
        let revision = s.app.revision();
        let cached = {
            let slot = s.cache.borrow();
            slot.as_ref()
                .filter(|(at, _)| *at == revision)
                .map(|(_, heavy)| Rc::clone(heavy))
        };
        let heavy = cached.unwrap_or_else(|| {
            let fresh = Rc::new(Heavy::read(s));
            *s.cache.borrow_mut() = Some((revision, Rc::clone(&fresh)));
            fresh
        });
        let (people, me, activity) = (
            heavy.people.clone(),
            heavy.me.clone(),
            heavy.activity.clone(),
        );
        let capture = match &s.capture {
            CapturePhase::NoSource => CaptureRead::NoSource,
            CapturePhase::Idle => CaptureRead::Idle,
            CapturePhase::Recording(recording) => CaptureRead::Recording {
                frames: recording.frame_count(),
                progress: recording.progress(),
            },
            CapturePhase::Failed(reason) => CaptureRead::Failed(reason.clone()),
        };
        Self {
            route: s.app.route.clone(),
            capture,
            composer_attached: s.app.composer.asset_bytes.is_some(),
            composer_title: s.app.composer.title.clone(),
            composer_caption: s.app.composer.caption.clone(),
            feed: s.app.home_feed,
            posts: s.app.feed.posts.clone(),
            people,
            me,
            activity,
            following: s.following.iter().cloned().collect(),
            search: s.search.clone(),
            media: active_post(s).and_then(|post| s.app.open_post_media(&post).ok()),
            active: s.active.clone(),
            notice: s.notice.clone(),
            feed_offset: s.feed_offset,
            comments: match &s.app.route {
                Route::Comments(id) => s
                    .app
                    .comments(id)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|c| (c.author.0, c.text))
                    .collect(),
                _ => vec![],
            },
            detail_post: match &s.app.route {
                Route::Post(id) | Route::Comments(id) => {
                    s.app.feed.posts.iter().find(|p| &p.id == id).cloned()
                }
                _ => None,
            },
        }
    }
    fn initial() -> Self {
        Self {
            route: Route::Root(Tab::Home),
            capture: CaptureRead::NoSource,
            composer_attached: false,
            composer_title: String::new(),
            composer_caption: String::new(),
            feed: HomeFeed::ForYou,
            posts: vec![],
            people: vec![],
            me: demo_profile("teja", "teja", "Teja"),
            activity: vec![],
            following: vec![],
            search: String::new(),
            media: None,
            active: None,
            notice: None,
            feed_offset: 0.0,
            comments: vec![],
            detail_post: None,
        }
    }
}

#[derive(Debug)]
pub struct SocialScreen;
#[widget]
impl SocialScreen {
    fn create_state(&self) -> Option<Box<dyn ElementState>> {
        Some(Box::new(SocialState::new()))
    }
    fn build(&self, ctx: &BuildContext) -> impl Into<WidgetNode> {
        wire_platform_share(ctx);
        let theme = ThemeData::of(ctx);
        let snap = ctx
            .state::<SocialState, Snapshot>(Snapshot::read)
            .unwrap_or_else(Snapshot::initial);
        let handle = ctx.state_handle().map(Handle);
        let body = match &snap.route {
            Route::Root(Tab::Home) => home_screen(&theme, &snap, handle.clone()),
            Route::Root(Tab::Explore) => explore_screen(&theme, &snap, handle.clone()),
            Route::Root(Tab::Create) => create_screen(&theme, &snap, handle.clone()),
            Route::Composer => create_screen(&theme, &snap, handle.clone()),
            Route::Root(Tab::Activity) => activity_screen(&theme, &snap),
            Route::Root(Tab::Profile) => profile_screen(&theme, &snap),
            Route::Post(_) => post_screen(&theme, &snap, handle.clone()),
            Route::Comments(_) => comments_screen(&theme, &snap, handle.clone()),
            Route::User(id) => user_screen(&theme, &snap, id, handle.clone()),
        };
        Container::new().color(theme.colors.surface).child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .push(Flexible::expanded(1).child(body))
                .push(navbar(&theme, &snap, handle)),
        )
    }
}

// ── the screens ─────────────────────────────────────────────────────────────

fn home_screen(theme: &ThemeData, snap: &Snapshot, handle: Option<Handle>) -> WidgetNode {
    let toggle = handle.clone();
    let label = if snap.feed == HomeFeed::ForYou {
        "For You"
    } else {
        "Following"
    };
    let header = Flex::row()
        .main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .push(Text::new("3").style(theme.text.title).size(28.0).bold())
        .push(Button::new(label).on_pressed(move || {
            if let Some(h) = &toggle {
                h.write(|s| s.toggle_feed())
            }
        }));

    // Every post is one moment card: author, caption, the capture (only the
    // playing one) and the action row on one raised surface. The list is a
    // `ListView` of uniform cards, so a feed of any length builds only the
    // cards near the window, and exactly one capture plays at a time.
    let posts: Rc<Vec<Post>> = Rc::new(snap.posts.clone());
    let active = snap
        .active
        .clone()
        .or_else(|| snap.posts.first().map(|p| p.id.clone()));
    let media = snap.media.clone();
    let card_theme = theme.clone();
    let card_handle = handle.clone();
    let list = ListView::new(
        posts.len(),
        FEED_CARD,
        Rc::new(move |index| match posts.get(index) {
            Some(post) => moment_card(
                &card_theme,
                post,
                active.as_ref() == Some(&post.id),
                media.clone(),
                card_handle.clone(),
            ),
            None => SizedBox::shrink().into(),
        }),
    );
    let drag = handle.clone();
    let extents = handle.clone();
    let feed: WidgetNode = Scrollable::vertical(snap.feed_offset)
        .on_drag(Rc::new(move |d: vieww::foundation::DragDetails| {
            if let Some(h) = &drag {
                h.write(|s| s.scroll_feed(d.delta.dy));
            }
        }))
        .on_extents(Rc::new(move |e: ScrollExtents| {
            if let Some(h) = &extents {
                h.write(|s| s.feed_viewport = e.viewport);
            }
        }))
        .child(list)
        .into();

    let mut column = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(12.0)
        .push(header);
    if let Some(note) = &snap.notice {
        column = column.push(
            Text::new(note.clone())
                .style(theme.text.label)
                .color(theme.colors.on_surface_variant),
        );
    }
    column.push(Flexible::expanded(1).child(feed)).into()
}

/// One post as a card, with every button wired.
fn moment_card(
    theme: &ThemeData,
    post: &Post,
    playing: bool,
    media: Option<Rc<LazyCapture>>,
    handle: Option<Handle>,
) -> WidgetNode {
    let (like, comment, remix, share, play, author) = (
        handle.clone(),
        handle.clone(),
        handle.clone(),
        handle.clone(),
        handle.clone(),
        handle,
    );
    let (id_like, id_comment, id_remix, id_share, id_play) = (
        post.id.clone(),
        post.id.clone(),
        post.id.clone(),
        post.id.clone(),
        post.id.clone(),
    );
    let author_id = post.author.clone();
    let chamber: WidgetNode = if playing {
        match media {
            Some(capture) => Flexible::expanded(1).child(ViewerScreen { capture }).into(),
            None => Flexible::expanded(1).child(SizedBox::shrink()).into(),
        }
    } else {
        Flexible::expanded(1)
            .child(
                Container::new()
                    .color(theme.colors.surface)
                    .radius(12.0)
                    .alignment(Alignment::CENTER)
                    .child(Button::new("▶ Play").on_pressed(move || {
                        if let Some(h) = &play {
                            h.write(|s| s.play(&id_play));
                        }
                    })),
            )
            .into()
    };
    let card = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(10.0)
        .push(
            // In a row so the button sizes to its label instead of
            // stretching across the card.
            Flex::row().push(
                Button::new(format!("@{}", post.author.0)).on_pressed(move || {
                    if let Some(h) = &author {
                        h.write(|s| s.open_user(&author_id));
                    }
                }),
            ),
        )
        .push(Text::new(post.caption.clone()).style(theme.text.body))
        .push(chamber)
        .push(
            Flex::row()
                .spacing(8.0)
                .push(
                    Button::new(format!("♥ {}", post.likes)).on_pressed(move || {
                        if let Some(h) = &like {
                            h.write(|s| s.like(&id_like));
                        }
                    }),
                )
                .push(
                    Button::new(format!("Comment {}", post.comments)).on_pressed(move || {
                        if let Some(h) = &comment {
                            h.write(|s| s.open_comments(&id_comment));
                        }
                    }),
                )
                .push(Button::new("Remix").on_pressed(move || {
                    if let Some(h) = &remix {
                        h.write(|s| s.remix(&id_remix));
                    }
                }))
                .push(Button::new("Share").on_pressed(move || {
                    if let Some(h) = &share {
                        h.write(|s| s.share(&id_share));
                    }
                })),
        );
    Padding::new(EdgeInsets::only(0.0, 0.0, 0.0, 10.0))
        .child(
            Container::new()
                .color(theme.colors.surface_variant)
                .radius(18.0)
                .shadow(Shadow::new(
                    Color::rgba(0, 0, 0, 110),
                    Offset::new(0.0, 12.0),
                    32.0,
                ))
                .padding(EdgeInsets::all(12.0))
                .child(card),
        )
        .into()
}

/// One post, full screen: the card, and a way back.
fn post_screen(theme: &ThemeData, snap: &Snapshot, handle: Option<Handle>) -> WidgetNode {
    let back = handle.clone();
    let body: WidgetNode = match &snap.detail_post {
        Some(post) => Flexible::expanded(1)
            .child(moment_card(theme, post, true, snap.media.clone(), handle))
            .into(),
        None => Text::new("This moment is no longer in your feed.")
            .style(theme.text.body)
            .into(),
    };
    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(12.0)
        .push(Button::new("← Back").on_pressed(move || {
            if let Some(h) = &back {
                h.write(|s| s.back());
            }
        }))
        .push(body)
        .into()
}

/// A post's comments.
fn comments_screen(theme: &ThemeData, snap: &Snapshot, handle: Option<Handle>) -> WidgetNode {
    let back = handle;
    let mut column = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(10.0)
        .push(Button::new("← Back").on_pressed(move || {
            if let Some(h) = &back {
                h.write(|s| s.back());
            }
        }))
        .push(Text::new("Comments").style(theme.text.title).bold());
    if snap.comments.is_empty() {
        column = column.push(
            Text::new("No comments yet.")
                .style(theme.text.body)
                .color(theme.colors.on_surface_variant),
        );
    }
    for (author, text) in &snap.comments {
        column = column.push(
            Flex::column()
                .spacing(2.0)
                .push(
                    Text::new(format!("@{author}"))
                        .style(theme.text.label)
                        .color(theme.colors.primary)
                        .bold(),
                )
                .push(Text::new(text.clone()).style(theme.text.body)),
        );
    }
    column.into()
}

/// Someone else's profile, with the Follow button wired.
fn user_screen(
    theme: &ThemeData,
    snap: &Snapshot,
    id: &UserId,
    handle: Option<Handle>,
) -> WidgetNode {
    let back = handle.clone();
    let follow = handle;
    let target = id.clone();
    let person = snap.people.iter().find(|p| &p.id == id);
    let following = snap.following.contains(id);
    let mut column = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(10.0)
        .push(Button::new("← Back").on_pressed(move || {
            if let Some(h) = &back {
                h.write(|s| s.back());
            }
        }));
    match person {
        Some(p) => {
            column = column
                .push(
                    Text::new(p.display_name.clone())
                        .style(theme.text.title)
                        .bold(),
                )
                .push(
                    Text::new(format!("@{}", p.handle))
                        .style(theme.text.label)
                        .color(theme.colors.primary),
                )
                .push(Text::new(p.bio.clone()).style(theme.text.body))
                .push(
                    Text::new(format!(
                        "{} posts · {} followers · {} following",
                        p.posts, p.followers, p.following
                    ))
                    .style(theme.text.label)
                    .color(theme.colors.on_surface_variant),
                )
                .push(
                    Button::new(if following { "Following" } else { "Follow" }).on_pressed(
                        move || {
                            if let Some(h) = &follow {
                                h.write(|s| s.follow(&target));
                            }
                        },
                    ),
                );
        }
        None => {
            column = column.push(Text::new("Nobody by that name.").style(theme.text.body));
        }
    }
    column.into()
}

/// The feed card's media: the first post's asset, opened lazily the way
/// a downloaded asset is. Falls back to the deterministic sample when the
/// feed is somehow empty, so the card never mounts a blank chamber.
fn feed_media(snap: &Snapshot) -> Rc<LazyCapture> {
    match (&snap.media, snap.posts.first()) {
        (Some(media), Some(_)) => Rc::clone(media),
        _ => Rc::new(
            LazyCapture::open(three_runtime::save(&sample_capture()).expect("sample encodes"))
                .expect("sample opens"),
        ),
    }
}

/// Explore: a live search field over the backend's own people, each card
/// carrying a Follow button wired through the product state. The list the
/// field filters is `search_profiles`' actual answer — the same call the
/// networked client makes.
fn explore_screen(theme: &ThemeData, snap: &Snapshot, handle: Option<Handle>) -> WidgetNode {
    let on_search = handle.clone();
    let field = Container::new()
        .color(theme.colors.surface_variant)
        .radius(f32::MAX)
        .padding(EdgeInsets::symmetric(6.0, 4.0))
        .child(
            Flex::row()
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .spacing(8.0)
                .push(
                    Icon::new(icons::search())
                        .size(17.0)
                        .color(theme.colors.on_surface_variant),
                )
                .push(
                    Flexible::expanded(1).child(
                        TextField::text(snap.search.clone())
                            .placeholder("Search people")
                            .placeholder_color(theme.colors.on_surface_variant)
                            .color(theme.colors.on_surface)
                            .cursor(theme.colors.primary, 2.0)
                            .single_line()
                            .on_changed(Rc::new(move |value: TextEditingValue| {
                                if let Some(h) = &on_search {
                                    h.write(|s| s.set_search(value.text));
                                }
                            })),
                    ),
                ),
        );

    let q = snap.search.trim().to_lowercase();
    let people: Vec<&Profile> = snap
        .people
        .iter()
        .filter(|p| p.id.0 != "teja")
        .filter(|p| {
            q.is_empty()
                || p.handle.to_lowercase().contains(&q)
                || p.display_name.to_lowercase().contains(&q)
        })
        .collect();

    let mut body: Vec<WidgetNode> = vec![Text::new("People").style(theme.text.title).bold().into()];
    if people.is_empty() {
        body.push(
            Text::new("No one by that name yet.")
                .style(theme.text.body)
                .color(theme.colors.on_surface_variant)
                .into(),
        );
    }
    for person in people {
        let follow = handle.clone();
        let id = person.id.clone();
        let is_following = snap.following.iter().any(|f| f == &id);
        let initials: String = person
            .display_name
            .split_whitespace()
            .take(2)
            .filter_map(|w| w.chars().next())
            .flat_map(char::to_uppercase)
            .collect();
        body.push(person_card(
            theme,
            person,
            &initials,
            is_following,
            move || {
                if let Some(h) = &follow {
                    h.write(|s| s.follow(&id));
                }
            },
        ));
    }

    Container::new()
        .padding(EdgeInsets::all(20.0))
        .child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .spacing(12.0)
                .push(field)
                .push(
                    Flexible::expanded(1).child(
                        Scrollable::vertical(0.0).child(
                            Flex::column()
                                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                                .spacing(10.0)
                                .children(body),
                        ),
                    ),
                ),
        )
        .into()
}

/// One person in Explore: an avatar disc carrying the person's own accent,
/// their name and handle, and the follow control.
fn person_card(
    theme: &ThemeData,
    person: &Profile,
    initials: &str,
    following: bool,
    on_follow: impl Fn() + 'static,
) -> WidgetNode {
    let avatar = Container::new()
        .size(46.0, 46.0)
        .radius(f32::MAX)
        .gradient(accent_for(&person.handle))
        .alignment(Alignment::CENTER)
        .child(
            Text::new(initials.to_string())
                .color(Color::WHITE)
                .size(15.0)
                .bold(),
        );

    let names = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .spacing(1.0)
        .push(
            Text::new(person.display_name.clone())
                .style(theme.text.body)
                .bold(),
        )
        .push(
            Text::new(format!(
                "{} · {} followers",
                person.handle, person.followers
            ))
            .style(theme.text.label)
            .color(theme.colors.on_surface_variant),
        );

    let button = if following {
        Button::new("Following")
    } else {
        Button::new("Follow").style(ButtonStyle::Filled)
    };
    let button = button.on_pressed(on_follow);

    Container::new()
        .color(theme.colors.surface_variant)
        .radius(16.0)
        .padding(EdgeInsets::symmetric(12.0, 10.0))
        .child(
            Flex::row()
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .spacing(12.0)
                .push(avatar)
                .push(Flexible::expanded(1).child(names))
                .push(button),
        )
        .into()
}

/// A stable per-person accent: the theme's primary hue rotated by a hash of
/// the handle, so a feed of people is a feed of distinguishable people
/// without inventing a colour system the theme does not carry.
fn accent_for(handle: &str) -> Gradient {
    let hash = handle.bytes().fold(0x811c9dc5u32, |h, b| {
        (h ^ b as u32).wrapping_mul(0x01000193)
    });
    let hue = (hash % 360) as f32;
    let top = hsl(hue, 0.62, 0.62);
    let bottom = hsl(hue + 34.0, 0.66, 0.42);
    Gradient::linear(Offset::new(0.5, 0.0), Offset::new(0.5, 1.0))
        .with_stops(&[(0.0, top), (1.0, bottom)])
}

/// HSL → RGB, the one colour helper this shell needs. The framework's `Color`
/// takes components; it does not take a wheel position.
fn hsl(hue: f32, saturation: f32, lightness: f32) -> Color {
    let h = ((hue % 360.0) + 360.0) % 360.0 / 360.0;
    let s = saturation.clamp(0.0, 1.0);
    let l = lightness.clamp(0.0, 1.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h * 6.0) % 2.0 - 1.0).abs());
    let m = l - c * 0.5;
    let (r, g, b) = match (h * 6.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Color::rgb(
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

/// Create: the pipeline as a timeline — six numbered stages, joined by a
/// Create: the camera, honestly. One button records a three-second moment
/// through the platform's depth-video source; the progress line is the
/// recording's own numbers; failure names its reason. On a build with no
/// camera wired (a desktop run), the state says so and the sample moment
/// is the dev path — the feed's content pipeline is identical either way,
/// which is the point of the trait boundary.
fn create_screen(theme: &ThemeData, snap: &Snapshot, handle: Option<Handle>) -> WidgetNode {
    // The recording's own numbers, read from the live state.
    let (recording, progress) = match &snap.capture {
        CaptureRead::Recording { frames, progress } => (Some(*frames), *progress),
        _ => (None, 0.0),
    };

    let mut column: Vec<WidgetNode> = vec![
        Text::new("Create").style(theme.text.title).bold().into(),
        Text::new("A real moment, in depth — the camera records video and the depth the hardware measures with it.")
            .style(theme.text.body)
            .into(),
    ];

    match &snap.capture {
        CaptureRead::NoSource => {
            column.push(
                Text::new("No camera on this build — run it on a phone to record. The dev path below publishes the same container.")
                    .style(theme.text.label)
                    .color(theme.colors.on_surface_variant)
                    .into(),
            );
        }
        CaptureRead::Idle => {
            column.push(
                Text::new(
                    "Three seconds, 24 fps. The subject at arm's length; the depth does the rest.",
                )
                .style(theme.text.label)
                .color(theme.colors.on_surface_variant)
                .into(),
            );
        }
        CaptureRead::Recording { .. } => {
            column.push(
                Text::new(format!(
                    "Recording… {} frames ({:.0}%)",
                    recording.unwrap_or(0),
                    progress * 100.0
                ))
                .style(theme.text.label)
                .color(theme.colors.primary)
                .bold()
                .into(),
            );
            // The progress as an honest bar: a track plus a fill.
            column.push(
                Container::new()
                    .height(8.0)
                    .radius(f32::MAX)
                    .color(theme.colors.surface_variant)
                    .alignment(Alignment::CENTER_LEFT)
                    .child(
                        Container::new()
                            .height(8.0)
                            .width((progress * 100.0).clamp(1.0, 100.0))
                            .radius(f32::MAX)
                            .color(theme.colors.primary),
                    )
                    .into(),
            );
        }
        CaptureRead::Failed(reason) => {
            column.push(
                Text::new(format!("The camera declined: {reason}"))
                    .style(theme.text.label)
                    .color(theme.colors.error)
                    .into(),
            );
        }
    }

    // The composer route shows the caption/publish flow once a capture is
    // attached; the Create tab is the camera.
    let body = if matches!(snap.route, Route::Composer) {
        composer_screen(theme, snap, handle.clone())
    } else {
        let mut rows = column;
        let actions = create_actions(theme, snap, handle.clone());
        rows.push(Flexible::expanded(1).child(SizedBox::new()).into());
        rows.push(actions);
        Flex::column()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .spacing(10.0)
            .children(rows)
            .into()
    };
    Container::new()
        .padding(EdgeInsets::all(20.0))
        .child(body)
        .into()
}

/// The Create tab's buttons: what can be done right now.
fn create_actions(theme: &ThemeData, snap: &Snapshot, handle: Option<Handle>) -> WidgetNode {
    let record = handle.clone();
    let sample = handle.clone();
    let mut row = Flex::row().spacing(8.0);

    match &snap.capture {
        CaptureRead::Idle | CaptureRead::Failed(_) => {
            row = row.push(
                Flexible::expanded(1).child(
                    Button::new("Record a moment")
                        .style(ButtonStyle::Filled)
                        .on_pressed(move || {
                            if let Some(h) = &record {
                                h.write(|s| s.start_recording());
                            }
                        }),
                ),
            );
        }
        CaptureRead::Recording { .. } => {
            row = row.push(
                Flexible::expanded(1).child(Button::new("Recording…").style(ButtonStyle::Text)),
            );
        }
        CaptureRead::NoSource => {}
    }

    row = row.push(
        Flexible::expanded(1).child(
            Button::new("Use the sample moment")
                .style(ButtonStyle::Text)
                .on_pressed(move || {
                    if let Some(h) = &sample {
                        h.write(|s| {
                            if let Err(error) = s.app.attach_capture(&sample_capture()) {
                                s.capture = CapturePhase::Failed(format!("{error:?}"));
                            } else {
                                s.route_to_composer();
                            }
                            s.pending = true;
                        });
                    }
                }),
        ),
    );

    let _ = theme;
    row.into()
}

/// The composer: caption and publish for an attached capture.
fn composer_screen(theme: &ThemeData, snap: &Snapshot, handle: Option<Handle>) -> WidgetNode {
    let publish = handle.clone();
    let caption = handle.clone();
    let discard = handle.clone();

    let attached = snap.composer_attached;

    let mut rows: Vec<WidgetNode> = vec![
        Text::new("Your moment")
            .style(theme.text.title)
            .bold()
            .into(),
        Text::new(if attached {
            format!(
                "Attached: {} — the moment is ready to publish",
                snap.composer_title
            )
        } else {
            "Nothing attached — record or use the sample first.".into()
        })
        .style(theme.text.label)
        .color(theme.colors.on_surface_variant)
        .into(),
    ];

    let field = Container::new()
        .color(theme.colors.surface_variant)
        .radius(12.0)
        .padding(EdgeInsets::all(12.0))
        .child(
            TextField::text(snap.composer_caption.clone())
                .placeholder("Say what the moment was…")
                .placeholder_color(theme.colors.on_surface_variant)
                .color(theme.colors.on_surface)
                .cursor(theme.colors.primary, 2.0)
                .single_line()
                .on_changed(Rc::new(move |value: TextEditingValue| {
                    if let Some(h) = &caption {
                        h.write(|s| {
                            s.app.composer.caption = value.text;
                            s.pending = true;
                        });
                    }
                })),
        );
    rows.push(field.into());

    let publish_button = Button::new("Publish")
        .style(if attached {
            ButtonStyle::Filled
        } else {
            ButtonStyle::Text
        })
        .on_pressed(move || {
            if let Some(h) = &publish {
                h.write(|s| {
                    match s.app.publish_composer() {
                        Ok(_) => {
                            s.capture = CapturePhase::Idle;
                        }
                        Err(error) => {
                            self_capture_failed(&mut s.capture, &error);
                        }
                    }
                    s.pending = true;
                });
            }
        });
    let buttons = Flex::row()
        .spacing(8.0)
        .push(
            Flexible::expanded(1).child(
                Button::new("Discard")
                    .style(ButtonStyle::Text)
                    .on_pressed(move || {
                        if let Some(h) = &discard {
                            h.write(|s| s.discard_composer());
                        }
                    }),
            ),
        )
        .push(Flexible::expanded(1).child(publish_button));
    rows.push(buttons.into());

    Container::new()
        .padding(EdgeInsets::all(20.0))
        .child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .spacing(12.0)
                .children(rows),
        )
        .into()
}

/// Activity: the backend's own notification feed, each act with its glyph and
/// actor. Every row happened — see `seeded_app` for the acts that left them.
fn activity_screen(theme: &ThemeData, snap: &Snapshot) -> WidgetNode {
    let mut rows: Vec<WidgetNode> =
        vec![Text::new("Activity").style(theme.text.title).bold().into()];

    if snap.activity.is_empty() {
        rows.push(Text::new("Likes, comments, follows and remixes land here — the moment you publish, so does the first one.")
            .style(theme.text.body).color(theme.colors.on_surface_variant).into());
    }
    for item in &snap.activity {
        let (glyph, tint) = match item.kind {
            NotificationKind::Like { .. } => (icons::heart(), theme.colors.primary),
            NotificationKind::Comment { .. } => (icons::comment(), theme.colors.primary),
            NotificationKind::Follow { .. } => (icons::person(), theme.colors.primary),
            NotificationKind::Remix { .. } => (icons::cube(), theme.colors.primary),
        };
        let disc = Container::new()
            .size(36.0, 36.0)
            .radius(f32::MAX)
            .color(theme.colors.surface_variant)
            .alignment(Alignment::CENTER)
            .child(Icon::new(glyph).size(16.0).color(tint));

        let mut line = Flex::row()
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .spacing(10.0)
            .push(disc)
            .push(
                Flexible::expanded(1).child(
                    Flex::column()
                        .cross_axis_alignment(CrossAxisAlignment::Start)
                        .spacing(1.0)
                        .push(
                            Flex::row()
                                .cross_axis_alignment(CrossAxisAlignment::Baseline)
                                .spacing(6.0)
                                .push(Text::new(item.actor.clone()).style(theme.text.body).bold())
                                .push(
                                    Text::new(item.actor_handle.clone())
                                        .style(theme.text.label)
                                        .color(theme.colors.on_surface_variant),
                                ),
                        )
                        .push(
                            Text::new(item.verb.clone())
                                .style(theme.text.label)
                                .color(theme.colors.on_surface_variant),
                        ),
                ),
            );
        if item.unread {
            line = line.push(
                Container::new()
                    .size(8.0, 8.0)
                    .radius(f32::MAX)
                    .color(theme.colors.primary),
            );
        }
        rows.push(
            Container::new()
                .color(theme.colors.surface_variant)
                .radius(16.0)
                .padding(EdgeInsets::symmetric(12.0, 10.0))
                .child(line)
                .into(),
        );
    }

    Container::new()
        .padding(EdgeInsets::all(20.0))
        .child(
            Scrollable::vertical(0.0).child(
                Flex::column()
                    .cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .spacing(10.0)
                    .children(rows),
            ),
        )
        .into()
}

/// Profile: the signed-in person, their numbers, and their moments as cards.
fn profile_screen(theme: &ThemeData, snap: &Snapshot) -> WidgetNode {
    let me = &snap.me;

    let ring = Container::new()
        .size(84.0, 84.0)
        .radius(f32::MAX)
        .gradient(
            Gradient::linear(Offset::new(0.0, 0.0), Offset::new(1.0, 1.0))
                .with_stops(&[(0.0, theme.colors.primary), (1.0, hsl(210.0, 0.7, 0.6))]),
        )
        .padding(EdgeInsets::all(3.0))
        .child(
            Container::new()
                .color(theme.colors.surface)
                .radius(f32::MAX)
                .padding(EdgeInsets::all(3.0))
                .child(
                    Container::new()
                        .size(72.0, 72.0)
                        .radius(f32::MAX)
                        .color(theme.colors.surface_variant)
                        .alignment(Alignment::CENTER)
                        .child(
                            Text::new("T".to_string())
                                .size(28.0)
                                .bold()
                                .color(theme.colors.on_surface),
                        ),
                ),
        );

    let stat = |label: &str, value: &str| {
        Flex::column()
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .spacing(2.0)
            .push(
                Text::new(value.to_string())
                    .size(19.0)
                    .bold()
                    .color(theme.colors.on_surface),
            )
            .push(
                Text::new(label.to_string())
                    .style(theme.text.label)
                    .color(theme.colors.on_surface_variant),
            )
    };

    let header = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(12.0)
        .push(
            Flex::row()
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .spacing(14.0)
                .push(ring)
                .push(
                    Flexible::expanded(1).child(
                        Flex::column()
                            .cross_axis_alignment(CrossAxisAlignment::Start)
                            .spacing(3.0)
                            .push(
                                Text::new(me.display_name.clone())
                                    .style(theme.text.title)
                                    .bold(),
                            )
                            .push(
                                Text::new(format!("@{}", me.handle))
                                    .style(theme.text.label)
                                    .color(theme.colors.primary),
                            )
                            .push(
                                Text::new(me.bio.clone())
                                    .style(theme.text.body)
                                    .color(theme.colors.on_surface_variant),
                            ),
                    ),
                ),
        )
        .push(
            Container::new()
                .color(theme.colors.surface_variant)
                .radius(16.0)
                .padding(EdgeInsets::symmetric(8.0, 12.0))
                .child(
                    Flex::row()
                        .main_axis_alignment(MainAxisAlignment::SpaceAround)
                        .push(stat("moments", &me.posts.to_string()))
                        .push(stat("followers", &me.followers.to_string()))
                        .push(stat("following", &me.following.to_string())),
                ),
        );

    let mut moments: Vec<WidgetNode> =
        vec![Text::new("Moments").style(theme.text.title).bold().into()];
    for post in &snap.posts {
        moments.push(
            Container::new()
                .color(theme.colors.surface_variant)
                .radius(16.0)
                .padding(EdgeInsets::all(12.0))
                .child(
                    Flex::column()
                        .cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .spacing(8.0)
                        .push(
                            Flex::row()
                                .cross_axis_alignment(CrossAxisAlignment::Center)
                                .spacing(8.0)
                                .push(
                                    Icon::new(icons::cube())
                                        .size(15.0)
                                        .color(theme.colors.primary),
                                )
                                .push(
                                    Text::new("3.0s · .3 moment".to_string())
                                        .style(theme.text.label)
                                        .color(theme.colors.on_surface_variant),
                                )
                                .push(Flexible::expanded(1).child(SizedBox::new()))
                                .push(
                                    Icon::new(icons::heart())
                                        .size(13.0)
                                        .color(theme.colors.on_surface_variant),
                                )
                                .push(
                                    Text::new(post.likes.to_string())
                                        .style(theme.text.label)
                                        .color(theme.colors.on_surface_variant),
                                )
                                .push(
                                    Icon::new(icons::comment())
                                        .size(13.0)
                                        .color(theme.colors.on_surface_variant),
                                )
                                .push(
                                    Text::new(post.comments.to_string())
                                        .style(theme.text.label)
                                        .color(theme.colors.on_surface_variant),
                                ),
                        )
                        .push(Text::new(post.caption.clone()).style(theme.text.body)),
                )
                .into(),
        );
    }
    if snap.posts.is_empty() {
        moments.push(
            Text::new("Nothing published yet. The first orbit is one tap away.")
                .style(theme.text.body)
                .color(theme.colors.on_surface_variant)
                .into(),
        );
    }

    Container::new()
        .padding(EdgeInsets::all(20.0))
        .child(
            Scrollable::vertical(0.0).child(
                Flex::column()
                    .cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .spacing(12.0)
                    .push(header)
                    .children(moments),
            ),
        )
        .into()
}

fn simple_screen(theme: &ThemeData, title: &str, subtitle: &str) -> WidgetNode {
    // Kept for the detail routes the shell does not draw yet — a title, a
    // rule, a line, rather than a blank surface.
    Container::new()
        .padding(EdgeInsets::all(20.0))
        .child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .spacing(10.0)
                .push(Text::new(title).style(theme.text.title).bold())
                .push(
                    Container::new()
                        .height(2.0)
                        .width(44.0)
                        .radius(f32::MAX)
                        .color(theme.colors.primary),
                )
                .push(
                    Text::new(subtitle)
                        .style(theme.text.body)
                        .color(theme.colors.on_surface_variant),
                ),
        )
        .into()
}
fn navbar(theme: &ThemeData, snap: &Snapshot, handle: Option<Handle>) -> WidgetNode {
    // The bar sits on its own raised surface with a hairline above it, so
    // the content scrolls *under* something rather than into nothing. The
    // active tab is the filled one and the rest are text — the framework's
    // own button hierarchy doing the work the "•" bullet used to do alone.
    let active = match &snap.route {
        Route::Root(t) => Some(*t),
        _ => None,
    };
    let mut row = Flex::row().spacing(6.0);
    for (tab, name) in [
        (Tab::Home, "Home"),
        (Tab::Explore, "Explore"),
        (Tab::Create, "Create"),
        (Tab::Activity, "Activity"),
        (Tab::Profile, "Profile"),
    ] {
        let h = handle.clone();
        let is_active = active == Some(tab);
        let mut b = Button::new(name).on_pressed(move || {
            if let Some(h) = &h {
                h.write(|s| s.tab(tab))
            }
        });
        b = if is_active {
            b.style(ButtonStyle::Filled)
        } else {
            b.style(ButtonStyle::Text)
        };
        row = row.push(Flexible::expanded(1).child(b));
    }
    Container::new()
        .color(theme.colors.surface_variant)
        .border(Border::thin(theme.colors.outline))
        .padding(EdgeInsets::symmetric(10.0, 6.0))
        .child(row)
        .into()
}
/// Desktop entry — the phone-shaped window the harness opens.
///
/// Called from `src/main.rs`; kept here so the Android entry point below can
/// share the exact same tree without duplicating the setup.
pub fn run_desktop() -> Result<(), Box<dyn std::error::Error>> {
    let report = App::new()
        .title("3 — social 3D")
        .size(Size::new(480.0, 860.0))
        .theme(ThemeData::dark())
        .run(|driver| driver.set_root(WidgetNode::new(SocialScreen)))?;
    println!("{report}");
    Ok(())
}

// ── Android entry point ─────────────────────────────────────────────────────
//
// Android does not call `main` — the activity loads this crate's `cdylib` and
// calls `android_main`. The tree is the same one `run_desktop` mounts; the only
// difference is that `App::run_android` takes the `AndroidApp` handle so the
// event loop is built around the activity's existing looper and surface.

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(android: vieww_platform_winit::AndroidApp) {
    let app = App::new().title("3 — social 3D").theme(ThemeData::dark());

    if let Err(error) = app.run_android(android, |driver| {
        driver.set_root(WidgetNode::new(SocialScreen));
    }) {
        eprintln!("3 (social): {error}");
    }
}

// ── the receipts ─────────────────────────────────────────────────────────────
//
// The five screens of the app's story, rendered through the native
// rasteriser with no display and no window — the same tree `App::run` shows,
// mounted in the same harness the viewer's own `previews.rs` uses. Run on
// demand (ignored by default because it writes files):
//
// ```text
// cargo test -p three-social-app -- --ignored --nocapture
// ```
#[cfg(test)]
mod receipts {
    use super::*;
    use std::time::Duration;
    use vieww_test_harness::visual;
    use vieww_test_harness::TestHarness;

    const WINDOW: Size = Size {
        width: 480.0,
        height: 860.0,
    };
    const OUT_DIR: &str = "target/screens";

    fn mounted_app() -> TestHarness {
        let mut harness = TestHarness::new(WINDOW);
        harness.mount(Theme::new(ThemeData::dark()).child(WidgetNode::new(SocialScreen)));
        harness
    }

    /// Walk the social state the way a tap would, then let a frame happen.
    fn select(harness: &mut TestHarness, tab: Tab) {
        let cell = {
            let driver = harness.driver();
            let tree = driver.elements();
            let root = tree
                .iter()
                .into_iter()
                .find(|element| element.debug_name() == "SocialScreen")
                .expect("the root screen is mounted");
            root.state()
                .expect("the screen owns a social state")
                .clone()
        };
        let mut state = cell.borrow_mut();
        let social = state
            .as_any_mut()
            .downcast_mut::<SocialState>()
            .expect("the screen's state is the social state");
        social.tab(tab);
        drop(state);
        harness.request_frame();
    }

    fn save(harness: &mut TestHarness, name: &str) {
        let frame = visual::render(harness.driver(), WINDOW, Color::WHITE);
        std::fs::create_dir_all(OUT_DIR).expect("create the screens directory");
        let path = std::path::Path::new(OUT_DIR).join(name);
        frame.write_png(&path).expect("write the screen");
        println!("wrote {}", path.display());
    }

    /// TH-01: after the first build, keystrokes and likes cause no asset
    /// downloads.
    #[test]
    fn keystrokes_and_likes_cause_no_downloads_after_the_first_build() {
        let mut harness = mounted_app();
        harness.tick(Duration::from_millis(400));
        let cell = {
            let driver = harness.driver();
            let tree = driver.elements();
            let root = tree
                .iter()
                .into_iter()
                .find(|element| element.debug_name() == "SocialScreen")
                .expect("the root screen is mounted");
            root.state().expect("state").clone()
        };
        let downloads = |cell: &std::rc::Rc<std::cell::RefCell<Box<dyn std::any::Any>>>| {
            let _ = cell;
        };
        let _ = downloads;
        let before = {
            let mut state = cell.borrow_mut();
            let social = state.as_any_mut().downcast_mut::<SocialState>().unwrap();
            social.app.downloads()
        };
        assert!(before >= 1, "the first build opened the feed's first post");
        for text in ["a", "al", "ali", "alic"] {
            {
                let mut state = cell.borrow_mut();
                let social = state.as_any_mut().downcast_mut::<SocialState>().unwrap();
                social.set_search(text.to_owned());
                let id = social.app.feed.posts.first().map(|p| p.id.clone());
                if let Some(id) = id {
                    social.like(&id);
                }
            }
            harness.request_frame();
            harness.tick(Duration::from_millis(50));
        }
        let after = {
            let mut state = cell.borrow_mut();
            let social = state.as_any_mut().downcast_mut::<SocialState>().unwrap();
            social.app.downloads()
        };
        assert_eq!(after, before, "no further downloads");
    }

    #[test]
    #[ignore = "writes PNG files; run with -- --ignored --nocapture"]
    fn render_the_app_screens() {
        // 1. Home: the moment card, feed live, capture playing.
        let mut harness = mounted_app();
        harness.tick(Duration::from_millis(400));
        save(&mut harness, "1-home.png");

        // 2-5. One screen per tab, each settled a beat.
        for (tab, name) in [
            (Tab::Explore, "2-explore.png"),
            (Tab::Create, "3-create.png"),
            (Tab::Activity, "4-activity.png"),
            (Tab::Profile, "5-profile.png"),
        ] {
            select(&mut harness, tab);
            harness.tick(Duration::from_millis(120));
            save(&mut harness, name);
        }
    }
}

#[cfg(test)]
mod th02 {
    use super::*;

    fn state() -> SocialState {
        let mut s = SocialState::new();
        s.app.refresh_home().expect("seeded feed");
        s
    }

    #[test]
    fn the_action_row_buttons_all_do_something() {
        let mut s = state();
        let id = s.app.feed.posts.first().expect("a post").id.clone();

        s.open_comments(&id);
        assert_eq!(s.app.route, Route::Comments(id.clone()));
        s.back();
        assert_eq!(s.app.route, Route::Root(Tab::Home));

        s.remix(&id);
        assert_eq!(s.app.route, Route::Composer);
        assert_eq!(s.app.composer.remix_of, Some(id.clone()));
        s.back();

        // Without a host share sheet it says so; with one it hands over a link.
        s.share(&id);
        assert!(s
            .notice
            .as_deref()
            .is_some_and(|n| n.contains("not available")));
        let seen = Rc::new(RefCell::new(String::new()));
        let sink = Rc::clone(&seen);
        set_share_hook(move |link| {
            *sink.borrow_mut() = link;
            true
        });
        s.share(&id);
        assert_eq!(s.notice.as_deref(), Some("Shared"));
        assert!(seen.borrow().starts_with("three://post/"));
    }

    #[test]
    fn the_detail_routes_are_screens_not_a_placeholder() {
        let mut s = state();
        let id = s.app.feed.posts.first().expect("a post").id.clone();
        for route in [
            Route::Post(id.clone()),
            Route::Comments(id.clone()),
            Route::User(UserId("teja".into())),
        ] {
            s.app.navigate(route);
            let snap = Snapshot::read(&s);
            assert!(snap.detail_post.is_some() || matches!(snap.route, Route::User(_)));
        }
    }

    #[test]
    fn the_feed_holds_more_than_one_post_and_scrolls() {
        let mut s = state();
        let posts = s.app.feed.posts.len();
        assert!(posts > 1, "the seeded feed has {posts} posts");
        s.feed_viewport = 800.0;
        s.scroll_feed(-300.0);
        assert!(s.feed_offset > 0.0, "a drag up scrolls into the feed");
        s.scroll_feed(10_000.0);
        assert_eq!(s.feed_offset, 0.0, "and back clamps at the top");
    }
}
