//! UX-02: committing a different tier buzzes the host once; re-picking the
//! tier already chosen does not.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::sync::Arc;

use vavlt_app::{Message, Platform, VavltState};
use vavlt_core::Tier;
use vavlt_engine::Source;
use vieww::foundation::task::FrameWaker;
use vieww::FrameDriver;

#[derive(Debug)]
struct Buzzer {
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

impl Platform for Buzzer {
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

#[test]
fn a_tier_change_buzzes_once_and_a_repeat_does_not() {
    let mut driver = FrameDriver::new(vieww::foundation::Size::new(400.0, 800.0));
    let runtime = driver.elements().runtime().clone();
    let buzzes = Rc::new(Cell::new(0));
    let dir = std::env::temp_dir().join("vavlt-haptic");
    std::fs::create_dir_all(&dir).ok();
    let state = VavltState::new(
        &runtime,
        Rc::new(Buzzer { dir, buzzes: buzzes.clone() }),
        Arc::new(Still),
    );
    let start = if state.tier.peek() == Tier::DeepMove { Tier::Move } else { Tier::DeepMove };
    state.set_tier(start);
    assert_eq!(buzzes.get(), 1);
    state.set_tier(start);
    assert_eq!(buzzes.get(), 1, "same tier again: no buzz");
}
