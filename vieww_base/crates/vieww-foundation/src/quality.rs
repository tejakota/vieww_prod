//! Device tiers and the quality profile they resolve to.
//!
//! # Why this lives in the foundation
//!
//! The probe that *assigns* a tier is platform code (`vieww-platform-winit`
//! reads `/proc/meminfo`, Android system properties and so on). What a tier
//! *means* has to be readable from a widget's `build`, and widgets sit below
//! the platform layer — so the vocabulary lives here and the platform
//! publishes one [`QualityProfile`] to the tree, exactly as it publishes
//! [`Accessibility`](crate::Accessibility) and
//! [`ViewMetrics`](crate::ViewMetrics).
//!
//! ```ignore
//! // In any build:
//! let quality = ctx.inherit_or(QualityProfile::HIGH);
//! if quality.backdrop_blur { /* frosted glass */ } else { /* flat wash */ }
//! ```
//!
//! # What a profile gates
//!
//! | field | gates | Low | Medium | High |
//! |---|---|---|---|---|
//! | `resolution_scale` | GPU direct-route render scale (phones only) | 0.5 | 0.75 | 1.0 |
//! | `max_internal_scale` | any app's own offscreen / 3D buffer | 0.5 | 0.75 | 1.0 |
//! | `backdrop_blur` | frosted glass; off = flat translucent wash | off | on | on |
//! | `max_blur_px` | blur radius ceiling | 0 | 12 | no cap |
//! | `shadow_blur` | blurred drop shadows; off = hard-edged offset | off | on | on |
//! | `ambient_motion` | drift, glows, idle "breathing" | off | on | on |
//!
//! A desktop never reduces `resolution_scale` or `max_internal_scale`: a
//! 2-core laptop reads as `Low` by the RAM/core rule, and a blurry window on
//! a machine that can fill its own pixels is the wrong trade. Effects still
//! follow the tier on a desktop, because those are what the i5-7200U
//! measurements (`editor-glass` 31.6 ms) are about.

use core::fmt;

/// The coarseness at which quality decisions are made. Deliberately three
/// levels: finer bands would want measurements the farm has not taken yet,
/// and a tier nobody can explain the boundaries of is worse than a coarse one
/// everybody can.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeviceTier {
    /// A phone where the CPU raster can miss the budget outright: the LG
    /// Stylo 5 (2 GB, Adreno 506). Quality is the cheapest there is, and the
    /// question is whether the GPU path can be made to pass at all.
    Low,
    /// The middle of the farm: the Galaxy A36 (6 GB), A73 (8 GB), S20 (8 GB),
    /// iPhone 13 (4 GB). Measurements decide the rasterizer; effects may run
    /// if they pay.
    Medium,
    /// Where every path passes with room: the S26 Ultra (12 GB) — more than
    /// 8 GB of RAM. Full resolution, effects on.
    High,
}

impl DeviceTier {
    /// The tier's name, as the Device Farm summary prints it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

impl fmt::Display for DeviceTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Whether the device is a phone/tablet or a desktop. Only the resolution
/// fields differ between the two; see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceClass {
    /// Android or iOS.
    Handheld,
    /// Windows, macOS, Linux, web.
    Desktop,
}

/// The render decisions a device tier gets by default.
///
/// Every field is a default, not a mandate: explicit application settings and
/// diagnostics (`VIEWW_GPU_SCALE`) override the profile, and the profile's own
/// numbers are what Device Farm measures against. `Copy` and comparable so it
/// can be published as an inherited value and a rebuild only fires on change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QualityProfile {
    /// The tier this profile was derived from. Informational: widgets should
    /// read the gates below rather than branch on the tier, so a future tier
    /// (or a user override) needs no widget changes.
    pub tier: DeviceTier,
    /// The fraction of the swapchain's resolution the GPU path's direct route
    /// renders at. `1.0` is full resolution; the CPU path is unaffected by
    /// this field whatever it says (its output is the fidelity reference).
    pub resolution_scale: f32,
    /// The largest scale any app-owned offscreen buffer (a 3D viewport, a
    /// compare stage, a body model) should settle at. Apps use
    /// `min(1.0, max_internal_scale)` for their settled frame.
    pub max_internal_scale: f32,
    /// Frosted glass allowed. Off means a flat translucent fill.
    pub backdrop_blur: bool,
    /// The blur radius ceiling in logical pixels, for tiers that allow blur
    /// but not an unbounded one. `f32::INFINITY` is "no ceiling".
    pub max_blur_px: f32,
    /// Blurred drop shadows allowed. Off means a hard-edged offset shadow.
    pub shadow_blur: bool,
    /// Ambient motion allowed: idle drift, breathing glows, shimmer. This is
    /// separate from reduce-motion, which is a person's request; this is the
    /// device's budget. Either one being "no" stops the motion.
    pub ambient_motion: bool,
}

impl QualityProfile {
    /// The Low tier on a phone: the LG Stylo 5 class. A quarter of the pixels
    /// — the single biggest lever on a fill-rate-bound frame — and no blur.
    pub const LOW: Self = Self {
        tier: DeviceTier::Low,
        resolution_scale: 0.5,
        max_internal_scale: 0.5,
        backdrop_blur: false,
        max_blur_px: 0.0,
        shadow_blur: false,
        ambient_motion: false,
    };

