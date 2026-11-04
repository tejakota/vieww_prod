//! The out-of-process preview, end to end: real `rustc`, a real child
//! process, real pixels back over the pipe.
//!
//! The assertion that matters most is the one a test process can make by
//! still being alive: a guest that aborts — which no `catch_unwind` can catch,
//! on any platform — takes its child with it and leaves this process, and a
//! fresh child, working. Skips, loudly, without a built workspace, like
//! `pipeline.rs`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use vieww_foundation::{Color, ImeEvent, KeyEvent, Offset, ScrollEvent};
use viewwstudio::compile::{compile, Session, Toolchain};
use viewwstudio::remote::{Environment, Pointer, RemoteError, RemoteEvent, RemotePreview};
use viewwstudio::state::Platform;

fn child() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_viewwstudio-preview"))
}

fn target() -> PathBuf {
    let exe = std::env::current_exe().expect("a test binary");
    exe.parent()
        .and_then(Path::parent)
        .expect("target/debug")
        .to_path_buf()
}

const PROBE: &str = r#"
use vieww::prelude::*;
#[derive(Debug)]
pub struct Probe;
impl Widget for Probe {
    fn debug_name(&self) -> &'static str { "Probe" }
    fn kind(&self) -> WidgetKind<'_> { WidgetKind::Composed }
    fn build(&self, _ctx: &BuildContext) -> WidgetNode { Container::new().into() }
}
pub fn screen() -> impl Widget { Probe }
"#;

/// The toolchain with the `vieww` candidate the **child** links — not this
/// test binary, which is a different compilation with different features.
/// Probed through a child, advancing past ABI mismatches exactly as the
/// studio does.
fn toolchain() -> Option<Toolchain> {
    static RESOLVED: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    let base = match Toolchain::discover(&target()) {
        Ok(t) => t,
        Err(error) => {
            eprintln!("skipping: {error}");
            return None;
        }
    };
    let candidate = *RESOLVED.get_or_init(|| {
        let session = Session::new(0x7E70).ok()?;
        for candidate in 0.. {
            let toolchain = base.with_candidate(candidate)?;
            let Ok(compiled) = compile(&toolchain, &session, PROBE, "probe.rs") else {
                continue;
            };
            let library = compiled.library?;
            let mut remote = RemotePreview::spawn(&child()).ok()?;
            match remote.load(&library, toolchain.sysroot_lib.as_deref()) {
                Ok(()) => return Some(candidate),
                Err(RemoteError::AbiMismatch(_)) => continue,
                Err(error) => {
                    eprintln!("probe candidate {candidate}: {error}");
                    continue;
                }
            }
        }
        None
    });
    let Some(candidate) = candidate else {
        eprintln!("skipping: no `vieww` rlib matches the preview child's compilation");
        return None;
    };
    base.with_candidate(candidate)
}

fn compiled(toolchain: &Toolchain, seed: u64, source: &str) -> PathBuf {
    let session = Session::new(seed).expect("a temp directory");
    let compiled = compile(toolchain, &session, source, "screen.rs").expect("rustc ran");
    assert!(
        compiled
            .diagnostics
            .iter()
            .all(|d| d.severity != viewwstudio::state::Severity::Error),
        "{:?}",
        compiled.diagnostics
    );
    // The session directory must outlive the load; leak it for the test.
    std::mem::forget(session);
    compiled.library.expect("it compiles")
}

fn loaded(toolchain: &Toolchain, library: &Path) -> RemotePreview {
    let mut remote = RemotePreview::spawn(&child()).expect("the child starts");
    remote
        .load(library, toolchain.sysroot_lib.as_deref())
        .expect("the child loads it");
    remote
}

