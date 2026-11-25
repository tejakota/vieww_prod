//! A11Y-02: on the Plan screen a keyboard alone reaches the tier segments,
//! Enter commits one (the host buzzes — the observable), and a focus ring is
//! drawn while the keyboard drives.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

use vavlt_app::model::Screen;
use vavlt_app::{Message, Platform, Scrolls, VavltApp, VavltState};
use vavlt_engine::{Item, Source};
use vieww::foundation::task::FrameWaker;
use vieww::foundation::{Color, KeyEvent, LogicalKey, NamedKey, Size};
use vieww::paint::native::NativeRenderer;
use vieww::FrameDriver;

const PHONE: Size = Size { width: 412.0, height: 915.0 };

#[derive(Debug)]
struct Host {
    dir: PathBuf,
    buzzes: Rc<Cell<u32>>,
}

#[derive(Debug)]
struct Nothing;
impl Source for Nothing {
    fn read(&self, _h: &str) -> anyhow::Result<Vec<u8>> {
        anyhow::bail!("no files")
    }
}

impl Platform for Host {
    fn data_dir(&self) -> PathBuf {
        self.dir.clone()
    }
    fn source(&self) -> Arc<dyn Source> {
        Arc::new(Nothing)
    }
    fn pick(&self, _m: usize, _r: Sender<Message>, _w: Arc<dyn FrameWaker>) {}
    fn spawn(&self, work: Box<dyn FnOnce() + Send + 'static>) {
        work();
    }
    fn haptic(&self) {
        self.buzzes.set(self.buzzes.get() + 1);
    }
}

#[derive(Debug)]
struct Still;
impl FrameWaker for Still {
    fn wake(&self) {}
}

fn key(driver: &mut FrameDriver, k: NamedKey) {
    driver.handle_key(&KeyEvent::down(LogicalKey::Named(k), Duration::ZERO));
    driver.draw_frame();
    driver.draw_frame();
}

fn pixels(r: &mut NativeRenderer, d: &FrameDriver) -> Vec<u8> {
    r.render_to_pixels(d.scene(), PHONE.width as u32, PHONE.height as u32, Color::hex(0x24_2424))
        .expect("renders")
        .0
        .data()
        .to_vec()
}

#[test]
fn tab_reaches_a_tier_segment_enter_commits_it_and_a_ring_shows() {
    let mut driver = FrameDriver::new(PHONE);
    let runtime = driver.elements().runtime().clone();
    let buzzes = Rc::new(Cell::new(0));
    let dir = std::env::temp_dir().join("vavlt-keyboard");
    std::fs::create_dir_all(&dir).ok();
    let state = VavltState::new(&runtime, Rc::new(Host { dir, buzzes: buzzes.clone() }), Arc::new(Still));
    state.deliver(Message::Picked(
        (0..9)
            .map(|i| Item::new(format!("h{i}"), format!("IMG_{:04}.jpg", 1200 + i), 4_000_000))
            .collect(),
    ));
    state.go(Screen::Plan);
    let scrolls = Rc::new(Scrolls::new(&runtime));
    scrolls.attach(driver.tickers());
    driver.set_root(VavltApp::new(state.clone(), scrolls));
    // Let the screen transition and the entrance animations finish.
    for ms in (0..1200).step_by(50) {
        driver.draw_frame_at(Duration::from_millis(ms));
    }
    let mut renderer = NativeRenderer::new();
    let tier_before = state.tier.peek();

    let baseline = pixels(&mut renderer, &driver);
    let mut ring_seen = false;
    for _ in 0..12 {
        key(&mut driver, NamedKey::Tab);
        let now = pixels(&mut renderer, &driver);
        ring_seen |= now != baseline;
        key(&mut driver, NamedKey::Enter);
        if buzzes.get() > 0 {
            break;
        }
    }
    assert!(ring_seen, "the keyboard focus ring is drawn on some control");
    assert!(buzzes.get() > 0, "Tab + Enter reached a tier segment and committed it");
    assert_ne!(state.tier.peek(), tier_before, "the tier changed");
}
