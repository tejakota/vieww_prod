//! The Linux tray thread's wait discipline, on its own so it can be tested
//! without GTK.
//!
//! # Why a tray thread needs a stated policy at all
//!
//! The GTK main loop the tray lives on is a polling loop: `gtk::events_pending`
//! plus a sleep, because there is no way to integrate an `mpsc` receiver with
//! GTK's blocking wait. The first version of that loop ran the poll
//! **unconditionally** — 62.5 wakeups per second, from the moment the service
//! was registered, whether or not the application ever put an icon in the
//! status area. On an idle desktop that is a real, measured defect class
//! (FW-15: 3.4% CPU at zero frames presented, "unexplained wakeups"), and the
//! whole of this module is the difference between that loop and this one:
//!
//! - **no icon** — block on the channel. Nothing is visible, so nothing GTK
//!   could do matters, and an application that registered the service and
//!   never showed an icon costs **zero** wakeups. This is also why the thread
//!   itself is not started until the first order arrives (see `desktop.rs`).
//! - **icon present** — poll, at the latency a menu click deserves. The poll
//!   is the price of dispatching a click without a GTK main loop of our own;
//!   it is paid only while the application is actually showing something.
//!
//! [`TrayWait`] is the decision, [`tray_wait`] is the rule, and the unit tests
//! in this file are what keeps the "block when there is nothing to show" half
//! from quietly reverting to a poll — the defect class this exists for is one
//! that no GTK test would catch, because on a machine with a display the
//! polling loop *works*, which is exactly how it survived.

use std::time::Duration;

/// How long a tray thread that owns an icon waits between GTK iterations.
///
/// 16 ms ≈ a 60 Hz display's frame: menu-latency-smooth, and a bound that
/// means something to a person, rather than a number tuned to a benchmark.
const ICON_POLL: Duration = Duration::from_millis(16);

/// What the tray thread does between orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrayWait {
    /// Block on the channel — no wakeups at all until an order arrives.
    Block,
    /// Sleep for this long, then iterate GTK and drain orders again.
    Poll(Duration),
}

/// The wait discipline for a tray thread whose icon state is `icon_present`.
#[must_use]
pub(crate) fn tray_wait(icon_present: bool) -> TrayWait {
    if icon_present {
        TrayWait::Poll(ICON_POLL)
    } else {
        TrayWait::Block
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point (FW-15): with nothing shown, the thread blocks rather
    /// than polls. A revert to unconditional polling is exactly the
    /// "unexplained wakeups" row in the idle budget.
    #[test]
    fn no_icon_means_block_not_poll() {
        assert_eq!(tray_wait(false), TrayWait::Block);
    }

    /// An icon is live UI: it is polled, so a menu click is dispatched at
    /// display latency rather than whenever the next order happens to arrive.
    #[test]
    fn an_icon_is_polled_at_menu_latency() {
        assert_eq!(tray_wait(true), TrayWait::Poll(Duration::from_millis(16)));
    }

    /// The two answers are different, so a caller that merged them (a `Poll`
    /// with a very long sleep is a block that lies) would show up here.
    #[test]
    fn the_two_answers_are_not_the_same_wait() {
        assert_ne!(tray_wait(false), tray_wait(true));
    }
}