const TAPPABLE: &str = concat!(
    include_str!("remote/guest_prelude.rs"),
    r#"
#[derive(Debug)]
pub struct Screen;

impl Widget for Screen {
    fn debug_name(&self) -> &'static str { "Screen" }
    fn kind(&self) -> WidgetKind<'_> { WidgetKind::Composed }
    fn create_state(&self) -> Option<Box<dyn ElementState>> { Some(Box::new(Tapped::default())) }
    fn build(&self, ctx: &BuildContext) -> WidgetNode {
        let color = if tapped(ctx) { Color::hex(0xFF_2244) } else { Color::hex(0x22_88FF) };
        GestureDetector::new().on_tap(on_tap(ctx)).child(Container::new().color(color)).into()
    }
}

pub fn screen() -> impl Widget { Screen }
"#
);

fn pixel(frame: &viewwstudio::remote::Frame, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * frame.width + x) * 4) as usize;
    [
        frame.rgba[i],
        frame.rgba[i + 1],
        frame.rgba[i + 2],
        frame.rgba[i + 3],
    ]
}

#[test]
fn a_screen_renders_in_the_child_and_a_tap_reaches_it() {
    let Some(toolchain) = toolchain() else { return };
    let library = compiled(&toolchain, 0x7E71, TAPPABLE);
    let mut remote = loaded(&toolchain, &library);
    let env = Environment::of(Platform::Android);

    let first = remote
        .frame(&env, 1.0, Duration::ZERO, Color::BLACK, &[])
        .expect("a frame");
    assert_eq!(
        (first.width, first.height),
        (412, 915),
        "the device's logical size at 1x"
    );
    let (cx, cy) = (first.width / 2, first.height / 2);
    assert_eq!(
        pixel(&first, cx, cy),
        [0x22, 0x88, 0xFF, 0xFF],
        "the guest's colour, drawn by the child"
    );

    let centre = Offset::new(206.0, 457.0);
    let tapped = remote
        .frame(
            &env,
            1.0,
            Duration::from_millis(16),
            Color::BLACK,
            &[
                RemoteEvent::Pointer(Pointer::Down(centre)),
                RemoteEvent::Pointer(Pointer::Up(centre)),
            ],
        )
        .expect("a frame after the tap");
    assert_eq!(
        pixel(&tapped, cx, cy),
        [0xFF, 0x22, 0x44, 0xFF],
        "the tap reached the guest and rebuilt it"
    );

    // Scale: physical pixels for a 2x window.
    let crisp = remote
        .frame(&env, 2.0, Duration::from_millis(32), Color::BLACK, &[])
        .expect("a 2x frame");
    assert_eq!((crisp.width, crisp.height), (824, 1830));
    // And the environment changed under it without losing the tap's state.
    let dark = Environment { dark: true, ..env };
    let after = remote
        .frame(&dark, 1.0, Duration::from_millis(48), Color::BLACK, &[])
        .expect("a frame in dark mode");
    assert_eq!(
        pixel(&after, cx, cy),
        [0xFF, 0x22, 0x44, 0xFF],
        "guest state survives an environment change"
    );
}

