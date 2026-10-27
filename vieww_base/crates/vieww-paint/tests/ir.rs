//! FW-25: `vieww-ir` round trips.
//!
//! * Random scenes, covering every command, gradients, clips, dashes,
//!   filters and layers, encode, decode and re-encode to the same bytes.
//!   They also render to the same pixels on the CPU rasterizer.
//! * Fonts and images are stored once, and sharing survives decoding.
//! * The version-1 golden fixture decodes and re-encodes byte for byte,
//!   which pins the format.
//! * Hostile bytes are refused with an error, never a panic.

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
use std::rc::Rc;

use vieww_foundation::{
    BlendMode, Color, Dash, FontData, Glyph, GlyphRun, Gradient, ImageFilter, Offset, Rect, Shadow,
    StrokeCap, StrokeJoin, StrokeStyle,
};
use vieww_paint::ir::{self, IrError};
use vieww_paint::{Canvas, Command, Image, Paint, Path, Scene, Stroke};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn f(&mut self, lo: f32, hi: f32) -> f32 {
        #[expect(clippy::cast_precision_loss, reason = "a unit float")]
        let u = (self.next() >> 40) as f32 / (1u64 << 24) as f32;
        lo + u * (hi - lo)
    }
    fn color(&mut self) -> Color {
        let b = self.next().to_le_bytes();
        Color::rgba(b[0], b[1], b[2], 128 | b[3])
    }
    fn rect(&mut self) -> Rect {
        let (x, y) = (self.f(0.0, 90.0), self.f(0.0, 70.0));
        Rect::new(x, y, x + self.f(2.0, 40.0), y + self.f(2.0, 30.0))
    }
    fn paint(&mut self) -> Paint {
        if self.next().is_multiple_of(3) {
            let r = self.rect();
            let g = match self.next() % 3 {
                0 => Gradient::linear(Offset::new(r.left, r.top), Offset::new(r.right, r.bottom)),
                1 => Gradient::radial(Offset::new(r.left, r.top), self.f(4.0, 40.0)),
                _ => Gradient::sweep(Offset::new(r.left, r.top), 0.0, 6.0),
            };
            let (c0, c1) = (self.color(), self.color());
            Paint::gradient(g.with_stops(&[(0.0, c0), (self.f(0.2, 0.8), c1), (1.0, c0)]))
        } else {
            Paint::solid(self.color())
        }
    }
    fn path(&mut self) -> Path {
        let mut p = Path::new();
        p.move_to(Offset::new(self.f(0.0, 100.0), self.f(0.0, 80.0)));
        for _ in 0..1 + self.next() % 4 {
            if self.next().is_multiple_of(2) {
                p.line_to(Offset::new(self.f(0.0, 100.0), self.f(0.0, 80.0)));
            } else {
                p.cubic_to(
                    Offset::new(self.f(0.0, 100.0), self.f(0.0, 80.0)),
                    Offset::new(self.f(0.0, 100.0), self.f(0.0, 80.0)),
                    Offset::new(self.f(0.0, 100.0), self.f(0.0, 80.0)),
                );
            }
        }
        p.close();
        p
    }
}

fn random_scene(seed: u64, image: &Image) -> Scene {
    let mut r = Rng(seed | 1);
    let mut s = Scene::new();
    let mut open = 0;
    for _ in 0..12 + r.next() % 20 {
        match r.next() % 9 {
            0 => s.fill_rect(r.rect(), r.paint()),
            1 => s.fill_path(&r.path(), r.paint()),
            2 => {
                let style = StrokeStyle {
                    cap: [StrokeCap::Butt, StrokeCap::Round, StrokeCap::Square]
                        [(r.next() % 3) as usize],
                    join: [StrokeJoin::Miter, StrokeJoin::Round, StrokeJoin::Bevel]
                        [(r.next() % 3) as usize],
                    miter_limit: r.f(1.0, 8.0),
                    dash: (r.next().is_multiple_of(2)).then(|| Dash {
                        pattern: vec![r.f(1.0, 6.0), r.f(1.0, 6.0)],
                        offset: r.f(0.0, 3.0),
                    }),
                };
                s.stroke_path(
                    &r.path(),
                    Stroke::new(r.f(0.5, 5.0)).styled(style),
                    r.paint(),
                );
            }
            3 => s.draw_shadow(
                r.rect(),
                r.f(0.0, 8.0),
                Shadow::new(r.color(), Offset::new(2.0, 3.0), r.f(0.0, 6.0)),
            ),
            4 => s.draw_image(r.rect(), image),
            5 => {
                s.save();
                s.clip_rect(r.rect());
            }
            6 if open < 3 => {
                open += 1;
                let mut f = ImageFilter::NONE;
                if r.next().is_multiple_of(2) {
                    f.blur_sigma = r.f(0.5, 3.0);
                }
                let blend = [
                    BlendMode::Normal,
                    BlendMode::Multiply,
                    BlendMode::Screen,
                    BlendMode::Hue,
                ][(r.next() % 4) as usize];
                s.push_filtered_layer(r.rect().inflate(10.0), r.f(0.3, 1.0), blend, f);
            }
            7 if open > 0 => {
                open -= 1;
                s.pop_layer();
            }
            _ => s.clip_rrect(r.rect(), r.f(1.0, 6.0)),
        }
    }
    for _ in 0..open {
        s.pop_layer();
    }
    s
}

