//! Lumen — a theme only vieww can draw.
//!
//! Renders one screen twice, headless, at 3× density (1170 × 2532 pixels):
//!
//! - `platform.png` — the same content in vieww's stock controls under
//!   `ThemeData::dark()`, composited the usual way, in gamma space.
//! - `lumen.png` — the Lumen theme: colours specified in Display P3, every
//!   blend done in linear light (`ColorPipeline::LinearLight`), frosted glass
//!   that blurs the real backdrop behind it, hairline rims that stay one
//!   device pixel wide at any density, and every highlight and shadow cast by
//!   **one** light, up and to the left.
//! - `compare.png` — the two side by side.
//!
//! ```console
//! cargo run -p lumen --release -- /tmp/lumen
//! ```
//!
//! Nothing here is a bitmap: the screen is vector all the way down, so the
//! same tree renders exactly as crisply at 4× or 8× — change `DENSITY`.

mod depth;
mod light;

use vieww::prelude::*;
use vieww::FrameDriver;
use vieww_foundation::{
    Border, BoxDecoration, Color, ColorSpace, FontWeight, Gradient, Offset, Shadow, Size,
    TextStyle, Transform,
};
use vieww_paint::native::{ColorPipeline, NativeRenderer};
use vieww_text::FontStore;
use vieww_widget::Theme;

/// The logical canvas: a modern phone in portrait.
const SCREEN: Size = Size::new(390.0, 844.0);
/// Device pixels per logical pixel.
const DENSITY: f32 = 3.0;

// ---- Lumen's tokens ------------------------------------------------------

/// One light, up and to the left. Every shadow falls away from it and every
/// rim catches it — that is what makes the surfaces read as one material.
const LIGHT: Offset = Offset::new(-0.45, -1.0);

/// A Display-P3 colour, mapped for the sRGB PNG this tool writes. On a P3
/// display the framework hands the original to the compositor; here, the
/// nearest sRGB colour is what a screenshot can hold.
fn p3(r: u8, g: u8, b: u8) -> Color {
    Color::p3(r, g, b).converted_to(ColorSpace::Srgb)
}
fn p3a(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::p3a(r, g, b, a).converted_to(ColorSpace::Srgb)
}

const GROUND: Color = Color::rgb(6, 8, 14);
const INK: Color = Color::rgb(240, 243, 250);
const MUTED: Color = Color::rgb(152, 162, 184);
/// A hairline: one device pixel at the density being rendered.
const HAIR: f32 = 1.0 / DENSITY;

fn accent() -> Color {
    p3(124, 92, 255)
}
fn accent_hi() -> Color {
    p3(92, 196, 255)
}

/// A shadow cast by the one light: it falls opposite the light, longer and
/// softer the higher the surface sits.
fn cast(elevation: f32, strength: u8) -> Shadow {
    Shadow::new(
        Color::rgba(0, 0, 0, strength),
        Offset::new(-LIGHT.dx * elevation * 0.5, -LIGHT.dy * elevation),
        elevation * 2.4,
    )
}

/// The rim the light catches along a surface's upper edge.
fn rim(alpha: u8) -> Shadow {
    Shadow::inset(
        Color::rgba(255, 255, 255, alpha),
        Offset::new(0.0, HAIR * 2.0),
        0.0,
    )
}

fn text(s: &str, size: f32, weight: FontWeight, color: Color) -> WidgetNode {
    Text::new(s)
        .style(TextStyle::new(size).weight(weight).color(color))
        .into()
}

/// The field of light behind everything: two wide-gamut glows on the ground,
/// the reason glass has something to bend.
fn lumen_ground() -> WidgetNode {
    Stack::new()
        .fit(StackFit::Expand)
        .push(Positioned::fill().child(Container::new().color(GROUND).child(SizedBox::expand())))
        .push(
            Positioned::new().left(-140.0).top(-120.0).child(
                Container::new()
                    .gradient(
                        Gradient::radial_fill().with_dither().with_stops(&[
                            (0.0, p3a(124, 92, 255, 150)),
                            (1.0, p3a(124, 92, 255, 0)),
                        ]),
                    )
                    .size(520.0, 520.0),
            ),
        )
        .push(
            Positioned::new().left(150.0).top(150.0).child(
                Container::new()
                    .gradient(
                        Gradient::radial_fill().with_dither().with_stops(&[
                            (0.0, p3a(40, 210, 255, 120)),
                            (1.0, p3a(40, 210, 255, 0)),
                        ]),
                    )
                    .size(420.0, 420.0),
            ),
        )
        .push(
            Positioned::new().left(-60.0).top(520.0).child(
                Container::new()
                    .gradient(
                        Gradient::radial_fill().with_dither().with_stops(&[
                            (0.0, p3a(255, 70, 160, 70)),
                            (1.0, p3a(255, 70, 160, 0)),
                        ]),
                    )
                    .size(380.0, 380.0),
            ),
        )
        .into()
}