#[test]
fn a_guest_that_aborts_takes_its_child_and_not_the_studio() {
    let Some(toolchain) = toolchain() else { return };
    // `abort` on the build after a tap. No unwinder anywhere catches this.
    let library = compiled(
        &toolchain,
        0x7E72,
        &format!(
            "{}{}",
            include_str!("remote/guest_prelude.rs"),
            r#"
#[derive(Debug)]
pub struct Screen;
impl Widget for Screen {
    fn debug_name(&self) -> &'static str { "Screen" }
    fn kind(&self) -> WidgetKind<'_> { WidgetKind::Composed }
    fn create_state(&self) -> Option<Box<dyn ElementState>> { Some(Box::new(Tapped::default())) }
    fn build(&self, ctx: &BuildContext) -> WidgetNode {
        if tapped(ctx) {
            eprintln!("the guest is about to abort");
            std::process::abort();
        }
        GestureDetector::new().on_tap(on_tap(ctx)).child(Container::new().color(Color::hex(0x22_88FF))).into()
    }
}
pub fn screen() -> impl Widget { Screen }
"#
        ),
    );
    let mut remote = loaded(&toolchain, &library);
    let env = Environment::of(Platform::Ios);
    remote
        .frame(&env, 1.0, Duration::ZERO, Color::BLACK, &[])
        .expect("the first frame draws");
    let c = Offset::new(196.0, 426.0);
    let error = remote
        .frame(
            &env,
            1.0,
            Duration::from_millis(16),
            Color::BLACK,
            &[
                RemoteEvent::Pointer(Pointer::Down(c)),
                RemoteEvent::Pointer(Pointer::Up(c)),
            ],
        )
        .expect_err("the child died");
    let RemoteError::Crashed { stderr, .. } = &error else {
        panic!("expected a crash, got {error:?}");
    };
    assert!(
        stderr.contains("the guest is about to abort"),
        "the guest's last words come back: {stderr:?}"
    );
    // A dead preview stays dead, and says the same thing each time.
    assert_eq!(
        remote
            .frame(&env, 1.0, Duration::ZERO, Color::BLACK, &[])
            .unwrap_err(),
        error
    );

    // This process is alive to read all that — and a fresh child works.
    let good = compiled(&toolchain, 0x7E73, TAPPABLE);
    let mut fresh = loaded(&toolchain, &good);
    let frame = fresh
        .frame(&env, 1.0, Duration::ZERO, Color::BLACK, &[])
        .expect("a new child draws");
    assert_eq!(
        pixel(&frame, frame.width / 2, frame.height / 2),
        [0x22, 0x88, 0xFF, 0xFF]
    );
}

#[test]
fn a_panic_on_a_later_rebuild_is_survived_on_every_platform() {
    let Some(toolchain) = toolchain() else { return };
    let library = compiled(
        &toolchain,
        0x7E74,
        &format!(
            "{}{}",
            include_str!("remote/guest_prelude.rs"),
            r#"
#[derive(Debug)]
pub struct Screen;
impl Widget for Screen {
    fn debug_name(&self) -> &'static str { "Screen" }
    fn kind(&self) -> WidgetKind<'_> { WidgetKind::Composed }
    fn create_state(&self) -> Option<Box<dyn ElementState>> { Some(Box::new(Tapped::default())) }
    fn build(&self, ctx: &BuildContext) -> WidgetNode {
        if tapped(ctx) { panic!("a panic on a later rebuild"); }
        GestureDetector::new().on_tap(on_tap(ctx)).child(Container::new().color(Color::hex(0x22_88FF))).into()
    }
}
pub fn screen() -> impl Widget { Screen }
"#
        ),
    );
    let mut remote = loaded(&toolchain, &library);
    let env = Environment::of(Platform::Ios);
    remote
        .frame(&env, 1.0, Duration::ZERO, Color::BLACK, &[])
        .expect("first frame");
    let c = Offset::new(196.0, 426.0);
    let result = remote.frame(
        &env,
        1.0,
        Duration::from_millis(16),
        Color::BLACK,
        &[
            RemoteEvent::Pointer(Pointer::Down(c)),
            RemoteEvent::Pointer(Pointer::Up(c)),
        ],
    );
    // With one shared libstd (Linux, macOS) the child's element tree catches
    // it and draws the placeholder; without (MSVC) the child dies. Either is
    // survival — the studio is this process, and it is still here.
    match result {
        Ok(frame) => assert_ne!(
            pixel(&frame, 196, 426),
            [0x22, 0x88, 0xFF, 0xFF],
            "the placeholder replaced the screen"
        ),
        Err(RemoteError::Crashed { .. }) => {
            if !cfg!(windows) {
                panic!("only MSVC lacks the shared libstd, so only Windows may crash here");
            }
        }
        Err(other) => panic!("{other:?}"),
    }
}

