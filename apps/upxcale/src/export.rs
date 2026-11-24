//! Writing an upscaled photograph back out as a file.
//!
//! `vieww-asset` decodes and does not encode — [`decode`](vieww_asset::decode)
//! and `decode_sized` are the whole of its raster surface, which is the right
//! scope for a crate whose job is "loading an application's own files". Saving
//! is the application's job, so the PNG encoder is a direct dependency here.
//!
//! # Where files go, and why that is a stand-in
//!
//! On a phone this would hand pixels to the system photo library. There is no
//! such service in vieww — `docs/PRODUCTION-GAPS.md` is explicit that camera,
//! geolocation, push and biometrics each still need per-platform FFI, and a
//! photo library is the same shape of hole. So this writes into a directory
//! beside the running binary and says where it went.
//!
//! That is a real save rather than a stub: the file exists and opens. It is
//! simply not the platform integration the button's label implies, and pretending
//! otherwise in the code would hide the one thing a reader most needs to know.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use vieww_foundation::Image;

static EXPORT_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Point exports at a directory the platform lets the app write to — the
/// app-private data directory on Android, where the old relative
/// `upxcale-exports/` (the process's cwd) failed silently (UP-01). Set once,
/// before the first save; a second call is ignored.
pub fn set_export_dir(dir: PathBuf) {
    let _ = EXPORT_DIR.set(dir);
}

/// Where exports land: the directory given to [`set_export_dir`], else
/// `upxcale-exports/` in the current working directory (a desktop run).
#[must_use]
pub fn export_dir() -> PathBuf {
    EXPORT_DIR
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("upxcale-exports"))
}

/// How a background save ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveOutcome {
    Saved(PathBuf),
    Failed(String),
}

/// The words a toast shows for an outcome.
#[must_use]
pub fn outcome_message(outcome: &SaveOutcome) -> String {
    match outcome {
        SaveOutcome::Saved(path) => format!("Saved to {}", path.display()),
        SaveOutcome::Failed(error) => format!("Could not save: {error}"),
    }
}

/// Encode `image` as a PNG and write it into [`export_dir`].
///
/// The filename is `<slug>-4x.png`. Re-saving overwrites, which is what a user
/// pressing the button twice means.
pub fn write_png(image: &Image, slug: &str) -> std::io::Result<PathBuf> {
    let dir = export_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{slug}-{}x.png", crate::photos::UPSCALE_FACTOR));
    write_png_to(image, &path)?;
    Ok(path)
}

/// Encode and write to an exact path — the half of [`write_png`] that tests
/// can point at a temporary directory.
pub fn write_png_to(image: &Image, path: &Path) -> std::io::Result<()> {
    let bytes = encode_png(image)?;
    std::fs::write(path, bytes)
}

/// Encode straight-alpha RGBA8 as PNG bytes.
pub fn encode_png(image: &Image) -> std::io::Result<Vec<u8>> {
    // `Image` is tightly packed RGBA8, which is exactly `image`'s `Rgba8`
    // layout, so this is a wrap rather than a conversion.
    let buffer: image::RgbaImage =
        image::ImageBuffer::from_raw(image.width(), image.height(), image.pixels().to_vec())
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "pixel buffer does not match the image's dimensions",
                )
            })?;

    let mut out = Cursor::new(Vec::new());
    buffer
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A round trip through the encoder must preserve every pixel — a save that
    /// quietly changes the picture is worse than one that fails.
    #[test]
    fn png_round_trip_is_lossless() {
        let pixels: Vec<u8> = (0..16 * 16 * 4).map(|i| (i % 251) as u8).collect();
        let original = Image::from_rgba8(pixels, 16, 16);

        let encoded = encode_png(&original).expect("encoding");
        let (decoded, format) = vieww_asset::decode(&encoded).expect("decoding");

        assert_eq!(format, vieww_asset::ImageFormat::Png);
        assert_eq!(decoded.width(), 16);
        assert_eq!(decoded.height(), 16);
        assert_eq!(decoded.pixels(), original.pixels());
    }
}