/// Frosted glass: the real backdrop, blurred, under a sheet lit from the
/// top-left — brighter where the light lands, a hairline rim on top.
fn glass(radius: f32, elevation: f32, child: impl Into<WidgetNode>) -> WidgetNode {
    Container::new()
        .decoration(
            BoxDecoration::new()
                .radius(radius)
                .shadow(cast(elevation, 110)),
        )
        .child(
            Clip::rounded(radius).child(
                Filtered::blur(26.0)
                    .with_backdrop()
                    .tint(GROUND, 0.28)
                    .child(
                        Container::new()
                            .decoration(
                                BoxDecoration::new()
                                    .radius(radius)
                                    .gradient(
                                        Gradient::linear(
                                            Offset::new(0.0, 0.0),
                                            Offset::new(1.0, 1.0),
                                        )
                                        .with_dither()
                                        .with_stops(&[
                                            (0.0, Color::rgba(255, 255, 255, 16)),
                                            (1.0, Color::rgba(255, 255, 255, 3)),
                                        ]),
                                    )
                                    .border(Border::new(Color::rgba(255, 255, 255, 30), HAIR))
                                    .shadow(rim(70)),
                            )
                            .child(child),
                    ),
            ),
        )
        .into()
}

fn lumen_button(label: &str) -> WidgetNode {
    Container::new()
        .decoration(BoxDecoration::new().stadium().shadow(Shadow::new(
            p3a(124, 92, 255, 120),
            Offset::new(-LIGHT.dx * 4.0, -LIGHT.dy * 8.0),
            26.0,
        )))
        .child(
            Container::new()
                .decoration(
                    BoxDecoration::new()
                        .stadium()
                        .gradient(
                            Gradient::linear(Offset::new(0.0, 0.0), Offset::new(1.0, 1.0))
                                .with_dither()
                                .with_stops(&[(0.0, accent()), (1.0, accent_hi())]),
                        )
                        .shadow(rim(120)),
                )
                .height(52.0)
                .alignment(Alignment::CENTER)
                .child(text(label, 16.0, FontWeight::Bold, Color::WHITE)),
        )
        .into()
}

fn lumen_field(label: &str, value: &str, focused: bool) -> WidgetNode {
    let border = if focused {
        Border::new(p3a(124, 92, 255, 230), HAIR * 2.0)
    } else {
        Border::new(Color::rgba(255, 255, 255, 36), HAIR)
    };
    let glow = if focused {
        Shadow::new(p3a(124, 92, 255, 90), Offset::ZERO, 14.0)
    } else {
        Shadow::inset(Color::rgba(0, 0, 0, 120), Offset::new(0.0, 2.0), 6.0)
    };
    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(6.0)
        .children(children![
            text(label, 12.0, FontWeight::Medium, MUTED),
            Container::new()
                .decoration(
                    BoxDecoration::new()
                        .radius(14.0)
                        .color(Color::rgba(0, 0, 0, 70))
                        .border(border)
                        .shadow(glow),
                )
                .height(46.0)
                .padding(EdgeInsets::symmetric(16.0, 0.0))
                .alignment(Alignment::CENTER_LEFT)
                .child(text(value, 15.0, FontWeight::Regular, INK)),
        ])
        .into()
}

fn lumen_toggle(on: bool) -> WidgetNode {
    let track = if on {
        BoxDecoration::new()
            .stadium()
            .gradient(
                Gradient::linear(Offset::new(0.0, 0.0), Offset::new(1.0, 1.0))
                    .with_stops(&[(0.0, accent()), (1.0, accent_hi())]),
            )
            .shadow(rim(110))
    } else {
        BoxDecoration::new()
            .stadium()
            .color(Color::rgba(255, 255, 255, 22))
            .border(Border::new(Color::rgba(255, 255, 255, 40), HAIR))
    };
    Container::new()
        .decoration(track)
        .size(52.0, 32.0)
        .padding(EdgeInsets::all(3.0))
        .alignment(if on {
            Alignment::CENTER_RIGHT
        } else {
            Alignment::CENTER_LEFT
        })
        .child(
            Container::new()
                .decoration(
                    BoxDecoration::new()
                        .stadium()
                        .gradient(
                            Gradient::linear(Offset::new(0.0, 0.0), Offset::new(0.6, 1.0))
                                .with_stops(&[
                                    (0.0, Color::WHITE),
                                    (1.0, Color::rgb(214, 220, 236)),
                                ]),
                        )
                        .shadow(cast(3.0, 120)),
                )
                .size(26.0, 26.0),
        )
        .into()
}

