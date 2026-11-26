//! How hot the device is running, as the platform reports it — for a run long
//! enough to warm a phone up.
//!
//! # Why
//!
//! Every Device Farm pass used to last ten seconds. A phone holds its clocks
//! for that long and throttles later, so a frame time measured cold says
//! nothing about the minute after: the cross-framework record (Lynx's 4.2 °C
//! lower peak under sustained GPU load, against React Native) is about
//! exactly that minute. The sustained pass (`debug.vieww.frames`) runs long
//! enough to reach it, and this says how warm the phone got meanwhile.
//!
//! # What is read
//!
//! - **Android 11+ (API 30):** `AThermal_getCurrentThermalStatus` — the
//!   system's own throttling verdict, `none` to `shutdown`.
//! - **Android 12+ (API 31):** `AThermal_getThermalHeadroom` — how close the
//!   device is to severe throttling, `1.0` being there. The system answers at
//!   most once a second; asked sooner it returns NaN, which reads as `None`.
//! - **Linux (and Android, where readable):** the hottest *trustworthy*
//!   `/sys/class/thermal/thermal_zone*/temp`, in °C. A zone counts only when
//!   its `type` names a skin or SoC sensor and its reading is not a "no data"
//!   sentinel (PERF-03: the S26 Ultra's flat 105.0 °C). Skin sensors win over
//!   SoC ones. When nothing qualifies the reading is `None` and the suite
//!   prints "unknown".
//!
//! The NDK functions are looked up at run time, as `perf_hint` does: linked
//! directly, a symbol the phone's `libandroid.so` lacks stops the library
//! loading at all (the LG Stylo 5 is Android 9).

/// One reading. Every field is `None` where the platform does not say.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ThermalReading {
    /// `AThermalStatus`: 0 none, 1 light, 2 moderate, 3 severe, 4 critical,
    /// 5 emergency, 6 shutdown.
    pub status: Option<i32>,
    /// `AThermal_getThermalHeadroom(0)`: 1.0 is severe throttling.
    pub headroom: Option<f32>,
    /// The hottest thermal zone the process may read, in °C.
    pub hottest_celsius: Option<f32>,
}

impl ThermalReading {
    /// Whether the reading says anything at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.status.is_none() && self.headroom.is_none() && self.hottest_celsius.is_none()
    }
}

/// The name Android gives a thermal status.
#[must_use]
pub fn status_name(status: i32) -> &'static str {
    match status {
        0 => "none",
        1 => "light",
        2 => "moderate",
        3 => "severe",
        4 => "critical",
        5 => "emergency",
        6 => "shutdown",
        _ => "unknown",
    }
}

/// How hot the device is now. Cheap enough to call once a second; the
/// headroom is `None` if asked more often than the system allows.
#[must_use]
pub fn thermal_reading() -> ThermalReading {
    let (status, headroom) = imp::platform();
    ThermalReading {
        status,
        headroom,
        hottest_celsius: hottest_zone(),
    }
}

/// One `/sys/class/thermal/thermal_zone*` entry: what it says it measures and
/// what it reads, in millidegrees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Zone {
    pub kind: String,
    pub millicelsius: i64,
}

/// Which part of the device a zone's `type` names, best first. `None` for a
/// type that is not a temperature of anything the user touches or the SoC
/// throttles on (battery gauges, chargers, PMIC rails, virtual fan-ins).
fn rank(kind: &str) -> Option<u8> {
    let kind = kind.trim().to_ascii_lowercase();
    // The outside of the phone: what a hand feels, and what Android's own
    // skin-temperature throttling is built on.
    if [
        "skin",
        "xo-therm",
        "quiet-therm",
        "case",
        "shell",
        "back-therm",
        "front-therm",
    ]
    .iter()
    .any(|name| kind.contains(name))
    {
        return Some(0);
    }
    // The SoC and its clusters, and a desktop CPU package.
    if [
        "soc",
        "cpuss",
        "cpu-",
        "cpu_",
        "tsens_tz_sensor",
        "x86_pkg_temp",
        "coretemp",
        "acpitz",
        "gpuss",
        "gpu-",
        "big",
        "little",
        "mid-",
    ]
    .iter()
    .any(|name| kind.contains(name))
    {
        return Some(1);
    }
    None
}