/// The studio's own Render, with isolation on: the scratch buffer compiles,
/// loads in a child, and the preview pane draws the child's frame.
#[test]
fn the_studio_renders_into_a_child_process_when_isolation_is_on() {
    use viewwstudio::state::{PreviewState, Studio};
    let Some(toolchain) = toolchain() else { return };
    let mut driver = vieww_render::FrameDriver::new(vieww_foundation::Size::new(1400.0, 900.0));
    viewwstudio::install(&mut driver);
    let runtime = driver.elements().runtime().clone();
    let studio = Studio::new(&runtime).with_toolchain(Ok(toolchain), Session::new(0x7E75).ok());
    studio.isolated_preview.set(true);
    studio.render();
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    // `poll_compile` finishes a Render — and on an ABI mismatch starts the
    // next candidate's, which is the studio's own walk — so poll until the
    // state settles.
    loop {
        studio.poll_compile();
        if studio.preview.get() != PreviewState::Compiling {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "the Render finished");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        studio.preview.get(),
        PreviewState::Rendered,
        "{:?}",
        studio.output.get()
    );
    assert!(
        studio.preview_screen.get().is_none(),
        "nothing was dlopened into the studio"
    );
    let remote = studio
        .preview_remote
        .get()
        .expect("the preview runs in a child");
    assert!(remote.borrow_mut().crashed().is_none());

    // The pane draws it: mount the shell, and the RemoteScreen inside it asks
    // the child for a frame during the build.
    driver.set_root(viewwstudio::Shell {
        studio: studio.clone(),
    });
    driver.draw_frame();
    assert!(
        remote.borrow_mut().crashed().is_none(),
        "the child answered the pane"
    );
    assert!(
        remote.borrow_mut().frames() >= 1,
        "the pane's RemoteScreen asked the child for a frame"
    );
    let frame = remote
        .borrow_mut()
        .frame(
            &Environment::of(Platform::Ios),
            1.0,
            Duration::ZERO,
            Color::BLACK,
            &[],
        )
        .expect("the child is still drawing");
    assert!(
        frame
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| *p != [0, 0, 0, 255]),
        "the scratch screen drew something"
    );
}