fn lumen_slider(value: f32) -> WidgetNode {
    let width = 310.0;
    Stack::new()
        .push(
            Positioned::new().left(0.0).top(13.0).child(
                Container::new()
                    .decoration(
                        BoxDecoration::new()
                            .stadium()
                            .color(Color::rgba(255, 255, 255, 26))
                            .shadow(Shadow::inset(
                                Color::rgba(0, 0, 0, 140),
                                Offset::new(0.0, 1.0),
                                2.0,
                            )),
                    )
                    .size(width, 6.0),
            ),
        )
        .push(
            Positioned::new().left(0.0).top(13.0).child(
                Container::new()
                    .decoration(BoxDecoration::new().stadium().gradient(
                        Gradient::horizontal().with_stops(&[(0.0, accent()), (1.0, accent_hi())]),
                    ))
                    .size(width * value, 6.0),
            ),
        )
        .push(
            Positioned::new().left(width * value - 14.0).top(2.0).child(
                Container::new()
                    .decoration(
                        BoxDecoration::new()
                            .stadium()
                            .color(Color::WHITE)
                            .shadow(Shadow::new(p3a(92, 196, 255, 150), Offset::ZERO, 12.0)),
                    )
                    .size(28.0, 28.0),
            ),
        )
        .push(SizedBox::from_size(Size::new(width, 32.0)))
        .into()
}

fn lumen_segmented(labels: &[&str], selected: usize) -> WidgetNode {
    let mut row = Flex::row().spacing(4.0);
    for (i, label) in labels.iter().enumerate() {
        let cell = Container::new()
            .height(36.0)
            .alignment(Alignment::CENTER)
            .child(text(
                label,
                14.0,
                FontWeight::Medium,
                if i == selected { INK } else { MUTED },
            ));
        let cell = if i == selected {
            cell.decoration(
                BoxDecoration::new()
                    .radius(11.0)
                    .color(Color::rgba(255, 255, 255, 34))
                    .border(Border::new(Color::rgba(255, 255, 255, 46), HAIR))
                    .shadow(rim(90)),
            )
        } else {
            cell
        };
        row = row.push(Flexible::expanded(1).child(cell));
    }
    glass(14.0, 4.0, Padding::new(EdgeInsets::all(4.0)).child(row))
}

fn lumen_tab_bar(active: usize) -> WidgetNode {
    let tabs = ["Home", "Explore", "Library", "You"];
    let mut row = Flex::row();
    for (i, label) in tabs.iter().enumerate() {
        let on = i == active;
        let dot = Container::new()
            .decoration(if on {
                BoxDecoration::new()
                    .stadium()
                    .gradient(
                        Gradient::horizontal().with_stops(&[(0.0, accent()), (1.0, accent_hi())]),
                    )
                    .shadow(Shadow::new(p3a(124, 92, 255, 160), Offset::ZERO, 10.0))
            } else {
                BoxDecoration::new()
                    .stadium()
                    .color(Color::rgba(255, 255, 255, 90))
            })
            .size(if on { 22.0 } else { 6.0 }, 6.0);
        row = row.push(
            Flexible::expanded(1).child(
                Flex::column()
                    .cross_axis_alignment(CrossAxisAlignment::Center)
                    .spacing(8.0)
                    .children(children![
                        dot,
                        text(
                            label,
                            12.0,
                            FontWeight::Medium,
                            if on { INK } else { MUTED }
                        ),
                    ]),
            ),
        );
    }
    glass(
        26.0,
        10.0,
        Container::new()
            .height(68.0)
            .alignment(Alignment::CENTER)
            .child(row),
    )
}

