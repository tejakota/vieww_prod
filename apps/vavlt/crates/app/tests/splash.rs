//! VA-01: the cold-start splash ends on a tap or on any key, and is never in
//! the way under reduce motion.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

use vavlt_app::{Message, Platform, Scrolls, VavltApp, VavltState};
use vavlt_engine::Source;
use vieww::foundation::task::FrameWaker;
use vieww::foundation::{KeyEvent, LogicalKey, NamedKey, Offset, PointerEvent, PointerId, Size};
use vieww::prelude::*;
use vieww::FrameDriver;

const PHONE: Size = Size { width: 412.0, height: 915.0 };

#[derive(Debug)]
struct Host(PathBuf);

#[derive(Debug)]
struct Nothing;
impl Source for Nothing {
    fn read(&self, _h: &str) -> anyhow::Result<Vec<u8>> {
        anyhow::bail!("no files")
    }
}

impl Platform for Host {
    fn data_dir(&self) -> PathBuf {
        self.0.clone()
    }
    fn source(&self) -> Arc<dyn Source> {
        Arc::new(Nothing)
    }
    fn pick(&self, _m: usize, _r: Sender<Message>, _w: Arc<dyn FrameWaker>) {}
    fn spawn(&self, work: Box<dyn FnOnce() + Send + 'static>) {
        work();
    }
}

#[derive(Debug)]
struct Still;
impl FrameWaker for Still {
    fn wake(&self) {}
}

/// The app mounted as a host mounts it on a cold start: splash running.
fn with_splash() -> (FrameDriver, vieww::element::Animation<f32>) {
    let mut driver = FrameDriver::new(PHONE);
    let runtime = driver.elements().runtime().clone();
    let dir = std::env::temp_dir().join("vavlt-splash");
    std::fs::create_dir_all(&dir).ok();
    let state = VavltState::new(&runtime, Rc::new(Host(dir)), Arc::new(Still));
    let scrolls = Rc::new(Scrolls::new(&runtime));
    scrolls.attach(driver.tickers());
    let splash = driver
        .animation(Tween::new(0.0_f32, 1.0_f32), vavlt_app::SPLASH_DURATION)
        .curve(Curve::EASE_IN_OUT);
    splash.forward(Duration::ZERO);
    splash.attach(driver.tickers());
    driver.set_root(VavltApp::new_with_splash(state, scrolls, splash.clone()));
    driver.draw_frame_at(Duration::from_millis(16));
    driver.draw_frame_at(Duration::from_millis(32));
    (driver, splash)
}

#[test]
fn any_key_ends_the_splash() {
    let (mut driver, splash) = with_splash();
    assert!(splash.value() < 0.5, "the splash is still playing");
    // Not Enter or Space, which a focused button would also take: a key no
    // control on the home screen uses.
    assert!(driver.handle_key(&KeyEvent::down(LogicalKey::Named(NamedKey::Escape), Duration::ZERO)));
    driver.draw_frame_at(Duration::from_millis(48));
    assert!(splash.value() >= 0.999, "a key skips the splash: {}", splash.value());
}

#[test]
fn a_tap_ends_the_splash() {
    let (mut driver, splash) = with_splash();
    let at = Offset::new(PHONE.width / 2.0, PHONE.height / 2.0);
    driver.handle_pointer(&PointerEvent::down(PointerId(1), at, Duration::from_millis(40)));
    driver.handle_pointer(&PointerEvent::up(PointerId(1), at, Duration::from_millis(60)));
    driver.draw_frame_at(Duration::from_millis(80));
    assert!(splash.value() >= 0.999, "a tap skips the splash: {}", splash.value());
}