fn image() -> Image {
    let px: Vec<u8> = (0..8 * 8)
        .flat_map(|i| [i as u8 * 4, 255 - i as u8 * 4, 90, 255])
        .collect();
    Image::from_rgba8(px, 8, 8)
}

#[test]
fn random_scenes_round_trip_to_the_same_bytes() {
    let img = image();
    for seed in 1..=200u64 {
        let scene = random_scene(seed * 0x9E37_79B9, &img);
        let bytes = ir::encode(&scene);
        let back = ir::decode(&bytes).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        assert_eq!(back.commands().len(), scene.commands().len());
        assert_eq!(
            ir::encode(&back),
            bytes,
            "seed {seed}: re-encoding changed the bytes"
        );
        assert_eq!(
            format!("{:?}", back.commands()),
            format!("{:?}", scene.commands()),
            "seed {seed}"
        );
    }
}

#[cfg(feature = "native")]
#[test]
fn a_decoded_scene_renders_the_same_pixels() {
    use vieww_paint::native::NativeRenderer;
    let img = image();
    for seed in 1..=40u64 {
        let scene = random_scene(seed * 0x51_7CC1_B727, &img);
        let back = ir::decode(&ir::encode(&scene)).unwrap();
        let a = NativeRenderer::new()
            .render_to_pixels(&scene, 120, 90, Color::WHITE)
            .unwrap()
            .0;
        let b = NativeRenderer::new()
            .render_to_pixels(&back, 120, 90, Color::WHITE)
            .unwrap()
            .0;
        assert!(
            a.data() == b.data(),
            "seed {seed}: the decoded scene draws different pixels"
        );
    }
}

#[test]
fn fonts_and_images_are_stored_once_and_shared_after_decoding() {
    let font = FontData::new(Rc::new(vec![7u8; 3000]), 2);
    let run = |x: f32| GlyphRun {
        font: font.clone(),
        size: 14.0,
        color: Color::BLACK,
        origin: Offset::new(x, 20.0),
        glyphs: Rc::from(vec![
            Glyph {
                id: 36,
                offset: Offset::new(0.0, 0.0),
            },
            Glyph {
                id: 37,
                offset: Offset::new(8.5, 0.0),
            },
        ]),
        is_rtl: false,
        ascent: 11.0,
        descent: 3.0,
    };
    let img = image();
    let mut s = Scene::new();
    for x in [0.0, 30.0, 60.0] {
        s.push_command(Command::DrawGlyphs {
            run: run(x),
            transform: vieww_foundation::Transform::IDENTITY,
            clip: vieww_paint::Clip::NONE,
        });
        s.draw_image(Rect::new(x, 40.0, x + 8.0, 48.0), &img);
    }
    let bytes = ir::encode(&s);
    assert!(
        bytes.len() < 3000 + 2 * 256 + 600,
        "the font and image are stored once ({} bytes)",
        bytes.len()
    );
    let back = ir::decode(&bytes).unwrap();
    let fonts: Vec<&FontData> = back
        .commands()
        .iter()
        .filter_map(|c| match c {
            Command::DrawGlyphs { run, .. } => Some(&run.font),
            _ => None,
        })
        .collect();
    assert_eq!(fonts.len(), 3);
    assert!(
        Rc::ptr_eq(fonts[0].shared(), fonts[2].shared()),
        "one font after decoding"
    );
    assert_eq!((fonts[0].index(), fonts[0].bytes().len()), (2, 3000));
    assert_eq!(ir::encode(&back), bytes);
}

/// The golden fixture: a scene with one of every command, its v1 bytes
/// committed. A change that alters the encoding fails here and must bump
/// `VERSION` (keeping a decoder for this one).
#[test]
fn the_version_1_fixture_is_stable() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ir/v1.bin");
    let scene = random_scene(0xF1C5_7A1D_5EED, &image());
    let bytes = ir::encode(&scene);
    if std::env::var_os("VIEWW_IR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &bytes).unwrap();
    }
    let golden = std::fs::read(&path).expect("tests/ir/v1.bin (VIEWW_IR_BLESS=1 writes it)");
    assert_eq!(
        &golden[4..6],
        &1u16.to_le_bytes(),
        "the fixture is version 1"
    );
    assert!(
        bytes == golden,
        "the v1 encoding changed: bump ir::VERSION and add a fixture"
    );
    let back = ir::decode(&golden).unwrap();
    assert_eq!(ir::encode(&back), golden);
}

#[test]
fn hostile_bytes_are_refused_not_panicked_on() {
    assert!(matches!(ir::decode(b"nope"), Err(IrError::NotIr)));
    let mut newer = ir::encode(&Scene::new());
    newer[4..6].copy_from_slice(&99u16.to_le_bytes());
    assert!(matches!(ir::decode(&newer), Err(IrError::TooNew(99))));
    let seed = ir::encode(&random_scene(42, &image()));
    let mut r = Rng(0xDEAD_BEEF);
    for _ in 0..20_000 {
        let mut m = seed.clone();
        for _ in 0..1 + r.next() % 4 {
            let at = (r.next() as usize) % m.len();
            match r.next() % 3 {
                0 => m[at] ^= 1 << (r.next() % 8),
                1 => m.truncate(at.max(8)),
                _ => m.insert(at, r.next() as u8),
            }
        }
        let _ = ir::decode(&m);
    }
}