fn lumen_screen() -> WidgetNode {
    let hero = glass(
        24.0,
        14.0,
        Padding::new(EdgeInsets::all(18.0)).child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .spacing(5.0)
                .children(children![
                    text("TONIGHT", 11.0, FontWeight::Bold, accent_hi()),
                    text("Your evening, in focus", 22.0, FontWeight::Bold, INK),
                    text(
                        "Three things left, one call at 9. Everything else can wait.",
                        14.0,
                        FontWeight::Regular,
                        MUTED,
                    ),
                    SizedBox::height(10.0),
                    lumen_slider(0.62),
                    text("62% of today done", 12.0, FontWeight::Medium, MUTED),
                ]),
        ),
    );

    let settings = glass(
        20.0,
        8.0,
        Padding::new(EdgeInsets::symmetric(18.0, 6.0)).child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .children(children![
                    setting_row("Focus mode", "Silence everything but calls", true),
                    Container::new()
                        .color(Color::rgba(255, 255, 255, 22))
                        .height(HAIR),
                    setting_row("Ambient light", "Follow the time of day", false),
                ]),
        ),
    );

    Stack::new()
        .fit(StackFit::Expand)
        .push(Positioned::fill().child(lumen_ground()))
        .push(
            Positioned::fill().child(
                Padding::new(EdgeInsets::only(20.0, 52.0, 20.0, 0.0)).child(
                    Flex::column()
                        .cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .spacing(12.0)
                        .children(children![
                            text("Good evening, Teja", 30.0, FontWeight::Bold, INK),
                            hero,
                            lumen_segmented(&["Day", "Week", "Month"], 1),
                            lumen_field("Search", "Find anything…", false),
                            lumen_field("Note", "Call the studio about Friday", true),
                            settings,
                            lumen_button("Start focus session"),
                        ]),
                ),
            ),
        )
        .push(
            Positioned::new()
                .left(16.0)
                .right(16.0)
                .bottom(24.0)
                .child(lumen_tab_bar(0)),
        )
        .into()
}

fn setting_row(title: &str, detail: &str, on: bool) -> WidgetNode {
    Container::new()
        .height(54.0)
        .child(
            Flex::row()
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .children(children![
                    Flexible::expanded(1).child(
                        Flex::column()
                            .main_axis_alignment(MainAxisAlignment::Center)
                            .cross_axis_alignment(CrossAxisAlignment::Start)
                            .spacing(3.0)
                            .children(children![
                                text(title, 15.0, FontWeight::Medium, INK),
                                text(detail, 12.0, FontWeight::Regular, MUTED),
                            ]),
                    ),
                    lumen_toggle(on),
                ]),
        )
        .into()
}

// ---- the same screen in vieww's stock controls --------------------------

fn platform_screen() -> WidgetNode {
    let card = |child: WidgetNode| -> WidgetNode {
        Container::new()
            .decoration(
                BoxDecoration::new()
                    .radius(16.0)
                    .color(Color::rgb(30, 33, 40)),
            )
            .padding(EdgeInsets::all(18.0))
            .child(child)
            .into()
    };
    let row = |title: &str, detail: &str, on: bool| -> WidgetNode {
        Container::new()
            .height(58.0)
            .child(
                Flex::row()
                    .cross_axis_alignment(CrossAxisAlignment::Center)
                    .children(children![
                        Flexible::expanded(1).child(
                            Flex::column()
                                .main_axis_alignment(MainAxisAlignment::Center)
                                .cross_axis_alignment(CrossAxisAlignment::Start)
                                .spacing(3.0)
                                .children(children![
                                    text(
                                        title,
                                        15.0,
                                        FontWeight::Medium,
                                        Color::rgb(236, 238, 242)
                                    ),
                                    text(
                                        detail,
                                        12.0,
                                        FontWeight::Regular,
                                        Color::rgb(150, 156, 168)
                                    ),
                                ]),
                        ),
                        Switch::new(on).on_changed(std::rc::Rc::new(|_| {})),
                    ]),
            )
            .into()
    };
    let field = |label: &str, value: &str| -> WidgetNode {
        Flex::column()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .spacing(8.0)
            .children(children![
                text(label, 12.0, FontWeight::Medium, Color::rgb(150, 156, 168)),
                SizedBox::height(48.0).child(TextField::text(value).show_cursor(false)),
            ])
            .into()
    };
    Container::new()
        .color(Color::rgb(18, 20, 24))
        .padding(EdgeInsets::only(20.0, 64.0, 20.0, 0.0))
        .child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .spacing(18.0)
                .children(children![
                    text(
                        "Good evening, Teja",
                        30.0,
                        FontWeight::Bold,
                        Color::rgb(236, 238, 242)
                    ),
                    card(
                        Flex::column()
                            .cross_axis_alignment(CrossAxisAlignment::Start)
                            .spacing(6.0)
                            .children(children![
                                text("TONIGHT", 11.0, FontWeight::Bold, Color::rgb(120, 160, 255)),
                                text(
                                    "Your evening, in focus",
                                    22.0,
                                    FontWeight::Bold,
                                    Color::rgb(236, 238, 242)
                                ),
                                text(
                                    "Three things left, one call at 9. Everything else can wait.",
                                    14.0,
                                    FontWeight::Regular,
                                    Color::rgb(150, 156, 168),
                                ),
                                SizedBox::height(10.0),
                                SizedBox::from_size(Size::new(310.0, 32.0))
                                    .child(Slider::new(0.62).on_changed(std::rc::Rc::new(|_| {}))),
                                text(
                                    "62% of today done",
                                    12.0,
                                    FontWeight::Medium,
                                    Color::rgb(150, 156, 168)
                                ),
                            ])
                            .into(),
                    ),
                    SegmentedControl::new(["Day", "Week", "Month"], 1)
                        .on_selected(std::rc::Rc::new(|_| {})),
                    field("Search", "Find anything…"),
                    field("Note", "Call the studio about Friday"),
                    card(
                        Flex::column()
                            .cross_axis_alignment(CrossAxisAlignment::Stretch)
                            .children(children![
                                row("Focus mode", "Silence everything but calls", true),
                                row("Ambient light", "Follow the time of day", false),
                            ])
                            .into(),
                    ),
                    SizedBox::height(50.0)
                        .child(Button::new("Start focus session").on_pressed(|| {})),
                ]),
        )
        .into()
}