/// A reading a sensor with no data reports instead of a number. These are the
/// trip-point constants some thermal HALs return for an unwired or disabled
/// sensor: the S26 Ultra printed a flat 105.0 °C for a whole run, which no
/// phone survives and no run varies by. Exact multiples only — a real die
/// temperature is not 105.000.
const SENTINEL_MILLICELSIUS: [i64; 5] = [0, 105_000, 110_000, 120_000, 125_000];

/// The hottest *trustworthy* zone, in °C — or `None`, which the suite prints
/// as "unknown" rather than a number it cannot stand behind.
///
/// Skin sensors win over SoC sensors when any are readable, because a phone's
/// outside is what throttling protects; among the same rank the hottest wins.
/// A zone is ignored when its type is unknown, its reading is a sentinel, or
/// the value is outside a plausible phone/laptop range (`-20..=100` °C).
pub(crate) fn select_zone(zones: &[Zone]) -> Option<f32> {
    let usable = |zone: &&Zone| {
        !SENTINEL_MILLICELSIUS.contains(&zone.millicelsius)
            && (-20_000..=100_000).contains(&zone.millicelsius)
    };
    let best_rank = zones
        .iter()
        .filter(usable)
        .filter_map(|zone| rank(&zone.kind))
        .min()?;
    zones
        .iter()
        .filter(usable)
        .filter(|zone| rank(&zone.kind) == Some(best_rank))
        .map(|zone| zone.millicelsius)
        .max()
        .map(|milli| milli as f32 / 1000.0)
}

/// The hottest trustworthy `/sys/class/thermal/thermal_zone*`, in °C.
fn hottest_zone() -> Option<f32> {
    let entries = std::fs::read_dir("/sys/class/thermal").ok()?;
    let mut zones = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("thermal_zone") {
            continue;
        }
        let path = entry.path();
        let Ok(text) = std::fs::read_to_string(path.join("temp")) else {
            continue;
        };
        let Ok(millicelsius) = text.trim().parse::<i64>() else {
            continue;
        };
        let kind = std::fs::read_to_string(path.join("type")).unwrap_or_default();
        zones.push(Zone {
            kind: kind.trim().to_owned(),
            millicelsius,
        });
    }
    select_zone(&zones)
}

#[cfg(target_os = "android")]
mod imp {
    use std::ffi::c_void;
    use std::sync::OnceLock;

    type Acquire = unsafe extern "C" fn() -> *mut c_void;
    type Status = unsafe extern "C" fn(*mut c_void) -> i32;
    type Headroom = unsafe extern "C" fn(*mut c_void, i32) -> f32;

    struct Api {
        manager: usize,
        status: Status,
        headroom: Option<Headroom>,
    }

    /// Look a function up in `libandroid.so`.
    ///
    /// # Safety
    ///
    /// `T` must be the function's real signature.
    unsafe fn symbol<T: Copy>(library: *mut c_void, name: &std::ffi::CStr) -> Option<T> {
        // SAFETY: `library` is a live handle; `name` is NUL-terminated.
        let address = unsafe { libc::dlsym(library, name.as_ptr()) };
        if address.is_null() {
            return None;
        }
        // SAFETY: the caller vouches for `T`; a data pointer and a function
        // pointer have the same size on every Android ABI.
        Some(unsafe { std::mem::transmute_copy::<*mut c_void, T>(&address) })
    }

