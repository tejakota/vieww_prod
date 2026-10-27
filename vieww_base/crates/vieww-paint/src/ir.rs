//! `vieww-ir`: a versioned binary encoding of a [`Scene`] (FW-25).
//!
//! A scene was an in-memory enum and nothing else. This module gives it a
//! stable interchange format, so that tools outside the process can use it:
//! profilers, the studio, fuzzers that generate scenes, other-language
//! bindings, and bug reports that replay exactly what was drawn. It is a
//! **testing and tooling contract**, never the format on the hot path: a
//! frame is still a `Vec<Command>`.
//!
//! # Layout (version 1)
//!
//! Little-endian throughout; `f32` as its IEEE bits, so every value
//! round-trips exactly (NaN payloads and signed zeros included).
//!
//! ```text
//! "VWIR" u16 version u16 flags(0)
//! u32 font count   { u32 face index, u32 variation count { [u8;4] tag, f32 value }, u32 len, bytes }
//! u32 image count  { u32 width, u32 height, u32 len, RGBA8 bytes }
//! u32 command count { u8 tag, payload }
//! ```
//!
//! A run's font and an image's pixels are stored once, in the tables.
//! Commands refer to them by index, and identity is kept on decode: runs
//! that shared a font before encoding share one after.
//!
//! # Versions
//!
//! [`decode`] reads [`VERSION`] and every earlier version. A file from a
//! newer vieww is refused by name, not misread. Golden fixtures under
//! `tests/ir/` pin each version's bytes.

use std::rc::Rc;

use vieww_foundation::{
    BlendMode, Color, Dash, FontData, FontVariation, Glyph, GlyphRun, Gradient, GradientGeometry,
    ImageFilter, Offset, Rect, Shadow, StrokeCap, StrokeJoin, StrokeStyle, Transform,
};

use crate::scene::{Clip, Command, Scene};
use crate::{Image, Paint, Path, Stroke};

/// The version [`encode`] writes.
pub const VERSION: u16 = 1;
const MAGIC: &[u8; 4] = b"VWIR";

/// Why bytes are not a scene.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrError {
    /// Not `vieww-ir` at all.
    NotIr,
    /// Written by a newer vieww (its version) than this one reads.
    TooNew(u16),
    /// Ends before a value it promised.
    Truncated,
    /// A value no encoder writes (a tag, an index, a size).
    Malformed(&'static str),
}

impl std::fmt::Display for IrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotIr => write!(f, "not vieww-ir (no VWIR header)"),
            Self::TooNew(v) => write!(
                f,
                "vieww-ir version {v} is newer than this build reads ({VERSION})"
            ),
            Self::Truncated => write!(f, "vieww-ir ends early"),
            Self::Malformed(w) => write!(f, "malformed vieww-ir: {w}"),
        }
    }
}

impl std::error::Error for IrError {}

const BLENDS: [BlendMode; 28] = [
    BlendMode::Normal,
    BlendMode::Clear,
    BlendMode::Src,
    BlendMode::Dst,
    BlendMode::DstOver,
    BlendMode::SrcIn,
    BlendMode::DstIn,
    BlendMode::SrcOut,
    BlendMode::DstOut,
    BlendMode::SrcAtop,
    BlendMode::DstAtop,
    BlendMode::Xor,
    BlendMode::Plus,
    BlendMode::Multiply,
    BlendMode::Screen,
    BlendMode::Overlay,
    BlendMode::Darken,
    BlendMode::Lighten,
    BlendMode::ColorDodge,
    BlendMode::ColorBurn,
    BlendMode::HardLight,
    BlendMode::SoftLight,
    BlendMode::Difference,
    BlendMode::Exclusion,
    BlendMode::Hue,
    BlendMode::Saturation,
    BlendMode::Color,
    BlendMode::Luminosity,
];

// ── writing ───────────────────────────────────────────────────────────────