// ---- rendering ---------------------------------------------------------

fn fonts() -> FontStore {
    const REGULAR: &[u8] = include_bytes!("../../u/assets/fonts/Inter-Regular.ttf");
    const MEDIUM: &[u8] = include_bytes!("../../u/assets/fonts/Inter-Medium.ttf");
    const BOLD: &[u8] = include_bytes!("../../u/assets/fonts/Inter-Bold.ttf");
    let mut faces = vec![REGULAR.to_vec(), MEDIUM.to_vec(), BOLD.to_vec()];
    let mono = std::fs::read("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf").ok();
    let has_mono = mono.is_some();
    faces.extend(mono);
    FontStore::with_application_faces(faces, Some("Inter"), has_mono.then_some("DejaVu Sans Mono"))
}

fn render(
    screen: WidgetNode,
    pipeline: ColorPipeline,
    base: Color,
) -> Result<image::RgbaImage, Box<dyn std::error::Error>> {
    render_at(screen, pipeline, base, DENSITY)
}

fn render_at(
    screen: WidgetNode,
    pipeline: ColorPipeline,
    base: Color,
    density: f32,
) -> Result<image::RgbaImage, Box<dyn std::error::Error>> {
    let device = Size::new(SCREEN.width * density, SCREEN.height * density);
    let mut driver = FrameDriver::new(device);
    driver.set_fonts(fonts());
    // The whole tree laid out at the logical size and drawn through one
    // scale — vector all the way down, so the density is a number, not a
    // second set of assets.
    driver.set_root(Theme::new(ThemeData::dark()).child(
        Transformed::new(Transform::scale(density, density)).child(
            Align::new(Alignment::TOP_LEFT).child(SizedBox::from_size(SCREEN).child(screen)),
        ),
    ));
    for frame in 1..=3_u64 {
        driver.draw_frame_at(std::time::Duration::from_millis(16 * frame));
    }
    let mut renderer = NativeRenderer::with_color_pipeline(pipeline);
    let (pixels, _) = renderer.render_to_pixels(
        driver.scene(),
        device.width as u32,
        device.height as u32,
        base,
    )?;
    image::RgbaImage::from_raw(
        device.width as u32,
        device.height as u32,
        pixels.data().to_vec(),
    )
    .ok_or_else(|| "invalid RGBA frame".into())
}

fn render_sized(
    screen: WidgetNode,
    size: Size,
    density: f32,
) -> Result<image::RgbaImage, Box<dyn std::error::Error>> {
    let device = Size::new(size.width * density, size.height * density);
    let mut driver = FrameDriver::new(device);
    driver.set_fonts(fonts());
    driver.set_root(
        Theme::new(ThemeData::dark()).child(
            Transformed::new(Transform::scale(density, density)).child(
                Align::new(Alignment::TOP_LEFT).child(SizedBox::from_size(size).child(screen)),
            ),
        ),
    );
    for frame in 1..=3_u64 {
        driver.draw_frame_at(std::time::Duration::from_millis(16 * frame));
    }
    let mut renderer = NativeRenderer::with_color_pipeline(ColorPipeline::LinearLight);
    let (pixels, _) = renderer.render_to_pixels(
        driver.scene(),
        device.width as u32,
        device.height as u32,
        Color::rgb(4, 5, 8),
    )?;
    image::RgbaImage::from_raw(
        device.width as u32,
        device.height as u32,
        pixels.data().to_vec(),
    )
    .ok_or_else(|| "invalid RGBA frame".into())
}

