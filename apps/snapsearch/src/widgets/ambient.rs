//! A free-running ambient clock that lives on one element.
//!
//! The library screen used to read a shared `tick` signal in its own `build`,
//! so every 140 ms the *whole screen* — header, every tile, the sheet, the
//! FAB — was rebuilt to move a halo and a few tiles. [`Ambient`] moves the
//! clock down to the thing that uses it: the element ticks itself in the
//! frame's animate phase and rebuilds only its own subtree.
//!
//! It stops asking for frames the moment it is disabled (reduce motion, a
//! low quality tier, a modal on top), so a still screen is a still screen.

use std::any::Any;
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use vieww::prelude::*;
use vieww::widget::{ElementState, WidgetKind};
use vieww::foundation::Key;

/// One ambient step. Matches the cadence the shared clock used.
pub const STEP: Duration = Duration::from_millis(140);

/// Builds a subtree from the current ambient step.
pub type AmbientBuilder = Rc<dyn Fn(u64) -> WidgetNode>;

#[derive(Clone)]
pub struct Ambient {
    enabled: bool,
    /// Added to the element's own step count, so neighbours that mount on
    /// the same frame do not all move in lock-step.
    phase: u64,
    builder: AmbientBuilder,
    key: Option<Key>,
}

impl Ambient {
    /// `enabled == false` builds once at step `phase` and never ticks.
    #[must_use]
    pub fn new(enabled: bool, build: impl Fn(u64) -> WidgetNode + 'static) -> Self {
        Self { enabled, phase: 0, builder: Rc::new(build), key: None }
    }

    /// Identity across rebuilds. A list that adds and removes items around
    /// the ones it keeps (a virtualised grid) needs this, or an element's
    /// clock and state would be handed to a different item.
    #[must_use]
    pub fn key(mut self, key: impl Into<Key>) -> Self {
        self.key = Some(key.into());
        self
    }

    #[must_use]
    pub const fn phase(mut self, phase: u64) -> Self {
        self.phase = phase;
        self
    }
}

impl fmt::Debug for Ambient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ambient").field("enabled", &self.enabled).finish_non_exhaustive()
    }
}

impl Widget for Ambient {
    fn debug_name(&self) -> &'static str {
        "Ambient"
    }

    fn kind(&self) -> WidgetKind<'_> {
        WidgetKind::Composed
    }

    fn key(&self) -> Option<&Key> {
        self.key.as_ref()
    }

    fn create_state(&self) -> Option<Box<dyn ElementState>> {
        Some(Box::new(AmbientState { enabled: self.enabled, origin: None, step: 0 }))
    }

    fn build(&self, ctx: &BuildContext) -> WidgetNode {
        let step = ctx.state(|s: &AmbientState| s.step).unwrap_or(0);
        (self.builder)(step.wrapping_add(self.phase))
    }
}

#[derive(Debug)]
pub struct AmbientState {
    enabled: bool,
    origin: Option<Duration>,
    step: u64,
}

impl AmbientState {
    /// The step at `now`, given where this element's clock started.
    fn step_at(origin: Duration, now: Duration) -> u64 {
        (now.saturating_sub(origin).as_millis() / STEP.as_millis()) as u64
    }
}

impl ElementState for AmbientState {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn widget_updated(&mut self, widget: &WidgetNode) {
        if let Some(w) = widget.downcast_ref::<Ambient>() {
            self.enabled = w.enabled;
        }
    }

    fn tick(&mut self, now: Duration) -> bool {
        if !self.enabled {
            return false;
        }
        let origin = *self.origin.get_or_insert(now);
        let step = Self::step_at(origin, now);
        if step == self.step {
            return false;
        }
        self.step = step;
        true
    }

    fn is_animating(&self) -> bool {
        self.enabled
    }
}

widget_node_from!(Ambient);

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn state(enabled: bool) -> AmbientState {
        AmbientState { enabled, origin: None, step: 0 }
    }

    #[test]
    fn it_rebuilds_once_per_step_not_once_per_frame() {
        let mut s = state(true);
        assert!(!s.tick(ms(1000)), "the first frame only sets the origin");
        let rebuilds = (1..=140).filter(|f| s.tick(ms(1000 + f * 16))).count();
        // 140 frames of 16 ms is 2.24 s: sixteen 140 ms steps, not 140 rebuilds.
        assert_eq!(rebuilds, 16);
    }

    #[test]
    fn a_disabled_ambient_neither_ticks_nor_asks_for_frames() {
        let mut s = state(false);
        assert!(!s.tick(ms(0)));
        assert!(!s.tick(ms(10_000)));
        assert!(!s.is_animating());
    }

    #[test]
    fn re_enabling_resumes_without_a_burst_of_catch_up_rebuilds() {
        let mut s = state(true);
        s.tick(ms(0));
        s.tick(ms(140));
        s.enabled = false;
        assert!(!s.tick(ms(5000)));
        s.enabled = true;
        // One rebuild to land on the current step, not one per missed step.
        assert!(s.tick(ms(5000)));
        assert!(!s.tick(ms(5010)));
    }
}