struct W {
    out: Vec<u8>,
    fonts: Vec<FontData>,
    images: Vec<Image>,
}

impl W {
    fn u8(&mut self, v: u8) {
        self.out.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.out.extend(v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.out.extend(v.to_le_bytes());
    }
    fn len(&mut self, n: usize) {
        self.u32(u32::try_from(n).expect("a scene table under 4 G entries"));
    }
    fn f32(&mut self, v: f32) {
        self.out.extend(v.to_bits().to_le_bytes());
    }
    fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }
    fn offset(&mut self, o: Offset) {
        self.f32(o.dx);
        self.f32(o.dy);
    }
    fn rect(&mut self, r: Rect) {
        self.f32(r.left);
        self.f32(r.top);
        self.f32(r.right);
        self.f32(r.bottom);
    }
    fn color(&mut self, c: Color) {
        self.out.extend([c.r, c.g, c.b, c.a]);
    }
    fn transform(&mut self, t: Transform) {
        for v in [t.a, t.b, t.c, t.d, t.tx, t.ty] {
            self.f32(v);
        }
    }
    fn opt_f32(&mut self, v: Option<f32>) {
        match v {
            Some(x) => {
                self.u8(1);
                self.f32(x);
            }
            None => self.u8(0),
        }
    }
    fn path(&mut self, p: &Path) {
        use vieww_foundation::PathVerb;
        self.len(p.verbs().len());
        for v in p.verbs() {
            match *v {
                PathVerb::MoveTo(a) => {
                    self.u8(0);
                    self.offset(a);
                }
                PathVerb::LineTo(a) => {
                    self.u8(1);
                    self.offset(a);
                }
                PathVerb::CubicTo(a, b, c) => {
                    self.u8(2);
                    self.offset(a);
                    self.offset(b);
                    self.offset(c);
                }
                PathVerb::Close => self.u8(3),
            }
        }
    }
    fn clip(&mut self, c: &Clip) {
        match c.bounds() {
            Some(r) => {
                self.u8(1);
                self.rect(r);
            }
            None => self.u8(0),
        }
        self.len(c.shapes().len());
        for s in c.shapes() {
            self.path(s);
        }
    }
    fn paint(&mut self, p: &Paint) {
        self.color(p.color);
        match &p.gradient {
            None => self.u8(0),
            Some(g) => {
                match g.geometry {
                    GradientGeometry::Linear { start, end } => {
                        self.u8(1);
                        self.offset(start);
                        self.offset(end);
                    }
                    GradientGeometry::Radial { center, radius } => {
                        self.u8(2);
                        self.offset(center);
                        self.f32(radius);
                    }
                    GradientGeometry::Sweep {
                        center,
                        start_angle,
                        end_angle,
                    } => {
                        self.u8(3);
                        self.offset(center);
                        self.f32(start_angle);
                        self.f32(end_angle);
                    }
                }
                self.bool(g.dither());
                self.len(g.stops().len());
                for s in g.stops() {
                    self.f32(s.offset);
                    self.color(s.color);
                }
            }
        }
    }
    fn stroke(&mut self, s: &Stroke) {
        self.f32(s.width);
        self.u8(match s.style.cap {
            StrokeCap::Butt => 0,
            StrokeCap::Round => 1,
            StrokeCap::Square => 2,
        });
        self.u8(match s.style.join {
            StrokeJoin::Miter => 0,
            StrokeJoin::Round => 1,
            StrokeJoin::Bevel => 2,
        });
        self.f32(s.style.miter_limit);
        match &s.style.dash {
            None => self.u8(0),
            Some(d) => {
                self.u8(1);
                self.f32(d.offset);
                self.len(d.pattern.len());
                for v in &d.pattern {
                    self.f32(*v);
                }
            }
        }
    }
    fn font(&mut self, f: &FontData) -> u32 {
        let at = self
            .fonts
            .iter()
            .position(|k| {
                Rc::ptr_eq(k.shared(), f.shared())
                    && k.index() == f.index()
                    && k.variations() == f.variations()
            })
            .unwrap_or_else(|| {
                self.fonts.push(f.clone());
                self.fonts.len() - 1
            });
        u32::try_from(at).expect("fewer than 4 G fonts")
    }
    fn image(&mut self, i: &Image) -> u32 {
        let at = self.images.iter().position(|k| k == i).unwrap_or_else(|| {
            self.images.push(i.clone());
            self.images.len() - 1
        });
        u32::try_from(at).expect("fewer than 4 G images")
    }
    fn filter(&mut self, f: &ImageFilter) {
        self.f32(f.blur_sigma);
        self.opt_f32(f.blur_angle);
        match &f.color_matrix {
            None => self.u8(0),
            Some(m) => {
                self.u8(1);
                for v in m {
                    self.f32(*v);
                }
            }
        }
        self.bool(f.backdrop);
    }
}