/// A guest that records what it was told: taps, text typed into a real
/// `TextField`, scrolls, and hover. Every kind of event the pane forwards,
/// held in a state whose snapshot round-trips — so the assertions read the
/// ledger rather than pixels, and the text is the field's own, not a copy
/// the test made.
const INPUT_LEDGER: &str = concat!(
    include_str!("remote/guest_prelude.rs"),
    r#"
use vieww::TextEditingValue;

#[derive(Debug, Default)]
struct Ledger {
    taps: u32,
    /// The field's whole value — text, caret and composing region — because
    /// a controlled field that rebuilds from text alone loses the composing
    /// range between one preedit and the next, and an IME's provisional text
    /// would then never be replaced by its commit. The same contract the
    /// studio's own editor follows, on the guest side of the wire.
    value: TextEditingValue,
    scroll: f32,
    hovered: bool,
    pending: bool,
}
impl ElementState for Ledger {
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
    fn take_pending(&mut self) -> bool { std::mem::take(&mut self.pending) }
    fn snapshot(&self) -> Option<String> {
        Some(format!("{}|{}|{:.2}|{}", self.taps, self.value.text, self.scroll, self.hovered))
    }
    fn restore(&mut self, saved: &str) -> bool {
        let mut parts = saved.split('|');
        self.taps = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        self.value = TextEditingValue::new(parts.next().unwrap_or("").to_owned());
        self.scroll = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        self.hovered = parts.next() == Some("true");
        true
    }
}

fn bump(ctx: &BuildContext, f: impl FnOnce(&mut Ledger)) {
    if let Some(handle) = &ctx.state_handle() {
        if let Ok(mut state) = handle.try_borrow_mut() {
            if let Some(l) = state.as_any_mut().downcast_mut::<Ledger>() {
                f(l);
                l.pending = true;
            }
        }
    }
}

#[derive(Debug)]
pub struct Screen;
impl Widget for Screen {
    fn debug_name(&self) -> &'static str { "Screen" }
    fn kind(&self) -> WidgetKind<'_> { WidgetKind::Composed }
    fn create_state(&self) -> Option<Box<dyn ElementState>> { Some(Box::new(Ledger::default())) }
    fn build(&self, ctx: &BuildContext) -> WidgetNode {
        // One handle per closure — an owned `Rc`, so each is `'static`, the
        // same shape as the prelude's `on_tap`.
        let mut handles = std::iter::repeat_with(|| ctx.state_handle());
        let text_handle = handles.next().unwrap();
        let tap_handle = handles.next().unwrap();
        let scroll_handle = handles.next().unwrap();
        let hover_handle = handles.next().unwrap();
        // The controlled-value round trip: what the field becomes is read
        // back from the ledger, so a rebuild keeps the text rather than
        // resetting it — the same contract the studio's own editor follows.
        let current = ctx
            .state::<Ledger, _>(|l| l.value.clone())
            .unwrap_or_default();
        let field = TextField::new(current)
            .on_changed(std::rc::Rc::new(move |value: TextEditingValue| {
                if let Some(handle) = &text_handle {
                    if let Ok(mut state) = handle.try_borrow_mut() {
                        if let Some(l) = state.as_any_mut().downcast_mut::<Ledger>() {
                            l.value = value;
                            l.pending = true;
                        }
                    }
                }
            }));
        let taps = tap_handle;
        let scrolls = scroll_handle;
        let hovers = hover_handle;
        Flex::column()
            .push(SizedBox::height(72.0).child(field))
            .push(
                Flexible::expanded(1).child(
                GestureDetector::new()
                    .on_tap(move |_| {
                        if let Some(handle) = &taps {
                            if let Ok(mut state) = handle.try_borrow_mut() {
                                if let Some(l) = state.as_any_mut().downcast_mut::<Ledger>() {
                                    l.taps += 1;
                                    l.pending = true;
                                }
                            }
                        }
                    })
                    .on_scroll(move |event: vieww::foundation::ScrollEvent| {
                        if let Some(handle) = &scrolls {
                            if let Ok(mut state) = handle.try_borrow_mut() {
                                if let Some(l) = state.as_any_mut().downcast_mut::<Ledger>() {
                                    l.scroll += event.delta.dy;
                                    l.pending = true;
                                }
                            }
                        }
                    })
                    .on_hover(move |inside: bool| {
                        if let Some(handle) = &hovers {
                            if let Ok(mut state) = handle.try_borrow_mut() {
                                if let Some(l) = state.as_any_mut().downcast_mut::<Ledger>() {
                                    l.hovered = inside;
                                    l.pending = true;
                                }
                            }
                        }
                    })
                    .child(ColoredBox::new(Color::hex(0x22_88FF))),
                ),
            )
            .into()
    }
}
pub fn screen() -> impl Widget { Screen }
"#
);

/// The ledger the child is holding, read through the state-capture request —
/// which is itself under test in the next test, so here it is only a means of
/// reading the guest, and a crash would fail the frame calls anyway.
fn ledger_of(remote: &mut RemotePreview) -> String {
    remote
        .capture_state()
        .expect("the child answers a capture")
        .into_iter()
        .map(|(_, state)| state)
        .find(|state| state.contains('|') && state.split('|').count() == 4)
        .unwrap_or_default()
}

