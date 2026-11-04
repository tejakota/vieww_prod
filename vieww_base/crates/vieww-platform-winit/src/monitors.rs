//! The displays the machine has, and what changes about them.
//!
//! # Why this is here and not in `vieww-foundation`
//!
//! The same rule `crate::lifecycle` states: `winit` is the *single* bridge
//! for all five targets, so a trait in a core crate would have exactly one
//! implementor and no second opinion to design against. The moment something
//! above the platform layer needs to observe displays — a layout that
//! re-flows per-monitor, say — this moves down and this crate reports into
//! it.
//!
//! # What the OS actually reports
//!
//! `winit` exposes enumeration (`available_monitors`, `primary_monitor`) and,
//! for the *window*, a `ScaleFactorChanged` event. What it does **not**
//! expose on any backend this version pins is a monitor-hotplug event —
//! plugging a display in is reported by the window moving or rescaling, or
//! not at all until something asks again. So this module is built as a
//! snapshot plus a diff: [`snapshot`] reads what the OS says *now*, and
//! [`diff`] turns two snapshots into the events an application cares about.
//! The runner re-reads on every event that could mean the furniture moved
//! (`Moved`, `ScaleFactorChanged`, and once at `resumed` for the baseline),
//! which is the same re-read discipline `crate::app`'s refresh-rate
//! tracking already practices — see that code's own comment for why a
//! once-a-second poll was deliberately *not* added.
//!
//! # Identity: a monitor is its name, or its geometry
//!
//! On X11 and Windows a monitor carries a stable name; on Wayland the
//! compositor hands out no name at all (`MonitorHandle::name` answers
//! `None`), and two identical panels would be indistinguishable by anything
//! but where they sit. So the match key is the name when there is one and
//! `WxH+X+Y` when there is not: a nameless monitor that *moves* reads as
//! removed-and-added rather than moved, which is the honest limit of what
//! the platform said — a "move" between two nameless monitors and a swap of
//! two nameless monitors produce byte-identical snapshots, and no diff can
//! tell those apart.
//!
//! # What is deliberately not an event
//!
//! Refresh-rate changes and geometry changes within one monitor are not
//! reported: the report's FW-19 list asks for enumeration plus DPI/scale
//! events, nothing between those has a consumer yet, and an event surface
//! nobody reads is the fastest way to breed untested code. Adding one is a
//! new arm in [`MonitorChange`] and a match arm in [`diff`] — the shape is
//! ready for it.

use std::fmt;

use winit::event_loop::ActiveEventLoop;
use winit::monitor::MonitorHandle;

/// One display, as the platform describes it — everything in **physical**
/// pixels and raw scale factors, because logical pixels are a *window*
/// concern (`crate::scale`'s whole argument) and a monitor is not a window.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorInfo {
    /// The platform's name for this display, or `None` on Wayland where the
    /// compositor gives none — see the module doc for what identity means
    /// there.
    pub name: Option<String>,
    /// The display's origin in the global desktop coordinate space.
    pub origin: (i32, i32),
    /// The display's size in physical pixels.
    pub physical_size: (u32, u32),
    /// How many physical pixels one logical pixel is — the per-monitor DPI
    /// signal, which is 2 on a retina panel and 1 on most desktops.
    pub scale_factor: f64,
    /// The display's refresh rate in millihertz, when the platform reports
    /// one (`Some(60_000)` for 60 Hz). Wayland reports it; X11 often cannot.
    pub refresh_millihertz: Option<u32>,
    /// Whether this is the primary display.
    pub primary: bool,
}

impl MonitorInfo {
    /// The key [`diff`] matches monitors by — the name when the platform
    /// gave one, the geometry when it did not.
    fn identity(&self) -> String {
        match &self.name {
            Some(name) => name.clone(),
            None => format!(
                "{}x{}+{}+{}",
                self.physical_size.0, self.physical_size.1, self.origin.0, self.origin.1
            ),
        }
    }
}