/// `scene`'s commands as `vieww-ir` bytes, at [`VERSION`].
#[must_use]
pub fn encode(scene: &Scene) -> Vec<u8> {
    let mut body = W {
        out: Vec::new(),
        fonts: Vec::new(),
        images: Vec::new(),
    };
    body.len(scene.commands().len());
    for c in scene.commands() {
        match c {
            Command::FillRect {
                rect,
                paint,
                transform,
                clip,
            } => {
                body.u8(0);
                body.rect(*rect);
                body.paint(paint);
                body.transform(*transform);
                body.clip(clip);
            }
            Command::FillPath {
                path,
                paint,
                transform,
                clip,
            } => {
                body.u8(1);
                body.path(path);
                body.paint(paint);
                body.transform(*transform);
                body.clip(clip);
            }
            Command::StrokePath {
                path,
                stroke,
                paint,
                transform,
                clip,
            } => {
                body.u8(2);
                body.path(path);
                body.stroke(stroke);
                body.paint(paint);
                body.transform(*transform);
                body.clip(clip);
            }
            Command::DrawShadow {
                rect,
                radius,
                shadow,
                transform,
                clip,
            } => {
                body.u8(3);
                body.rect(*rect);
                body.f32(*radius);
                body.color(shadow.color);
                body.offset(shadow.offset);
                body.f32(shadow.blur);
                body.f32(shadow.spread);
                body.bool(shadow.is_inset);
                body.transform(*transform);
                body.clip(clip);
            }
            Command::DrawGlyphs {
                run,
                transform,
                clip,
            } => {
                body.u8(4);
                let font = body.font(&run.font);
                body.u32(font);
                body.f32(run.size);
                body.color(run.color);
                body.offset(run.origin);
                body.bool(run.is_rtl);
                body.f32(run.ascent);
                body.f32(run.descent);
                body.len(run.glyphs.len());
                for g in run.glyphs.iter() {
                    body.u16(g.id);
                    body.offset(g.offset);
                }
                body.transform(*transform);
                body.clip(clip);
            }
            Command::DrawImage {
                rect,
                image,
                transform,
                clip,
            } => {
                body.u8(5);
                body.rect(*rect);
                let i = body.image(image);
                body.u32(i);
                body.transform(*transform);
                body.clip(clip);
            }
            Command::PushLayer {
                bounds,
                alpha,
                blend,
                clip,
                filter,
            } => {
                body.u8(6);
                body.rect(*bounds);
                body.f32(*alpha);
                let b = BLENDS.iter().position(|x| x == blend).unwrap_or(0);
                body.u8(u8::try_from(b).unwrap_or(0));
                body.clip(clip);
                body.filter(filter);
            }
            Command::PopLayer => body.u8(7),
        }
    }
    let mut head = W {
        out: Vec::with_capacity(body.out.len() + 64),
        fonts: Vec::new(),
        images: Vec::new(),
    };
    head.out.extend(MAGIC);
    head.u16(VERSION);
    head.u16(0);
    head.len(body.fonts.len());
    for f in &body.fonts {
        head.u32(f.index());
        head.len(f.variations().len());
        for v in f.variations() {
            head.out.extend(v.tag);
            head.f32(v.value);
        }
        head.len(f.bytes().len());
        head.out.extend(f.bytes());
    }
    head.len(body.images.len());
    for i in &body.images {
        head.u32(i.width());
        head.u32(i.height());
        head.len(i.pixels().len());
        head.out.extend(i.pixels());
    }
    head.out.extend(body.out);
    head.out
}