#[test]
fn keys_ime_scroll_and_hover_reach_the_guest() {
    let Some(toolchain) = toolchain() else { return };
    let library = compiled(&toolchain, 0x7E76, INPUT_LEDGER);
    let mut remote = loaded(&toolchain, &library);
    let env = Environment::of(Platform::Android);

    remote
        .frame(&env, 1.0, Duration::ZERO, Color::BLACK, &[])
        .expect("the first frame draws");

    // Tap the field so it takes focus in the child — the same thing a person
    // does before typing.
    let field_tap = Offset::new(206.0, 36.0);
    remote
        .frame(
            &env,
            1.0,
            Duration::from_millis(16),
            Color::BLACK,
            &[
                RemoteEvent::Pointer(Pointer::Down(field_tap)),
                RemoteEvent::Pointer(Pointer::Up(field_tap)),
            ],
        )
        .expect("the tap draws");

    // Type. A key at a time, as a keyboard does — one frame each, the way
    // a real window delivers them.
    let mut after_keys = String::new();
    for (i, c) in "hi".chars().enumerate() {
        let at = Duration::from_millis(32 + i as u64 * 16);
        remote
            .frame(
                &env,
                1.0,
                at,
                Color::BLACK,
                &[RemoteEvent::Key(KeyEvent::character(c.to_string(), at))],
            )
            .expect("the key draws");
        after_keys = ledger_of(&mut remote);
    }
    assert!(
        after_keys.contains("hi"),
        "the keys reached the field: {after_keys:?}"
    );

    // An IME composition, the way an input method actually types: the preedit
    // grows in place — the field's text carries it, underlined — and then a
    // commit replaces it. Both halves have to cross the wire.
    for (i, (text, cursor)) in [("ね", (0, 3)), ("ねこ", (0, 6))].iter().enumerate() {
        let at = Duration::from_millis(48 + i as u64 * 16);
        remote
            .frame(
                &env,
                1.0,
                at,
                Color::BLACK,
                &[RemoteEvent::Ime(ImeEvent::Preedit {
                    text: (*text).to_owned(),
                    cursor: Some(*cursor),
                })],
            )
            .expect("the preedit draws");
    }
    let composing = ledger_of(&mut remote);
    assert!(
        composing.contains("ねこ"),
        "the composition reached the field: {composing:?}"
    );

    // The commit replaces the composition: "hi" + "猫", not "hiねこ猫".
    remote
        .frame(
            &env,
            1.0,
            Duration::from_millis(80),
            Color::BLACK,
            &[RemoteEvent::Ime(ImeEvent::commit("猫"))],
        )
        .expect("the commit draws");
    let after_ime = ledger_of(&mut remote);
    assert!(
        after_ime.contains("hi猫"),
        "the IME commit reached the field and replaced the preedit: {after_ime:?}"
    );
    assert!(
        !after_ime.contains("ねこ"),
        "the committed text left no preedit behind: {after_ime:?}"
    );

    // A scroll in the lower half, where the detector is.
    remote
        .frame(
            &env,
            1.0,
            Duration::from_millis(96),
            Color::BLACK,
            &[RemoteEvent::Scroll(ScrollEvent::new(
                Offset::new(206.0, 500.0),
                Offset::new(0.0, -120.0),
                Duration::from_millis(96),
            ))],
        )
        .expect("the scroll draws");
    assert!(
        ledger_of(&mut remote).contains("-120.00"),
        "the scroll reached the guest"
    );

    // Hover, at the detector and away from it.
    remote
        .frame(
            &env,
            1.0,
            Duration::from_millis(112),
            Color::BLACK,
            &[
                RemoteEvent::Hover(Some(Offset::new(206.0, 500.0))),
                RemoteEvent::Hover(Some(Offset::new(3.0, 900.0))),
                RemoteEvent::Hover(None),
            ],
        )
        .expect("the hover draws");
    let after_hover = ledger_of(&mut remote);
    assert!(
        after_hover.ends_with("|false"),
        "hover came and went: {after_hover:?}"
    );
}