    fn api() -> Option<&'static Api> {
        static API: OnceLock<Option<Api>> = OnceLock::new();
        API.get_or_init(|| {
            // SAFETY: `dlopen` of a system library by name; never closed, so
            // the symbols stay valid for the process.
            let library = unsafe { libc::dlopen(c"libandroid.so".as_ptr(), libc::RTLD_NOW) };
            if library.is_null() {
                return None;
            }
            // SAFETY: the NDK's signatures (android/thermal.h).
            let (acquire, status, headroom) = unsafe {
                (
                    symbol::<Acquire>(library, c"AThermal_acquireManager")?,
                    symbol::<Status>(library, c"AThermal_getCurrentThermalStatus")?,
                    symbol::<Headroom>(library, c"AThermal_getThermalHeadroom"),
                )
            };
            // SAFETY: returns a manager owned by the caller; kept for the
            // process's life, as the NDK allows.
            let manager = unsafe { acquire() };
            (!manager.is_null()).then_some(Api {
                manager: manager as usize,
                status,
                headroom,
            })
        })
        .as_ref()
    }

    pub(super) fn platform() -> (Option<i32>, Option<f32>) {
        let Some(api) = api() else {
            return (None, None);
        };
        let manager = api.manager as *mut c_void;
        // SAFETY: a live manager, and the NDK's signatures.
        let status = unsafe { (api.status)(manager) };
        let headroom = api
            .headroom
            // SAFETY: as above; 0 asks about now rather than a forecast.
            .map(|headroom| unsafe { headroom(manager, 0) })
            .filter(|h| h.is_finite());
        ((status >= 0).then_some(status), headroom)
    }
}

#[cfg(not(target_os = "android"))]
mod imp {
    pub(super) const fn platform() -> (Option<i32>, Option<f32>) {
        (None, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(kind: &str, milli: i64) -> Zone {
        Zone {
            kind: kind.to_owned(),
            millicelsius: milli,
        }
    }

    /// PERF-03: the S26 Ultra's constant 105.0 °C.
    #[test]
    fn a_constant_105_degree_sentinel_is_not_a_temperature() {
        let zones = [zone("skin-therm", 105_000), zone("cpuss-0", 105_000)];
        assert_eq!(select_zone(&zones), None, "print unknown, not 105.0");
    }

    #[test]
    fn skin_beats_soc_even_when_the_soc_is_hotter() {
        let zones = [
            zone("cpuss-0", 71_500),
            zone("skin-therm", 38_200),
            zone("battery", 31_000),
        ];
        assert_eq!(select_zone(&zones), Some(38.2));
    }

    #[test]
    fn without_a_skin_sensor_the_hottest_soc_zone_is_used() {
        let zones = [
            zone("cpu-0-0-usr", 52_000),
            zone("cpu-1-0-usr", 58_500),
            zone("pmic-therm", 90_000),
        ];
        assert_eq!(
            select_zone(&zones),
            Some(58.5),
            "PMIC rails are not the SoC"
        );
    }

    #[test]
    fn unknown_types_and_implausible_values_are_ignored() {
        assert_eq!(select_zone(&[zone("mystery", 40_000)]), None);
        assert_eq!(
            select_zone(&[zone("cpuss-0", 180_000)]),
            None,
            "out of range"
        );
        assert_eq!(select_zone(&[zone("cpuss-0", 0)]), None, "zero is no data");
        assert_eq!(select_zone(&[]), None);
        // A real desktop reading is still reported.
        assert_eq!(select_zone(&[zone("x86_pkg_temp", 47_000)]), Some(47.0));
    }

    #[test]
    fn statuses_have_android_names() {
        assert_eq!(status_name(0), "none");
        assert_eq!(status_name(3), "severe");
        assert_eq!(status_name(6), "shutdown");
        assert_eq!(status_name(42), "unknown");
    }

    #[test]
    fn a_reading_never_panics_and_says_whether_it_said_anything() {
        let reading = thermal_reading();
        assert_eq!(
            reading.is_empty(),
            reading.status.is_none()
                && reading.headroom.is_none()
                && reading.hottest_celsius.is_none()
        );
    }
}