// ── reading ───────────────────────────────────────────────────────────────

struct R<'a> {
    b: &'a [u8],
    at: usize,
    fonts: Vec<FontData>,
    images: Vec<Image>,
}

/// More entries than any real table holds: a corrupt count, refused before
/// anything is allocated for it.
const MAX_COUNT: usize = 1 << 24;

impl R<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], IrError> {
        let end = self.at.checked_add(n).ok_or(IrError::Truncated)?;
        let s = self.b.get(self.at..end).ok_or(IrError::Truncated)?;
        self.at = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, IrError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, IrError> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Result<u32, IrError> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn count(&mut self) -> Result<usize, IrError> {
        let n = usize::try_from(self.u32()?).map_err(|_| IrError::Malformed("count"))?;
        if n > MAX_COUNT || n > self.b.len() - self.at.min(self.b.len()) {
            return Err(IrError::Malformed("a count larger than the data"));
        }
        Ok(n)
    }
    fn f32(&mut self) -> Result<f32, IrError> {
        Ok(f32::from_bits(self.u32()?))
    }
    fn bool(&mut self) -> Result<bool, IrError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(IrError::Malformed("bool")),
        }
    }
    fn offset(&mut self) -> Result<Offset, IrError> {
        Ok(Offset::new(self.f32()?, self.f32()?))
    }
    fn rect(&mut self) -> Result<Rect, IrError> {
        Ok(Rect {
            left: self.f32()?,
            top: self.f32()?,
            right: self.f32()?,
            bottom: self.f32()?,
        })
    }
    fn color(&mut self) -> Result<Color, IrError> {
        let s = self.take(4)?;
        Ok(Color::rgba(s[0], s[1], s[2], s[3]))
    }
    fn transform(&mut self) -> Result<Transform, IrError> {
        Ok(Transform {
            a: self.f32()?,
            b: self.f32()?,
            c: self.f32()?,
            d: self.f32()?,
            tx: self.f32()?,
            ty: self.f32()?,
        })
    }
    fn opt_f32(&mut self) -> Result<Option<f32>, IrError> {
        Ok(if self.bool()? {
            Some(self.f32()?)
        } else {
            None
        })
    }
    fn path(&mut self) -> Result<Path, IrError> {
        use vieww_foundation::PathVerb;
        let n = self.count()?;
        let mut verbs = Vec::with_capacity(n);
        for _ in 0..n {
            verbs.push(match self.u8()? {
                0 => PathVerb::MoveTo(self.offset()?),
                1 => PathVerb::LineTo(self.offset()?),
                2 => PathVerb::CubicTo(self.offset()?, self.offset()?, self.offset()?),
                3 => PathVerb::Close,
                _ => return Err(IrError::Malformed("path verb")),
            });
        }
        Ok(Path::from_verbs(verbs))
    }
    fn clip(&mut self) -> Result<Clip, IrError> {
        let bounds = if self.bool()? {
            Some(self.rect()?)
        } else {
            None
        };
        let n = self.count()?;
        let mut shapes = Vec::with_capacity(n);
        for _ in 0..n {
            shapes.push(self.path()?);
        }
        Ok(Clip::from_parts(bounds, shapes))
    }
    fn paint(&mut self) -> Result<Paint, IrError> {
        let color = self.color()?;
        let gradient = match self.u8()? {
            0 => None,
            k => {
                let base = match k {
                    1 => Gradient::linear(self.offset()?, self.offset()?),
                    2 => Gradient::radial(self.offset()?, self.f32()?),
                    3 => Gradient::sweep(self.offset()?, self.f32()?, self.f32()?),
                    _ => return Err(IrError::Malformed("gradient kind")),
                };
                let dither = self.bool()?;
                let n = self.count()?;
                if n > vieww_foundation::MAX_GRADIENT_STOPS {
                    return Err(IrError::Malformed("too many gradient stops"));
                }
                let mut stops = Vec::with_capacity(n);
                for _ in 0..n {
                    stops.push((self.f32()?, self.color()?));
                }
                // What `with_stops` requires of its caller: finite offsets,
                // non-decreasing. An encoder only ever writes such stops.
                if stops.iter().any(|(o, _)| !o.is_finite())
                    || stops.windows(2).any(|w| w[0].0 > w[1].0 + 1e-6)
                {
                    return Err(IrError::Malformed("gradient stops out of order"));
                }
                let g = base.with_stops(&stops);
                Some(if dither {
                    g.with_dither()
                } else {
                    g.undithered()
                })
            }
        };
        Ok(Paint { color, gradient })
    }
    fn stroke(&mut self) -> Result<Stroke, IrError> {
        let width = self.f32()?;
        let cap = match self.u8()? {
            0 => StrokeCap::Butt,
            1 => StrokeCap::Round,
            2 => StrokeCap::Square,
            _ => return Err(IrError::Malformed("stroke cap")),
        };
        let join = match self.u8()? {
            0 => StrokeJoin::Miter,
            1 => StrokeJoin::Round,
            2 => StrokeJoin::Bevel,
            _ => return Err(IrError::Malformed("stroke join")),
        };
        let miter_limit = self.f32()?;
        let dash = if self.bool()? {
            let offset = self.f32()?;
            let n = self.count()?;
            let mut pattern = Vec::with_capacity(n);
            for _ in 0..n {
                pattern.push(self.f32()?);
            }
            Some(Dash { pattern, offset })
        } else {
            None
        };
        Ok(Stroke {
            width,
            style: StrokeStyle {
                cap,
                join,
                miter_limit,
                dash,
            },
        })
    }
    fn filter(&mut self) -> Result<ImageFilter, IrError> {
        let blur_sigma = self.f32()?;
        let blur_angle = self.opt_f32()?;
        let color_matrix = if self.bool()? {
            let mut m = [0.0f32; 20];
            for v in &mut m {
                *v = self.f32()?;
            }
            Some(m)
        } else {
            None
        };
        let backdrop = self.bool()?;
        let mut f = ImageFilter::NONE;
        f.blur_sigma = blur_sigma;
        f.blur_angle = blur_angle;
        f.color_matrix = color_matrix;
        f.backdrop = backdrop;
        Ok(f)
    }
}