#[test]
fn the_guest_state_crosses_a_re_render_through_the_child() {
    let Some(toolchain) = toolchain() else { return };
    let library = compiled(&toolchain, 0x7E77, INPUT_LEDGER);
    let mut remote = loaded(&toolchain, &library);
    let env = Environment::of(Platform::Android);

    remote
        .frame(&env, 1.0, Duration::ZERO, Color::BLACK, &[])
        .expect("the tree exists");

    // Interact: tap the field, type, tap the detector twice.
    let field_tap = Offset::new(206.0, 36.0);
    let at = Duration::from_millis(16);
    remote
        .frame(
            &env,
            1.0,
            at,
            Color::BLACK,
            &[
                RemoteEvent::Pointer(Pointer::Down(field_tap)),
                RemoteEvent::Pointer(Pointer::Up(field_tap)),
            ],
        )
        .expect("focus");
    for (i, c) in "ok".chars().enumerate() {
        let kat = Duration::from_millis(32 + i as u64 * 16);
        remote
            .frame(
                &env,
                1.0,
                kat,
                Color::BLACK,
                &[RemoteEvent::Key(KeyEvent::character(c.to_string(), kat))],
            )
            .expect("typed");
    }
    let body_tap = Offset::new(206.0, 500.0);
    for _ in 0..2 {
        remote
            .frame(
                &env,
                1.0,
                at,
                Color::BLACK,
                &[
                    RemoteEvent::Pointer(Pointer::Down(body_tap)),
                    RemoteEvent::Pointer(Pointer::Up(body_tap)),
                ],
            )
            .expect("tapped");
    }

    // The capture: the child walks its own tree.
    let snapshot = remote.capture_state().expect("the capture answers");
    assert!(
        snapshot
            .iter()
            .any(|(_, state)| state.contains("ok") && state.starts_with("2|")),
        "the snapshot holds the guest's own numbers: {snapshot:?}"
    );

    // A fresh child, the way a Render makes one: the state goes back in.
    let mut fresh = loaded(&toolchain, &library);
    let restored = fresh
        .restore_state(&snapshot, Duration::from_millis(0))
        .expect("the restore answers");
    assert!(restored >= 1, "at least the ledger came back");
    let back = ledger_of(&mut fresh);
    assert!(
        back.contains("ok") && back.starts_with("2|"),
        "the state survived the process boundary: {back:?}"
    );
    fresh
        .frame(&env, 1.0, Duration::from_millis(16), Color::BLACK, &[])
        .expect("the fresh child draws");
}

// ── ST-04: Say screens over the wire ────────────────────────────────────────

/// The counter, as the repository ships it: the one screen whose `keep`
/// state a tap changes, which is what makes it the honest fixture for a
/// state-capture test — the number has to come back *and* still work.
const COUNTER_SAY: &str =
    include_str!("../../../crates/vieww-say-codegen/tests/fixtures/counter.say");

/// A Say list long enough to have somewhere to scroll to: twenty padded
/// cards in one scroll view, on a 412×915 device.
const LONG_LIST_SAY: &str = "-- say-language: 1
-- long enough that the scroll view has somewhere to go.

keep a list of text called names starting at \"n01\", \"n02\", \"n03\", \"n04\", \"n05\", \"n06\", \"n07\", \"n08\", \"n09\", \"n10\", \"n11\", \"n12\", \"n13\", \"n14\", \"n15\", \"n16\", \"n17\", \"n18\", \"n19\", \"n20\"

screen \"Names\":
    a scroll view:
        for each name in names:
            a card, padded 24:
                a text \"\\(name)\"
";

/// A `.say` fixture compiled to its generated Rust, ready for `rustc` — the
/// same hop `Studio::render` makes for a Say buffer.
fn say_rust(file: &str, source: &str) -> String {
    vieww_say_codegen::compile(file, source)
        .unwrap_or_else(|diagnostics| panic!("{file} did not compile: {diagnostics:?}"))
        .rust
}

/// Where the counter's `Add one` button is: the column starts under the safe
/// area, and the button is the third child — heading, label, button — so its
/// centre is a computed position, not a magic one.
fn counter_button() -> Offset {
    let safe_top = 30.0;
    let heading = 24.0 * 1.2;
    let body = 15.0 * 1.45;
    let spacing = 16.0;
    let button_height = 48.0;
    Offset::new(
        40.0,
        safe_top + heading + spacing + body + spacing + button_height / 2.0,
    )
}

