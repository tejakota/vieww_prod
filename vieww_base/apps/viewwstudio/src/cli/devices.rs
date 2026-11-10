//! `devices` — the preview pane's device table, as a command and as the
//! names `--device` takes.
//!
//! One table, not two: the presets are [`crate::state::Device::ALL`] — the
//! same eight the studio's device picker lists, with the same sizes, the
//! same platforms, the same density and safe-area metrics `view_metrics`
//! derives from them. All this module adds is the *spelling*: a CLI takes
//! names typed at a keyboard, so `Phone (small)` is `phone-small`, and the
//! generic words people actually type (`mobile`, `android`, `desktop`) are
//! aliases for the presets they mean.

use crate::state::{Device, Platform};

/// The device `preview` frames by default: the pane's own phone, the same
/// `393×852` every default render uses.
pub(crate) const PREVIEW_DEFAULT: &str = "mobile";

/// The CLI's name for a preset: lowercase, spaces and parentheses to dashes,
/// the dashes collapsed and trimmed — `Phone (small)` → `phone-small`,
/// `Android tablet` → `android-tablet`.
fn cli_name(device: Device) -> String {
    device
        .name
        .to_lowercase()
        .replace([' ', '(', ')'], "-")
        .replace("--", "-")
        .trim_matches('-')
        .to_owned()
}

/// The preset a `--device` name means, or a refusal naming every name there
/// is — including the short forms, because that is the list a person scans
/// when the one they typed is not on it.
///
/// The name is normalised the way [`cli_name`] normalises a preset's own
/// name, so `Phone (large)`, `PHONE LARGE` and `phone-large` are all the
/// same preset — the spelling a person typed is a question of keyboards,
/// not of devices.
///
/// # Errors
///
/// The name is no preset's and no alias's.
pub(crate) fn by_name(name: &str) -> Result<Device, String> {
    let wanted = name
        .trim()
        .to_lowercase()
        .replace([' ', '(', ')'], "-")
        .replace("--", "-");
    let wanted = wanted.trim_matches('-');
    let table = Device::ALL;

    // The generic words first, because they are the ones people type: the
    // user's own examples say `--device mobile`, and a table that answers
    // "no device called mobile" to that is a table nobody reads past.
    let alias = match wanted {
        "mobile" | "phone" | "ios" => Some("phone"),
        "android" => Some("android-phone"),
        "tablet" | "ipad" => Some("tablet"),
        "desktop" | "web" => Some("desktop"),
        _ => None,
    };
    if let Some(alias) = alias {
        return table
            .iter()
            .find(|device| cli_name(**device) == alias)
            .copied()
            .ok_or_else(|| format!("no device called {name}"));
    }

    // Then the presets, by their own names.
    if let Some(device) = table.iter().find(|device| cli_name(**device) == wanted) {
        return Ok(*device);
    }

    // And a refusal that is a list — every preset's CLI name, so the fix is
    // on the same line as the error.
    let names: Vec<String> = table.iter().map(|d| cli_name(*d)).collect();
    Err(format!(
        "no device called \"{name}\" — the presets are: {} (or the short forms mobile, \
         android, tablet, desktop)",
        names.join(", ")
    ))
}

/// `devices`: the table, one line per preset — the name `--device` takes,
/// the size in points, the platform whose conventions it implies, and the
/// density the screen is told. `--list` for a script is what it always
/// prints; there is nothing else to configure.
///
/// # Errors
///
/// Never; the signature keeps the shape the other commands have.
pub fn run_devices() -> Result<String, String> {
    let mut out = String::from("the presets --device takes (points, platform, density):\n");
    for device in Device::ALL {
        let density = match device.platform {
            Platform::Ios => "3×",
            Platform::Android => "2.625×",
            Platform::Desktop => "1×",
        };
        out.push_str(&format!(
            "  {:<16} {:>9}   {:<8} {}\n",
            cli_name(device),
            device.size_label(),
            device.platform.label(),
            density,
        ));
    }
    out.push_str("  --landscape turns any of them on its side.\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_is_reachable_by_its_cli_name() {
        for device in Device::ALL {
            let name = cli_name(device);
            assert_eq!(by_name(&name).unwrap(), device, "round-trips {name}");
        }
    }

    #[test]
    fn the_generic_words_mean_the_presets_people_mean() {
        assert_eq!(by_name("mobile").unwrap().name, "Phone");
        assert_eq!(by_name("phone").unwrap().name, "Phone");
        assert_eq!(by_name("ios").unwrap().name, "Phone");
        assert_eq!(by_name("android").unwrap().name, "Android phone");
        assert_eq!(by_name("tablet").unwrap().name, "Tablet");
        assert_eq!(by_name("desktop").unwrap().name, "Desktop");
        assert_eq!(by_name("laptop").unwrap().name, "Laptop");
    }

    #[test]
    fn case_and_padding_do_not_matter() {
        assert_eq!(by_name(" Phone (large) ").unwrap().name, "Phone (large)");
        assert_eq!(by_name("PHONE-LARGE").unwrap().name, "Phone (large)");
    }

    #[test]
    fn an_unknown_device_lists_the_names() {
        let error = by_name("blackberry").unwrap_err();
        assert!(error.contains("blackberry"), "{error}");
        assert!(error.contains("android-tablet"), "{error}");
        assert!(error.contains("mobile"), "{error}");
    }

    #[test]
    fn the_listing_names_every_preset_and_a_density() {
        let listing = run_devices().unwrap();
        for device in Device::ALL {
            assert!(
                listing.contains(&cli_name(device)),
                "the listing names {device:?}"
            );
        }
        assert!(listing.contains("393 × 852"), "the Phone's size: {listing}");
        assert!(listing.contains("--landscape"), "{listing}");
    }
}