/// What changed between two snapshots of the machine's displays.
///
/// Deliberately small: added, removed, and the DPI signal. A scale change is
/// reported as *itself* rather than remove-plus-add because it is the one
/// change an application can react to without re-flowing everything: the
/// same content at a different density wants a re-raster, not a re-layout.
#[derive(Debug, Clone, PartialEq)]
pub enum MonitorChange {
    /// A display the previous snapshot did not have — plugged in, or the
    /// first snapshot an application ever sees (which is how enumeration is
    /// delivered through [`App::on_monitor_change`](crate::App::on_monitor_change)).
    Added(MonitorInfo),
    /// A display that went away. Carries the identity it was matched by, not
    /// the whole record: what an application needs to know about a display
    /// that no longer exists is *which one* it was.
    Removed { identity: String },
    /// A display's scale factor changed — the per-monitor DPI event. Carries
    /// both sides because "which direction" is the first question a caller
    /// asks and re-reading the snapshot to answer it is a race.
    ScaleChanged {
        identity: String,
        from: f64,
        to: f64,
    },
}

impl fmt::Display for MonitorChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Added(info) => write!(
                f,
                "monitor added: {} ({}x{} at +{}+{}, scale {})",
                info.name.as_deref().unwrap_or("(unnamed)"),
                info.physical_size.0,
                info.physical_size.1,
                info.origin.0,
                info.origin.1,
                info.scale_factor
            ),
            Self::Removed { identity } => write!(f, "monitor removed: {identity}"),
            Self::ScaleChanged { identity, from, to } => {
                write!(f, "monitor {identity} rescaled: {from} -> {to}")
            }
        }
    }
}

/// The scale factor a monitor reported, with the platform's absurd answers
/// replaced by the one [`crate::Scale`] would pick — a display with zero or
/// negative pixels-per-logical-pixel does not exist, and forwarding a NaN
/// into application layout turns into a panic several layers from the cause.
fn sanitized_scale(factor: f64) -> f64 {
    if factor.is_finite() && factor > 0.0 {
        factor
    } else {
        1.0
    }
}

/// One `winit` monitor handle as a [`MonitorInfo`]. Pure conversion — no
/// connection, no window, no event loop — so a test on any machine can
/// exercise the sanitisation by going through the same field list.
fn describe(handle: &MonitorHandle, primary: bool) -> MonitorInfo {
    let position = handle.position();
    let size = handle.size();
    MonitorInfo {
        name: handle.name(),
        origin: (position.x, position.y),
        physical_size: (size.width, size.height),
        scale_factor: sanitized_scale(handle.scale_factor()),
        refresh_millihertz: handle.refresh_rate_millihertz(),
        primary,
    }
}

/// Every display the event loop's platform reports right now, primary first.
///
/// Public because a [`winit`] loop is something an application can be driving
/// itself (this crate re-exports enough of winit's surface for that to make
/// sense, `AndroidApp` being the precedent), and enumeration without the
/// events is exactly what a startup screen sizing itself to the desktop wants.
/// Applications running through [`App`](crate::App) get the same facts pushed
/// as [`MonitorChange::Added`] through
/// [`App::on_monitor_change`](crate::App::on_monitor_change) instead — one
/// source of truth, two doors.
#[must_use]
pub fn snapshot(active: &ActiveEventLoop) -> Vec<MonitorInfo> {
    let primary = active.primary_monitor();
    let mut infos: Vec<MonitorInfo> = active
        .available_monitors()
        .map(|monitor| {
            let is_primary = primary.as_ref().is_some_and(|prime| *prime == monitor);
            describe(&monitor, is_primary)
        })
        .collect();
    // Available already yields the primary first on every backend this crate
    // has observed, but that is an implementation detail, not a contract; the
    // sort makes it ours. Stable, so equal keys keep enumeration order.
    infos.sort_by_key(|info| !info.primary);
    infos
}

