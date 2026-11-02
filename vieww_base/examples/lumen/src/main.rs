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
    Shadow::inset(Color::rgba(255, 255, 255, alpha), Offset::new(0.0, HAIR * 2.0), 0.0)
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
                        Gradient::radial_fill()
                            .with_dither()
                            .with_stops(&[(0.0, p3a(124, 92, 255, 150)), (1.0, p3a(124, 92, 255, 0))]),
                    )
                    .size(520.0, 520.0),
            ),
        )
        .push(
            Positioned::new().left(150.0).top(150.0).child(
                Container::new()
                    .gradient(
                        Gradient::radial_fill()
                            .with_dither()
                            .with_stops(&[(0.0, p3a(40, 210, 255, 120)), (1.0, p3a(40, 210, 255, 0))]),
                    )
                    .size(420.0, 420.0),
            ),
        )
        .push(
            Positioned::new().left(-60.0).top(520.0).child(
                Container::new()
                    .gradient(
                        Gradient::radial_fill()
                            .with_dither()
                            .with_stops(&[(0.0, p3a(255, 70, 160, 70)), (1.0, p3a(255, 70, 160, 0))]),
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
        .decoration(BoxDecoration::new().radius(radius).shadow(cast(elevation, 110)))
        .child(
            Clip::rounded(radius).child(
                Filtered::blur(26.0).with_backdrop().tint(GROUND, 0.28).child(
                    Container::new()
                        .decoration(
                            BoxDecoration::new()
                                .radius(radius)
                                .gradient(
                                    Gradient::linear(Offset::new(0.0, 0.0), Offset::new(1.0, 1.0))
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
        .decoration(
            BoxDecoration::new()
                .stadium()
                .shadow(Shadow::new(
                    p3a(124, 92, 255, 120),
                    Offset::new(-LIGHT.dx * 4.0, -LIGHT.dy * 8.0),
                    26.0,
                )),
        )
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
        .alignment(if on { Alignment::CENTER_RIGHT } else { Alignment::CENTER_LEFT })
        .child(
            Container::new()
                .decoration(
                    BoxDecoration::new()
                        .stadium()
                        .gradient(
                            Gradient::linear(Offset::new(0.0, 0.0), Offset::new(0.6, 1.0))
                                .with_stops(&[(0.0, Color::WHITE), (1.0, Color::rgb(214, 220, 236))]),
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
                            .shadow(Shadow::inset(Color::rgba(0, 0, 0, 140), Offset::new(0.0, 1.0), 2.0)),
                    )
                    .size(width, 6.0),
            ),
        )
        .push(
            Positioned::new().left(0.0).top(13.0).child(
                Container::new()
                    .decoration(
                        BoxDecoration::new().stadium().gradient(
                            Gradient::horizontal().with_stops(&[(0.0, accent()), (1.0, accent_hi())]),
                        ),
                    )
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
            .child(text(label, 14.0, FontWeight::Medium, if i == selected { INK } else { MUTED }));
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
                    .gradient(Gradient::horizontal().with_stops(&[(0.0, accent()), (1.0, accent_hi())]))
                    .shadow(Shadow::new(p3a(124, 92, 255, 160), Offset::ZERO, 10.0))
            } else {
                BoxDecoration::new().stadium().color(Color::rgba(255, 255, 255, 90))
            })
            .size(if on { 22.0 } else { 6.0 }, 6.0);
        row = row.push(Flexible::expanded(1).child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .spacing(8.0)
                .children(children![
                    dot,
                    text(label, 12.0, FontWeight::Medium, if on { INK } else { MUTED }),
                ]),
        ));
    }
    glass(26.0, 10.0, Container::new().height(68.0).alignment(Alignment::CENTER).child(row))
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
                    Container::new().color(Color::rgba(255, 255, 255, 22)).height(HAIR),
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
        .push(Positioned::new().left(16.0).right(16.0).bottom(24.0).child(lumen_tab_bar(0)))
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
            .decoration(BoxDecoration::new().radius(16.0).color(Color::rgb(30, 33, 40)))
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
                                    text(title, 15.0, FontWeight::Medium, Color::rgb(236, 238, 242)),
                                    text(detail, 12.0, FontWeight::Regular, Color::rgb(150, 156, 168)),
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
                    text("Good evening, Teja", 30.0, FontWeight::Bold, Color::rgb(236, 238, 242)),
                    card(
                        Flex::column()
                            .cross_axis_alignment(CrossAxisAlignment::Start)
                            .spacing(6.0)
                            .children(children![
                                text("TONIGHT", 11.0, FontWeight::Bold, Color::rgb(120, 160, 255)),
                                text("Your evening, in focus", 22.0, FontWeight::Bold, Color::rgb(236, 238, 242)),
                                text(
                                    "Three things left, one call at 9. Everything else can wait.",
                                    14.0,
                                    FontWeight::Regular,
                                    Color::rgb(150, 156, 168),
                                ),
                                SizedBox::height(10.0),
                                SizedBox::from_size(Size::new(310.0, 32.0)).child(Slider::new(0.62).on_changed(std::rc::Rc::new(|_| {}))),
                                text("62% of today done", 12.0, FontWeight::Medium, Color::rgb(150, 156, 168)),
                            ])
                            .into(),
                    ),
                    SegmentedControl::new(["Day", "Week", "Month"], 1).on_selected(std::rc::Rc::new(|_| {})),
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
                    SizedBox::height(50.0).child(Button::new("Start focus session").on_pressed(|| {})),
                ]),
        )
        .into()
}

// ---- rendering ---------------------------------------------------------

fn fonts() -> FontStore {
    const REGULAR: &[u8] = include_bytes!("../../u/assets/fonts/Inter-Regular.ttf");
    const MEDIUM: &[u8] = include_bytes!("../../u/assets/fonts/Inter-Medium.ttf");
    const BOLD: &[u8] = include_bytes!("../../u/assets/fonts/Inter-Bold.ttf");
    FontStore::with_application_faces(
        [REGULAR.to_vec(), MEDIUM.to_vec(), BOLD.to_vec()],
        Some("Inter"),
        None,
    )
}

fn render(
    screen: WidgetNode,
    pipeline: ColorPipeline,
    base: Color,
) -> Result<image::RgbaImage, Box<dyn std::error::Error>> {
    let device = Size::new(SCREEN.width * DENSITY, SCREEN.height * DENSITY);
    let mut driver = FrameDriver::new(device);
    driver.set_fonts(fonts());
    // The whole tree laid out at the logical size and drawn through one
    // scale — vector all the way down, so the density is a number, not a
    // second set of assets.
    driver.set_root(
        Theme::new(ThemeData::dark()).child(
            Transformed::new(Transform::scale(DENSITY, DENSITY)).child(
                Align::new(Alignment::TOP_LEFT).child(SizedBox::from_size(SCREEN).child(screen)),
            ),
        ),
    );
    for frame in 1..=3_u64 {
        driver.draw_frame_at(std::time::Duration::from_millis(16 * frame));
    }
    let mut renderer = NativeRenderer::with_color_pipeline(pipeline);
    let (pixels, _) =
        renderer.render_to_pixels(driver.scene(), device.width as u32, device.height as u32, base)?;
    image::RgbaImage::from_raw(device.width as u32, device.height as u32, pixels.data().to_vec())
        .ok_or_else(|| "invalid RGBA frame".into())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "lumen-shots".to_owned());
    std::fs::create_dir_all(&out)?;

    let platform = render(platform_screen(), ColorPipeline::GammaSpace, Color::rgb(18, 20, 24))?;
    let lumen = render(lumen_screen(), ColorPipeline::LinearLight, GROUND)?;
    platform.save(format!("{out}/platform.png"))?;
    lumen.save(format!("{out}/lumen.png"))?;

    let gap = 60;
    let mut both = image::RgbaImage::from_pixel(
        platform.width() * 2 + gap,
        platform.height(),
        image::Rgba([255, 255, 255, 255]),
    );
    image::imageops::overlay(&mut both, &platform, 0, 0);
    image::imageops::overlay(&mut both, &lumen, i64::from(platform.width() + gap), 0);
    both.save(format!("{out}/compare.png"))?;
    println!("lumen -> {out}/{{platform,lumen,compare}}.png");
    Ok(())
}