/// The scene `bytes` encode. Reads [`VERSION`] and every earlier one.
///
/// # Errors
/// [`IrError`]: not IR, too new, truncated or malformed. Never panics on
/// any input (fuzzed in `tests/ir.rs`).
pub fn decode(bytes: &[u8]) -> Result<Scene, IrError> {
    if bytes.len() < 8 || &bytes[..4] != MAGIC {
        return Err(IrError::NotIr);
    }
    let mut r = R {
        b: bytes,
        at: 4,
        fonts: Vec::new(),
        images: Vec::new(),
    };
    let version = r.u16()?;
    if version > VERSION {
        return Err(IrError::TooNew(version));
    }
    if version == 0 {
        return Err(IrError::Malformed("version 0"));
    }
    let _flags = r.u16()?;
    for _ in 0..r.count()? {
        let index = r.u32()?;
        let mut vars = Vec::new();
        for _ in 0..r.count()? {
            let t = r.take(4)?;
            let tag = [t[0], t[1], t[2], t[3]];
            vars.push(FontVariation {
                tag,
                value: r.f32()?,
            });
        }
        let n = r.count()?;
        let data = r.take(n)?.to_vec();
        r.fonts.push(if vars.is_empty() {
            FontData::new(Rc::new(data), index)
        } else {
            FontData::with_variations(Rc::new(data), index, &vars)
        });
    }
    for _ in 0..r.count()? {
        let (w, h) = (r.u32()?, r.u32()?);
        let n = r.count()?;
        if u64::from(w) * u64::from(h) * 4 != n as u64 {
            return Err(IrError::Malformed("image size"));
        }
        let px = r.take(n)?.to_vec();
        r.images.push(Image::from_rgba8(px, w, h));
    }
    let mut scene = Scene::new();
    let mut depth = 0usize;
    for _ in 0..r.count()? {
        let command = match r.u8()? {
            0 => Command::FillRect {
                rect: r.rect()?,
                paint: r.paint()?,
                transform: r.transform()?,
                clip: r.clip()?,
            },
            1 => Command::FillPath {
                path: r.path()?,
                paint: r.paint()?,
                transform: r.transform()?,
                clip: r.clip()?,
            },
            2 => Command::StrokePath {
                path: r.path()?,
                stroke: r.stroke()?,
                paint: r.paint()?,
                transform: r.transform()?,
                clip: r.clip()?,
            },
            3 => {
                let rect = r.rect()?;
                let radius = r.f32()?;
                let shadow = Shadow {
                    color: r.color()?,
                    offset: r.offset()?,
                    blur: r.f32()?,
                    spread: r.f32()?,
                    is_inset: r.bool()?,
                };
                Command::DrawShadow {
                    rect,
                    radius,
                    shadow,
                    transform: r.transform()?,
                    clip: r.clip()?,
                }
            }
            4 => {
                let font = usize::try_from(r.u32()?)
                    .ok()
                    .and_then(|i| r.fonts.get(i).cloned())
                    .ok_or(IrError::Malformed("font index"))?;
                let size = r.f32()?;
                let color = r.color()?;
                let origin = r.offset()?;
                let is_rtl = r.bool()?;
                let ascent = r.f32()?;
                let descent = r.f32()?;
                let n = r.count()?;
                let mut glyphs = Vec::with_capacity(n);
                for _ in 0..n {
                    glyphs.push(Glyph {
                        id: r.u16()?,
                        offset: r.offset()?,
                    });
                }
                Command::DrawGlyphs {
                    run: GlyphRun {
                        font,
                        size,
                        color,
                        origin,
                        glyphs: Rc::from(glyphs),
                        is_rtl,
                        ascent,
                        descent,
                    },
                    transform: r.transform()?,
                    clip: r.clip()?,
                }
            }
            5 => {
                let rect = r.rect()?;
                let image = usize::try_from(r.u32()?)
                    .ok()
                    .and_then(|i| r.images.get(i).cloned())
                    .ok_or(IrError::Malformed("image index"))?;
                Command::DrawImage {
                    rect,
                    image,
                    transform: r.transform()?,
                    clip: r.clip()?,
                }
            }
            6 => {
                depth += 1;
                Command::PushLayer {
                    bounds: r.rect()?,
                    alpha: r.f32()?,
                    blend: *BLENDS
                        .get(usize::from(r.u8()?))
                        .ok_or(IrError::Malformed("blend mode"))?,
                    clip: r.clip()?,
                    filter: r.filter()?,
                }
            }
            7 => {
                depth = depth
                    .checked_sub(1)
                    .ok_or(IrError::Malformed("a PopLayer with no PushLayer"))?;
                Command::PopLayer
            }
            _ => return Err(IrError::Malformed("command tag")),
        };
        scene.push_command(command);
    }
    if depth != 0 {
        return Err(IrError::Malformed("a PushLayer never popped"));
    }
    if r.at != bytes.len() {
        return Err(IrError::Malformed("bytes after the last command"));
    }
    Ok(scene)
}