/// Turn a previous snapshot and the current one into the events between
/// them. Pure — the runner calls it with two [`snapshot`]s, a test calls it
/// with two `Vec`s it wrote by hand.
///
/// A monitor present in both with a different scale is one
/// [`MonitorChange::ScaleChanged`]; the same monitor with a different
/// geometry is remove-plus-add (the identity *is* the geometry for nameless
/// monitors, and a named monitor changing its mode is a different event this
/// surface does not claim — see the module doc).
#[must_use]
pub fn diff(old: &[MonitorInfo], new: &[MonitorInfo]) -> Vec<MonitorChange> {
    let mut changes = Vec::new();
    for incoming in new {
        match old.iter().find(|was| was.identity() == incoming.identity()) {
            None => changes.push(MonitorChange::Added(incoming.clone())),
            Some(was) => {
                if was.scale_factor != incoming.scale_factor {
                    changes.push(MonitorChange::ScaleChanged {
                        identity: incoming.identity(),
                        from: was.scale_factor,
                        to: incoming.scale_factor,
                    });
                }
            }
        }
    }
    for was in old {
        if !new
            .iter()
            .any(|incoming| incoming.identity() == was.identity())
        {
            changes.push(MonitorChange::Removed {
                identity: was.identity(),
            });
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(
        name: Option<&str>,
        origin: (i32, i32),
        size: (u32, u32),
        scale: f64,
    ) -> MonitorInfo {
        MonitorInfo {
            name: name.map(str::to_owned),
            origin,
            physical_size: size,
            scale_factor: scale,
            refresh_millihertz: Some(60_000),
            primary: false,
        }
    }

    #[test]
    fn the_first_snapshot_reports_every_display_as_added() {
        // This is also how enumeration is delivered to an application: the
        // baseline diff against nothing is the machine's display list.
        let displays = vec![
            monitor(Some("eDP-1"), (0, 0), (1920, 1080), 1.0),
            monitor(Some("HDMI-1"), (1920, 0), (3840, 2160), 2.0),
        ];
        assert_eq!(
            diff(&[], &displays),
            vec![
                MonitorChange::Added(displays[0].clone()),
                MonitorChange::Added(displays[1].clone()),
            ]
        );
    }

    #[test]
    fn an_unchanged_snapshot_reports_nothing() {
        let displays = vec![monitor(Some("eDP-1"), (0, 0), (1920, 1080), 1.0)];
        assert_eq!(diff(&displays, &displays), Vec::<MonitorChange>::new());
    }

    #[test]
    fn a_scale_change_is_one_event_with_both_sides_not_a_replacement() {
        let before = vec![monitor(Some("eDP-1"), (0, 0), (1920, 1080), 1.0)];
        let after = vec![monitor(Some("eDP-1"), (0, 0), (1920, 1080), 2.0)];
        assert_eq!(
            diff(&before, &after),
            vec![MonitorChange::ScaleChanged {
                identity: "eDP-1".to_owned(),
                from: 1.0,
                to: 2.0,
            }]
        );
    }

    #[test]
    fn a_removed_display_is_reported_by_its_identity() {
        let before = vec![
            monitor(Some("eDP-1"), (0, 0), (1920, 1080), 1.0),
            monitor(Some("HDMI-1"), (1920, 0), (3840, 2160), 2.0),
        ];
        let after = vec![monitor(Some("eDP-1"), (0, 0), (1920, 1080), 1.0)];
        assert_eq!(
            diff(&before, &after),
            vec![MonitorChange::Removed {
                identity: "HDMI-1".to_owned(),
            }]
        );
        // And a machine that loses every display says so, rather than
        // quietly reporting nothing — a diff against an empty world is an
        // event, not a no-op.
        assert_eq!(
            diff(&before, &[]),
            vec![
                MonitorChange::Removed {
                    identity: "eDP-1".to_owned(),
                },
                MonitorChange::Removed {
                    identity: "HDMI-1".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn an_added_and_removed_display_both_appear_in_one_diff() {
        // The docked-and-undocked-in-one-read case: old has the laptop panel
        // and a dock's DVI, new has the laptop panel and the dock's DP.
        let before = vec![
            monitor(Some("eDP-1"), (0, 0), (1920, 1080), 1.0),
            monitor(Some("DVI-1"), (1920, 0), (2560, 1440), 1.0),
        ];
        let after = vec![
            monitor(Some("eDP-1"), (0, 0), (1920, 1080), 1.0),
            monitor(Some("DP-1"), (1920, 0), (3840, 2160), 1.5),
        ];
        let changes = diff(&before, &after);
        assert!(changes.contains(&MonitorChange::Removed {
            identity: "DVI-1".to_owned(),
        }));
        assert!(changes.contains(&MonitorChange::Added(monitor(
            Some("DP-1"),
            (1920, 0),
            (3840, 2160),
            1.5
        ))));
        assert_eq!(changes.len(), 2, "{changes:?}");
    }

    #[test]
    fn a_nameless_display_is_matched_by_its_geometry_not_reported_as_two() {
        // Wayland: no names, so two identical snapshots of two nameless
        // panels are the same world, not a swap of strangers.
        let panels = vec![
            monitor(None, (0, 0), (2560, 1440), 1.0),
            monitor(None, (2560, 0), (2560, 1440), 1.0),
        ];
        assert_eq!(diff(&panels, &panels), Vec::<MonitorChange>::new());

        // A nameless panel rescaling is still a scale change, matched by
        // geometry.
        let after = vec![
            monitor(None, (0, 0), (2560, 1440), 2.0),
            monitor(None, (2560, 0), (2560, 1440), 1.0),
        ];
        assert_eq!(
            diff(&panels, &after),
            vec![MonitorChange::ScaleChanged {
                identity: "2560x1440+0+0".to_owned(),
                from: 1.0,
                to: 2.0,
            }]
        );
    }

    #[test]
    fn a_nameless_display_that_moves_is_replaced_not_moved() {
        // The honest limit the module doc states: a nameless monitor's
        // identity is its geometry, so moving reads as remove-and-add. The
        // test pins that reading so it cannot silently become a claim about
        // "moves" no platform made.
        let before = vec![monitor(None, (0, 0), (2560, 1440), 1.0)];
        let after = vec![monitor(None, (5120, 0), (2560, 1440), 1.0)];
        let changes = diff(&before, &after);
        assert!(changes.contains(&MonitorChange::Removed {
            identity: "2560x1440+0+0".to_owned(),
        }));
        assert!(changes.contains(&MonitorChange::Added(after[0].clone())));
        assert_eq!(changes.len(), 2, "{changes:?}");
    }

    #[test]
    fn the_platforms_absurd_scale_answers_are_clamped_like_scale_is() {
        // The same rule `Scale::new` applies, for the same reason: a
        // compositor bug must not travel into application layout as a divide
        // by zero three layers away from the cause.
        assert_eq!(sanitized_scale(2.0), 2.0);
        assert_eq!(sanitized_scale(1.0), 1.0);
        for absurd in [0.0, -2.0, f64::NAN, f64::INFINITY] {
            assert_eq!(sanitized_scale(absurd), 1.0, "{absurd} should not survive");
        }
    }

    #[test]
    fn the_changes_read_as_sentences() {
        // A log line is the second consumer of this type, and a wrong one
        // costs a debugging session: the message must name the display and
        // both scales.
        let added = MonitorChange::Added(monitor(Some("DP-1"), (0, 0), (3840, 2160), 2.0));
        assert!(added.to_string().contains("DP-1"), "{}", added);
        assert!(added.to_string().contains("3840x2160"), "{}", added);
        let rescaled = MonitorChange::ScaleChanged {
            identity: "DP-1".to_owned(),
            from: 1.0,
            to: 2.0,
        };
        let line = rescaled.to_string();
        // `f64`'s `Display` drops the trailing `.0`, so the assertion is on
        // the sentence's shape rather than on a spelling that would fail the
        // day a scale factor grows a fraction.
        assert!(
            line.contains("rescaled") && line.contains("1") && line.contains("2"),
            "{line}"
        );
        assert!(line.contains("->"), "{line}");
    }
}