/// How much light a frame asks an OLED panel to emit, as a percentage of an
/// all-white screen: the mean linear luminance of its pixels. Emission is
/// what an OLED pays for, pixel by pixel, so this is the theme's share of
/// display power — compute (blur, redraws) is a separate cost.
fn emission(image: &image::RgbaImage) -> f64 {
    let lin = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let mut sum = 0.0;
    for p in image.pixels() {
        sum += 0.2126 * lin(p[0]) + 0.7152 * lin(p[1]) + 0.0722 * lin(p[2]);
    }
    100.0 * sum / (image.width() as f64 * image.height() as f64)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "lumen-shots".to_owned());
    let mode = args.next();
    let frames = mode.as_deref() == Some("--frames");
    std::fs::create_dir_all(&out)?;

    if mode.as_deref() == Some("--colors") {
        // Every palette under every light, at 2x, and every text pairing
        // measured against WCAG's 4.5:1.
        let times = [
            ("dawn", 0.06_f32),
            ("noon", 0.5),
            ("dusk", 0.94),
            ("night", 1.5),
        ];
        let mut rows = Vec::new();
        let mut worst = (f32::MAX, String::new());
        println!("lowest text contrast per palette and light (WCAG AA needs 4.5):");
        for pal in light::PALETTES {
            let mut row = Vec::new();
            let mut line = format!("  {:<15}", pal.name);
            for (name, t) in times {
                let l = light::Light::at(t);
                row.push(render_at(
                    light::screen(l, 0.0, pal),
                    ColorPipeline::LinearLight,
                    Color::rgb(4, 5, 8),
                    2.0,
                )?);
                let checks = light::checks(l, pal);
                let low = checks
                    .iter()
                    .min_by(|a, b| a.ratio.total_cmp(&b.ratio))
                    .unwrap();
                line.push_str(&format!(" {name} {:>5.2} ({})", low.ratio, low.place));
                if low.ratio < worst.0 {
                    worst = (low.ratio, format!("{} at {name}: {}", pal.name, low.place));
                }
            }
            println!("{line}");
            rows.push(row);
        }
        println!("worst: {:.2} — {}", worst.0, worst.1);
        let (w, h) = (rows[0][0].width() / 2, rows[0][0].height() / 2);
        let gap = 24;
        let mut sheet = image::RgbaImage::from_pixel(
            (w + gap) * 4 - gap,
            (h + gap) * rows.len() as u32 - gap,
            image::Rgba([255, 255, 255, 255]),
        );
        for (r, row) in rows.iter().enumerate() {
            for (c, img) in row.iter().enumerate() {
                let small =
                    image::imageops::resize(img, w, h, image::imageops::FilterType::Triangle);
                image::imageops::overlay(
                    &mut sheet,
                    &small,
                    i64::from((w + gap) * c as u32),
                    i64::from((h + gap) * r as u32),
                );
            }
        }
        sheet.save(format!("{out}/colors.png"))?;
        println!("colors -> {out}/colors.png");
        return Ok(());
    }

    if mode.as_deref() == Some("--studio") {
        // viewwstudio's shell, rebuilt in the lit theme at its own 1440x900,
        // dark and light.
        let l = light::Light::at(0.94);
        for (name, pal) in [
            ("studio-dark", light::PALETTES[0]),
            ("studio-light", light::PALETTES[1]),
        ] {
            let image = render_sized(light::studio_screen(l, pal), Size::new(1440.0, 900.0), 1.0)?;
            image.save(format!("{out}/{name}.png"))?;
        }
        println!("studio -> {out}");
        return Ok(());
    }

    if mode.as_deref() == Some("--flex") {
        // Developer flexibility: defaults, nudged + custom widget, retuned —
        // dark on top, light below.
        let l = light::Light::at(0.94);
        let mut shots = Vec::new();
        for pal in [light::PALETTES[0], light::PALETTES[1]] {
            for v in 0..3 {
                shots.push(render_at(
                    light::flex_screen(l, 0.0, pal, v),
                    ColorPipeline::LinearLight,
                    Color::rgb(4, 5, 8),
                    2.0,
                )?);
            }
        }
        let (w, h) = (shots[0].width() / 2, shots[0].height() / 2);
        let gap = 24;
        let mut sheet = image::RgbaImage::from_pixel(
            (w + gap) * 3 - gap,
            (h + gap) * 2 - gap,
            image::Rgba([255, 255, 255, 255]),
        );
        for (i, img) in shots.iter().enumerate() {
            let small = image::imageops::resize(img, w, h, image::imageops::FilterType::Triangle);
            image::imageops::overlay(
                &mut sheet,
                &small,
                i64::from((w + gap) * (i as u32 % 3)),
                i64::from((h + gap) * (i as u32 / 3)),
            );
        }
        sheet.save(format!("{out}/flex.png"))?;
        println!("flex -> {out}/flex.png");
        return Ok(());
    }

    if mode.as_deref() == Some("--glimpse") {
        // A photo-sharing app in the same theme: feed, profile, comments,
        // dark and light, in the evening light.
        let l = light::Light::at(0.94);
        let make: [fn(light::Light, f32, light::Palette) -> WidgetNode; 3] = [
            light::glimpse_feed,
            light::glimpse_profile,
            light::glimpse_comments,
        ];
        let mut shots = Vec::new();
        for pal in [light::PALETTES[0], light::PALETTES[1]] {
            for f in make {
                shots.push(render_at(
                    f(l, 0.0, pal),
                    ColorPipeline::LinearLight,
                    Color::rgb(4, 5, 8),
                    2.0,
                )?);
            }
        }
        let (w, h) = (shots[0].width() / 2, shots[0].height() / 2);
        let gap = 24;
        let mut sheet =
            image::RgbaImage::from_pixel((w + gap) * 6 - gap, h, image::Rgba([255, 255, 255, 255]));
        for (i, img) in shots.iter().enumerate() {
            let small = image::imageops::resize(img, w, h, image::imageops::FilterType::Triangle);
            image::imageops::overlay(&mut sheet, &small, i64::from((w + gap) * i as u32), 0);
        }
        sheet.save(format!("{out}/glimpse.png"))?;
        shots[0].save(format!("{out}/glimpse-feed-dark.png"))?;
        shots[4].save(format!("{out}/glimpse-profile-light.png"))?;
        println!("glimpse -> {out}");
        return Ok(());
    }

    if mode.as_deref() == Some("--screens") {
        // The same rules on five different screens, dark and light, in the
        // evening light.
        let l = light::Light::at(0.94);
        let make: [fn(light::Light, f32, light::Palette) -> WidgetNode; 5] = [
            light::screen,
            light::list_screen,
            light::settings_screen,
            light::chat_screen,
            light::dialog_screen,
        ];
        let mut rows = Vec::new();
        for pal in [light::PALETTES[0], light::PALETTES[1]] {
            let mut row = Vec::new();
            for f in make {
                row.push(render_at(
                    f(l, 0.0, pal),
                    ColorPipeline::LinearLight,
                    Color::rgb(4, 5, 8),
                    2.0,
                )?);
            }
            rows.push(row);
        }
        let (w, h) = (rows[0][0].width() / 2, rows[0][0].height() / 2);
        let gap = 24;
        let mut sheet = image::RgbaImage::from_pixel(
            (w + gap) * 5 - gap,
            (h + gap) * 2 - gap,
            image::Rgba([255, 255, 255, 255]),
        );
        for (r, row) in rows.iter().enumerate() {
            for (c, img) in row.iter().enumerate() {
                let small =
                    image::imageops::resize(img, w, h, image::imageops::FilterType::Triangle);
                image::imageops::overlay(
                    &mut sheet,
                    &small,
                    i64::from((w + gap) * c as u32),
                    i64::from((h + gap) * r as u32),
                );
            }
        }
        sheet.save(format!("{out}/screens.png"))?;
        println!("screens -> {out}/screens.png");
        return Ok(());
    }

    if mode.as_deref() == Some("--light") {
        // The same screen at four times of day, then a reel of the sun
        // crossing it (dawn to dusk) and the lamp coming on.
        let times = [
            ("dawn", 0.06_f32),
            ("noon", 0.5),
            ("dusk", 0.94),
            ("night", 1.5),
        ];
        let mut stills = Vec::new();
        for (name, t) in times {
            let image = render(
                light::screen(light::Light::at(t), 0.0, light::PALETTES[0]),
                ColorPipeline::LinearLight,
                Color::rgb(4, 5, 8),
            )?;
            image.save(format!("{out}/light-{name}.png"))?;
            println!("  {name:<6} emitted {:5.2}%", emission(&image));
            stills.push(image);
        }
        let gap = 60;
        let w = stills[0].width();
        let mut sheet = image::RgbaImage::from_pixel(
            w * 4 + gap * 3,
            stills[0].height(),
            image::Rgba([255, 255, 255, 255]),
        );
        for (i, still) in stills.iter().enumerate() {
            image::imageops::overlay(&mut sheet, still, i64::from((w + gap) * i as u32), 0);
        }
        sheet.save(format!("{out}/light-times.png"))?;
        let dir = format!("{out}/light-frames");
        std::fs::create_dir_all(&dir)?;
        let total = 120;
        for i in 0..total {
            let t = if i < 100 { i as f32 / 99.0 } else { 1.5 };
            let tilt = (i as f32 / 40.0 * std::f32::consts::TAU).sin();
            let image = render_at(
                light::screen(light::Light::at(t), tilt, light::PALETTES[0]),
                ColorPipeline::LinearLight,
                Color::rgb(4, 5, 8),
                2.0,
            )?;
            image.save(format!("{dir}/{i:03}.png"))?;
        }
        println!("light -> {out}");
        return Ok(());
    }

    use depth::{Frame, Power};
    let still = Frame {
        tilt: 0.0,
        dialog: 0.0,
        power: Power::Full,
    };

    let shots = [
        (
            "platform",
            render(
                platform_screen(),
                ColorPipeline::GammaSpace,
                Color::rgb(18, 20, 24),
            )?,
        ),
        (
            "lumen",
            render(lumen_screen(), ColorPipeline::LinearLight, GROUND)?,
        ),
        (
            "depth",
            render(
                depth::screen(still),
                ColorPipeline::LinearLight,
                depth::GROUND,
            )?,
        ),
        (
            "depth-tilted",
            render(
                depth::screen(Frame { tilt: 1.0, ..still }),
                ColorPipeline::LinearLight,
                depth::GROUND,
            )?,
        ),
        (
            "depth-dialog",
            render(
                depth::screen(Frame {
                    dialog: 1.0,
                    ..still
                }),
                ColorPipeline::LinearLight,
                depth::GROUND,
            )?,
        ),
        (
            "depth-saver",
            render(
                depth::screen(Frame {
                    power: Power::Saver,
                    ..still
                }),
                ColorPipeline::LinearLight,
                depth::GROUND,
            )?,
        ),
    ];
    println!("emitted light, % of an all-white screen (OLED display power share):");
    for (name, image) in &shots {
        image.save(format!("{out}/{name}.png"))?;
        println!("  {name:<14} {:5.2}%", emission(image));
    }

    let side = |a: &image::RgbaImage, b: &image::RgbaImage, c: &image::RgbaImage| {
        let gap = 60;
        let mut all = image::RgbaImage::from_pixel(
            a.width() * 3 + gap * 2,
            a.height(),
            image::Rgba([255, 255, 255, 255]),
        );
        image::imageops::overlay(&mut all, a, 0, 0);
        image::imageops::overlay(&mut all, b, i64::from(a.width() + gap), 0);
        image::imageops::overlay(&mut all, c, i64::from((a.width() + gap) * 2), 0);
        all
    };
    side(&shots[0].1, &shots[1].1, &shots[2].1).save(format!("{out}/compare.png"))?;
    side(&shots[2].1, &shots[4].1, &shots[5].1).save(format!("{out}/depth-states.png"))?;

    if frames {
        // Two seconds of tilt, then the dialog opening forward: 2x density
        // to keep the reel quick to render.
        let dir = format!("{out}/frames");
        std::fs::create_dir_all(&dir)?;
        let total = 72;
        for i in 0..total {
            let f = i as f32;
            let frame = if i < 48 {
                Frame {
                    tilt: (f / 48.0 * std::f32::consts::TAU).sin(),
                    dialog: 0.0,
                    power: Power::Full,
                }
            } else {
                let t = ((f - 48.0) / 18.0).min(1.0);
                let ease = 1.0 - (1.0 - t).powi(3);
                Frame {
                    tilt: 0.0,
                    dialog: ease,
                    power: Power::Full,
                }
            };
            let image = render_at(
                depth::screen(frame),
                ColorPipeline::LinearLight,
                depth::GROUND,
                2.0,
            )?;
            image.save(format!("{dir}/{i:03}.png"))?;
        }
        println!("frames -> {dir}");
    }
    println!("lumen -> {out}");
    Ok(())
}