#[test]
fn a_say_list_scrolls_over_the_wire() {
    let Some(toolchain) = toolchain() else { return };
    let library = compiled(&toolchain, 0x7E78, &say_rust("names.say", LONG_LIST_SAY));
    let mut remote = loaded(&toolchain, &library);
    let env = Environment::of(Platform::Android);

    let before = remote
        .frame(&env, 1.0, Duration::ZERO, Color::WHITE, &[])
        .expect("the list draws");

    // A wheel notch in the middle of the list, in the content-moves
    // convention the wire documents: −200 dy shows later items.
    let at = Duration::from_millis(16);
    let after = remote
        .frame(
            &env,
            1.0,
            at,
            Color::WHITE,
            &[RemoteEvent::Scroll(ScrollEvent::new(
                Offset::new(206.0, 457.0),
                Offset::new(0.0, -200.0),
                at,
            ))],
        )
        .expect("the scroll draws");

    // The whole picture is the list, and it moved.
    assert!(
        before.rgba != after.rgba,
        "scrolling a real list changed the frame"
    );
    // And by exactly the delta: what is at y now was at y+200 before — a
    // scroll of a list, not "something repainted".
    for x in [40, 206, 370] {
        for y in [240, 400, 560] {
            assert_eq!(
                pixel(&after, x, y),
                pixel(&before, x, y + 200),
                "the list moved up by exactly the scroll delta at ({x}, {y})"
            );
        }
    }
}

#[test]
fn say_keep_state_survives_a_remote_re_render() {
    let Some(toolchain) = toolchain() else { return };
    let library = compiled(&toolchain, 0x7E79, &say_rust("counter.say", COUNTER_SAY));
    let mut remote = loaded(&toolchain, &library);
    let env = Environment::of(Platform::Android);

    remote
        .frame(&env, 1.0, Duration::ZERO, Color::WHITE, &[])
        .expect("the screen draws");

    // Two taps on `Add one`: the `keep a whole number called count` starts
    // at 0 and the button's handler adds to it.
    let button = counter_button();
    for i in 0..2u64 {
        let at = Duration::from_millis(16 + i * 16);
        remote
            .frame(
                &env,
                1.0,
                at,
                Color::WHITE,
                &[
                    RemoteEvent::Pointer(Pointer::Down(button)),
                    RemoteEvent::Pointer(Pointer::Up(button)),
                ],
            )
            .expect("the tap draws");
    }
    let snapshot = remote.capture_state().expect("the capture answers");
    assert!(
        snapshot.iter().any(|(_, state)| state.contains("count=2")),
        "two taps, two counts: {snapshot:?}"
    );

    // A fresh child — the process a Render makes — and the state goes back in.
    let mut fresh = loaded(&toolchain, &library);
    let restored = fresh
        .restore_state(&snapshot, Duration::from_millis(0))
        .expect("the restore answers");
    assert!(restored >= 1, "at least the Say state came back");
    let back = fresh.capture_state().expect("the read-back answers");
    assert!(
        back.iter().any(|(_, state)| state.contains("count=2")),
        "the kept value survived the process boundary: {back:?}"
    );

    // And it came back *live*, not as a string: one more tap makes three,
    // through the same handler the first two went through.
    fresh
        .frame(
            &env,
            1.0,
            Duration::from_millis(16),
            Color::WHITE,
            &[
                RemoteEvent::Pointer(Pointer::Down(button)),
                RemoteEvent::Pointer(Pointer::Up(button)),
            ],
        )
        .expect("the restored screen still takes a tap");
    let live = fresh.capture_state().expect("the second read-back answers");
    assert!(
        live.iter().any(|(_, state)| state.contains("count=3")),
        "the restored state drives the guest: {live:?}"
    );
}