    /// The Medium tier: the A36 / iPhone 13 class, where measurements already
    /// pick a rasterizer that passes. Three quarters keeps the density loss
    /// at the edge of noticeability while cutting a third of the fill; blur
    /// stays but is capped at 12 px.
    pub const MEDIUM: Self = Self {
        tier: DeviceTier::Medium,
        resolution_scale: 0.75,
        max_internal_scale: 0.75,
        backdrop_blur: true,
        max_blur_px: 12.0,
        shadow_blur: true,
        ambient_motion: true,
    };

    /// The High tier: everything passes with room, so nothing is scaled.
    pub const HIGH: Self = Self {
        tier: DeviceTier::High,
        resolution_scale: 1.0,
        max_internal_scale: 1.0,
        backdrop_blur: true,
        max_blur_px: f32::INFINITY,
        shadow_blur: true,
        ambient_motion: true,
    };

    /// The profile a tier gets on a phone. Kept for callers that predate
    /// [`for_device`](Self::for_device); prefer that.
    #[must_use]
    pub const fn for_tier(tier: DeviceTier) -> Self {
        match tier {
            DeviceTier::Low => Self::LOW,
            DeviceTier::Medium => Self::MEDIUM,
            DeviceTier::High => Self::HIGH,
        }
    }

    /// The profile a tier gets on this class of device.
    ///
    /// A desktop keeps its effect gates but never reduces resolution — see the
    /// module docs for the reason.
    #[must_use]
    pub const fn for_device(tier: DeviceTier, class: DeviceClass) -> Self {
        let base = Self::for_tier(tier);
        match class {
            DeviceClass::Handheld => base,
            DeviceClass::Desktop => Self {
                resolution_scale: 1.0,
                max_internal_scale: 1.0,
                ..base
            },
        }
    }

    /// The scale an app's *settled* offscreen frame should use: capped at 1.0
    /// and never below a usable floor.
    #[must_use]
    pub fn settled_scale(&self) -> f32 {
        self.max_internal_scale.clamp(0.25, 1.0)
    }

    /// A blur radius after the tier's ceiling, or `None` when blur is off and
    /// the caller should draw its flat fallback.
    #[must_use]
    pub fn clamp_blur(&self, requested_px: f32) -> Option<f32> {
        if !self.backdrop_blur || requested_px <= 0.0 {
            return None;
        }
        Some(requested_px.min(self.max_blur_px))
    }

    /// Whether ambient (idle) motion may run, given the person's reduce-motion
    /// preference. Either one can stop it.
    #[must_use]
    pub const fn allows_ambient_motion(&self, reduce_motion: bool) -> bool {
        self.ambient_motion && !reduce_motion
    }
}

impl Default for QualityProfile {
    /// Full quality: what a tree gets when nothing published a profile (a
    /// headless render, a test).
    fn default() -> Self {
        Self::HIGH
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_profiles_match_the_tiers() {
        assert_eq!(QualityProfile::for_tier(DeviceTier::Low), QualityProfile::LOW);
        assert_eq!(
            QualityProfile::for_tier(DeviceTier::Medium),
            QualityProfile::MEDIUM
        );
        assert_eq!(
            QualityProfile::for_tier(DeviceTier::High),
            QualityProfile::HIGH
        );
    }

    #[test]
    fn effects_only_ever_get_cheaper_as_the_tier_drops() {
        let (l, m, h) = (QualityProfile::LOW, QualityProfile::MEDIUM, QualityProfile::HIGH);
        assert!(l.resolution_scale <= m.resolution_scale && m.resolution_scale <= h.resolution_scale);
        assert!(l.max_internal_scale <= m.max_internal_scale);
        assert!(l.max_blur_px <= m.max_blur_px && m.max_blur_px <= h.max_blur_px);
        assert!(!l.backdrop_blur && m.backdrop_blur && h.backdrop_blur);
        assert!(!l.shadow_blur && !l.ambient_motion);
    }

    #[test]
    fn a_desktop_is_never_resolution_scaled_but_keeps_its_effect_gates() {
        let low_desktop = QualityProfile::for_device(DeviceTier::Low, DeviceClass::Desktop);
        assert_eq!(low_desktop.resolution_scale, 1.0);
        assert_eq!(low_desktop.settled_scale(), 1.0, "idle desktop renders at 1.0x");
        assert!(!low_desktop.backdrop_blur, "the i5's glass fixtures are why");
        let phone = QualityProfile::for_device(DeviceTier::Low, DeviceClass::Handheld);
        assert_eq!(phone.resolution_scale, 0.5);
        assert_eq!(phone.settled_scale(), 0.5);
    }

    #[test]
    fn blur_is_dropped_capped_or_passed_through_by_tier() {
        assert_eq!(QualityProfile::LOW.clamp_blur(24.0), None);
        assert_eq!(QualityProfile::MEDIUM.clamp_blur(24.0), Some(12.0));
        assert_eq!(QualityProfile::MEDIUM.clamp_blur(8.0), Some(8.0));
        assert_eq!(QualityProfile::HIGH.clamp_blur(24.0), Some(24.0));
        assert_eq!(QualityProfile::HIGH.clamp_blur(0.0), None, "no blur asked, none drawn");
    }

    #[test]
    fn ambient_motion_needs_both_the_device_and_the_person_to_agree() {
        assert!(QualityProfile::HIGH.allows_ambient_motion(false));
        assert!(!QualityProfile::HIGH.allows_ambient_motion(true));
        assert!(!QualityProfile::LOW.allows_ambient_motion(false));
    }

    #[test]
    fn a_tree_with_no_published_profile_gets_full_quality() {
        assert_eq!(QualityProfile::default(), QualityProfile::HIGH);
    }
}
