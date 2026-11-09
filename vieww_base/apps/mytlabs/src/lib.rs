//! mytlabs — meet your thoughts labs — the company page, as a vieww widget
//! tree rendered to DOM.
//!
//! # What this is
//!
//! The page that presents the lab, built the way `apps/wrightsite` builds
//! Wright's page and `apps/viewwsite` the studio's: as a vieww widget tree —
//! the same widget catalogue, the same layout algorithm — handed to
//! `vieww-platform-web-dom`, which walks the element tree and emits elements
//! and CSS instead of scene commands. There is no canvas half and no island:
//! everything this page does — scroll, reveal, hover, sticky — a browser
//! already does, so the tree is the whole page and the DOM walk is the whole
//! renderer. [`Styled`] carries the declarations a rasteriser cannot honour
//! and [`Tag`] names the element a box should be; everything else is plain
//! vieww.
//!
//! # Review without a wasm toolchain
//!
//! `examples/snapshot.rs` mounts this tree natively, walks it with the same
//! `vieww-platform-web-dom` the live mount uses, and writes `index.html` with
//! the tree in place of the boot fallback — for screenshots in either colour
//! scheme, for review, and for a crawler. The wasm build (`build-site.sh`) is
//! for the page a person visits.
//!
//! # The palette
//!
//! mytlabs' own, not vieww's and not Wright's. The ground is the indigo night
//! a thought is thought in; the accent is the violet of the spark the lab
//! turns thoughts into; the cyan is a second gradient stop reserved for
//! gradients, the glow, and one syntax colour. Registered with
//! `vieww_platform_web_dom::set_color_tokens`, so every text run and box in
//! these colours is emitted as `var(--ink)` and friends, and `index.html`'s
//! `prefers-color-scheme: light` block re-themes the page without a second
//! palette in Rust. The seven product tints are registered too, with darker
//! light-mode values, so a chip that reads on the night reads on the day.
//!
//! # The mark
//!
//! The thought-trail: three dots ascending, and the spark they become. The
//! lab's whole pitch in one glyph — a thought is not a thing yet, and then,
//! made, it is. Drawn as an SVG data URI (`Styled::empty`), the same trade
//! Wright's W makes: the DOM walk has no `Painting`, and a mark that only
//! existed on a rasterised half would leave the served page logo-less.

use vieww::foundation::{Color, EdgeInsets, FontFamily, TextAlign, TextStyle};
use vieww::prelude::*;
use vieww_platform_web_dom::{Styled, Tag};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

// ─── the palette ──────────────────────────────────────────────────────────

/// The ground — the indigo night a thought is thought in.
const GROUND: Color = Color::rgb(0x0C, 0x0C, 0x16);
/// Page ink, a shade off white so the violet ground can breathe through it.
const INK: Color = Color::rgb(0xEC, 0xEB, 0xF6);
/// The second text colour.
const INK_2: Color = Color::rgb(0xA6, 0xA4, 0xC0);
/// Captions, file names, small print.
const INK_3: Color = Color::rgb(0x73, 0x71, 0x8E);
/// The accent as text on the dark ground, lifted the way Wright lifts its own.
const ACCENT_INK: Color = Color::rgb(0xA7, 0x9B, 0xFF);
/// The live dot — a lab at work.
const LIVE: Color = Color::rgb(0x7F, 0xE8, 0xC8);

/// viewwstudio's tint — violet, the house accent's own sibling.
const VIOLET: Color = ACCENT_INK;
/// wright's tint — cyan, the gradient's second stop.
const CYAN: Color = Color::rgb(0x56, 0xCC, 0xF2);
/// upxcale's tint — mint.
const MINT: Color = LIVE;
/// snapsearch's tint — amber, and the code sample's string colour.
const AMBER: Color = Color::rgb(0xF5, 0xC2, 0x6B);
/// three's tint — rose.
const ROSE: Color = Color::rgb(0xF4, 0x9B, 0xC1);
/// vavlt's tint — peach.
const PEACH: Color = Color::rgb(0xFF, 0xB8, 0x8C);
/// twin's tint — lilac, and the code sample's keyword colour.
const LILAC: Color = Color::rgb(0xC9, 0xB8, 0xFF);

/// The palette as CSS custom properties, for
/// `vieww_platform_web_dom::set_color_tokens`: every text run and box in
/// these colours is written as `var(--name)`, and `index.html`'s
/// `prefers-color-scheme: light` block redefines them — with darker values
/// for the seven product tints, which are washed for tiles but read as text
/// in roles and chips.
pub const COLOR_TOKENS: &[(Color, &str)] = &[
    (GROUND, "ground"),
    (INK, "ink"),
    (INK_2, "ink-2"),
    (INK_3, "ink-3"),
    (ACCENT_INK, "accent"),
    (LIVE, "live"),
    (VIOLET, "violet"),
    (CYAN, "cyan"),
    (MINT, "mint"),
    (AMBER, "amber"),
    (ROSE, "rose"),
    (PEACH, "peach"),
    (LILAC, "lilac"),
];

/// The repository the page links to — the workspace's own canonical value,
/// restated because a page that drifted from the tree it lives in would be
/// advertising a repository nobody builds.
const REPO: &str = "https://github.com/tejakota/vieww_prod";

// ─── the small vocabulary this page is written in ─────────────────────────

fn sans(size: f32, color: Color) -> TextStyle {
    TextStyle::new(size)
        .family(FontFamily::Named("Geist"))
        .color(color)
        .line_height(1.55)
}

fn mono(size: f32, color: Color) -> TextStyle {
    TextStyle::new(size)
        .family(FontFamily::Named("Geist Mono"))
        .color(color)
        .line_height(1.5)
}

fn text(content: &str, style: TextStyle) -> WidgetNode {
    Text::new(content.to_string()).style(style).into()
}

/// A heading, in the semantic element it deserves.
fn heading(level: &'static str, content: &str, size: f32, align: TextAlign) -> WidgetNode {
    Tag::new(
        level,
        Text::new(content.to_string())
            .style(
                sans(size, INK)
                    .weight(FontWeight::Bold)
                    .line_height(1.07)
                    .letter_spacing(size * -0.023),
            )
            .align(align),
    )
    .into()
}

/// The column every section is measured against — `max-w-7xl`, the same
/// measure Wright's page holds itself to.
fn column(child: impl Into<WidgetNode>) -> WidgetNode {
    Styled::css(
        "width:100%;max-width:1280px;margin:0 auto;padding-left:20px;padding-right:20px",
        child,
    )
    .into()
}

/// Fades and lifts as it comes into view — `animation-timeline: view()`: the
/// browser drives it off the scroll position with no observer, no listener
/// and no per-frame work in this application at all.
fn reveal(child: impl Into<WidgetNode>) -> WidgetNode {
    Styled::class("mt-reveal", child).into()
}

fn spacer(height: f32) -> WidgetNode {
    SizedBox::height(height).into()
}

fn col(spacing: f32, children: Vec<WidgetNode>) -> Flex {
    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(spacing)
        .children(children)
}

fn row(spacing: f32, children: Vec<WidgetNode>) -> Flex {
    Flex::row()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .spacing(spacing)
        .children(children)
}

/// A section: the vertical rhythm, and an `id` for the nav to land on.
fn section(id: &'static str, children: Vec<WidgetNode>) -> WidgetNode {
    Tag::new(
        "section",
        Styled::class("mt-section", column(col(0.0, children))),
    )
    .id(id)
    .into()
}

/// The pill above a heading.
fn eyebrow(label: &str) -> WidgetNode {
    Styled::css(
        "display:inline-flex;align-items:center;gap:7px;border-radius:999px;\
         border:1px solid rgba(167,155,255,0.28);background:rgba(167,155,255,0.10);\
         padding:5px 12px;width:fit-content;margin:0 auto",
        row(
            7.0,
            children![
                Container::new()
                    .color(ACCENT_INK)
                    .radius(999.0)
                    .size(5.0, 5.0),
                text(
                    label,
                    mono(10.5, ACCENT_INK)
                        .letter_spacing(1.6)
                        .weight(FontWeight::Medium),
                ),
            ],
        ),
    )
    .into()
}

/// Eyebrow, heading, lead — the block every section opens with.
fn section_head(label: &str, title: &str, lead: &str) -> WidgetNode {
    reveal(
        Flex::column()
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .spacing(0.0)
            .children(children![
                eyebrow(label),
                spacer(18.0),
                Styled::css(
                    "max-width:768px;margin:0 auto",
                    heading("h2", title, 42.0, TextAlign::Center)
                )
                .with("mt-h2"),
                spacer(16.0),
                Styled::css(
                    "max-width:672px;margin:0 auto",
                    Text::new(lead.to_string())
                        .style(sans(17.5, INK_2).line_height(1.62))
                        .align(TextAlign::Center)
                )
                .with("mt-lead"),
            ]),
    )
}

/// The hairline between sections.
fn divider() -> WidgetNode {
    Styled::empty(
        "max-width:1280px;margin:0 auto;width:100%;height:1px;\
         background:linear-gradient(90deg,transparent,rgba(var(--glass),0.09),transparent)",
    )
    .into()
}

/// A small outlined label: a status, a platform, a fact.
fn chip(label: &str) -> WidgetNode {
    Styled::css(
        "border:1px solid rgba(var(--glass),0.14);border-radius:999px;padding:3px 10px;\
         white-space:nowrap",
        text(label, mono(10.0, INK_2).letter_spacing(0.8)),
    )
    .into()
}

// ─── the mark ─────────────────────────────────────────────────────────────

/// The thought-trail's geometry on its 24-unit grid: three dots ascending,
/// then the spark they become. Fixed white — a data URI cannot ask the page
/// for `currentColor`, and the mark is white everywhere it appears: on the
/// tile in the hero, in the nav, in the splash.
fn trail_svg(fill: &str) -> String {
    format!(
        "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'%3E\
         %3Ccircle cx='4.6' cy='18.6' r='1.8' fill='{fill}'/%3E\
         %3Ccircle cx='9.6' cy='14.4' r='2.5' fill='{fill}'/%3E\
         %3Ccircle cx='15.4' cy='9.4' r='3.2' fill='{fill}'/%3E\
         %3Cpath d='M20 0.5 L21.02 2.88 L23.4 3.9 L21.02 4.92 L20 7.3 L18.98 4.92 L16.6 3.9 L18.98 2.88 Z' fill='{fill}'/%3E\
         %3C/svg%3E"
    )
}

/// The spark alone, centred on its own 24-unit grid — each product's corner
/// glyph, in that product's own tint.
fn spark_svg(fill: &str) -> String {
    format!(
        "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'%3E\
         %3Cpath d='M12 2 L15 9 L22 12 L15 15 L12 22 L9 15 L2 12 L9 9 Z' fill='{fill}'/%3E\
         %3C/svg%3E"
    )
}

/// The mark on its tile, the way the nav, the hero and the closing carry it:
/// the violet squircle, radius 30% of the side, the trail at 62% of it. Two
/// background layers — the trail over the gradient — because a gradient and
/// an image are two layers, not one declaration with two images in it.
fn mark(side: f32) -> WidgetNode {
    let radius = side * 0.3;
    let inner = side * 0.62;
    Styled::empty(format!(
        "width:{side}px;height:{side}px;border-radius:{radius}px;\
         background:url(\"{svg}\") center/{inner}px {inner}px no-repeat,\
         linear-gradient(180deg,#8B7CF6,#5A48E0)",
        svg = trail_svg("%23FFFFFF"),
    ))
    .with("mt-mark")
    .into()
}

/// A product's spark on its tinted tile — the corner glyph that stops a grid
/// of cards reading as a wall of paragraphs.
fn product_tile(tint: Color, side: f32) -> WidgetNode {
    let fill = format!("%23{:02X}{:02X}{:02X}", tint.r, tint.g, tint.b);
    let rgb = format!("{},{},{}", tint.r, tint.g, tint.b);
    let spark = (side * 0.5).round();
    let radius = (side * 0.28).round();
    Styled::empty(format!(
        "width:{side}px;height:{side}px;border-radius:{radius}px;flex:none;\
         background:rgba({rgb},0.12) url(\"{svg}\") center/{spark}px {spark}px no-repeat;\
         border:1px solid rgba({rgb},0.30)",
        svg = spark_svg(&fill),
    ))
    .into()
}

// ─── controls ─────────────────────────────────────────────────────────────

fn button(label: &str, href: &str, filled: bool) -> WidgetNode {
    let css = if filled {
        "display:inline-flex;align-items:center;justify-content:center;gap:8px;\
         border-radius:999px;background:linear-gradient(180deg,#8F7FFF,#6C5CE7);\
         color:#FFFFFF;padding:11px 22px;text-decoration:none;font-weight:600;\
         box-shadow:0 8px 24px rgba(124,106,246,0.35),inset 0 1px 0 rgba(255,255,255,0.35);\
         white-space:nowrap"
    } else {
        "display:inline-flex;align-items:center;justify-content:center;gap:8px;\
         border-radius:999px;border:1px solid rgba(var(--glass),0.16);\
         background:rgba(var(--glass),0.04);padding:11px 22px;text-decoration:none;\
         backdrop-filter:blur(8px);white-space:nowrap"
    };
    Tag::link(
        href.to_string(),
        Styled::css(
            css,
            text(
                label,
                sans(14.5, if filled { Color::WHITE } else { INK }).weight(FontWeight::Medium),
            ),
        )
        .with("mt-btn"),
    )
    .into()
}

// ─── the nav ──────────────────────────────────────────────────────────────

/// Sticky, translucent, and blurred over whatever is behind it — the state a
/// browser holds, which is why there is no scroll arithmetic in this tree.
fn nav() -> WidgetNode {
    let link = |label: &str, href: &'static str| -> WidgetNode {
        Tag::link(
            href.to_string(),
            Styled::class(
                "mt-navlink",
                text(label, sans(14.0, INK_2).weight(FontWeight::Medium)),
            ),
        )
        .into()
    };

    Tag::new(
        "nav",
        Styled::class(
            "mt-nav",
            column(Styled::css(
                "height:64px",
                Flex::row()
                    .main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .cross_axis_alignment(CrossAxisAlignment::Center)
                    .children(children![
                        Tag::link(
                            "#top".to_string(),
                            Styled::css(
                                "text-decoration:none",
                                row(
                                    10.0,
                                    children![
                                        mark(22.0),
                                        text("mytlabs", sans(15.0, INK).weight(FontWeight::Bold)),
                                    ],
                                ),
                            ),
                        ),
                        Styled::class(
                            "mt-navlinks",
                            row(
                                28.0,
                                children![
                                    link("Vision", "#vision"),
                                    link("Products", "#products"),
                                    link("Framework", "#framework"),
                                    link("Mission", "#mission"),
                                ],
                            ),
                        ),
                        button("vieww on GitHub", REPO, true),
                    ]),
            )),
        ),
    )
    .into()
}

// ─── the hero ─────────────────────────────────────────────────────────────

/// The company in one breath, under the headline. The words are the lab's
/// own brief: products to use, a framework to create with.
const LEDE: &str = "mytlabs — meet your thoughts labs — builds next-age products \
                    that turn what people imagine into things they can use, and ships \
                    the framework, vieww, so that anyone can create products of \
                    their own.";

fn hero() -> WidgetNode {
    Tag::new(
        "header",
        Styled::class(
            "mt-hero",
            Stack::new().children(children![
                // **The glow, and only the glow** — one violet bloom where the
                // mark sits, and a faint cyan one beside it: the night the
                // page is set in, lit by the spark. Inline rather than in the
                // class so the DOM walk sees `position:absolute` and does not
                // hand it a `grid-area`.
                Styled::empty("position:absolute;inset:0;pointer-events:none")
                    .with("mt-hero-glow"),
                column(
                    Flex::column()
                        .cross_axis_alignment(CrossAxisAlignment::Center)
                        .spacing(0.0)
                        .children(children![
                            Styled::class("mt-rise mt-d0 mt-markglow", mark(84.0)),
                            spacer(24.0),
                            Styled::class("mt-rise mt-d1", status_pill()),
                            spacer(30.0),
                            Styled::css(
                                "max-width:980px;margin:0 auto",
                                Flex::column()
                                    .cross_axis_alignment(CrossAxisAlignment::Center)
                                    .spacing(2.0)
                                    .children(children![
                                        Styled::class(
                                            "mt-h1 mt-rise mt-d2",
                                            heading(
                                                "h1",
                                                "Meet your thoughts.",
                                                68.0,
                                                TextAlign::Center,
                                            ),
                                        ),
                                        // The second sentence in a
                                        // white-to-violet gradient clipped to
                                        // the glyphs — a declaration no
                                        // rasteriser primitive expresses.
                                        Styled::class(
                                            "mt-h1 mt-gradient-text mt-rise mt-d3",
                                            heading(
                                                "div",
                                                "Make them real.",
                                                68.0,
                                                TextAlign::Center,
                                            ),
                                        ),
                                    ]),
                            ),
                            spacer(26.0),
                            Styled::class(
                                "mt-rise mt-d4",
                                Styled::css(
                                    "max-width:680px;margin:0 auto",
                                    Text::new(LEDE.to_string())
                                        .style(sans(19.0, INK_2).line_height(1.6))
                                        .align(TextAlign::Center),
                                )
                                .with("mt-lead-hero"),
                            ),
                            spacer(36.0),
                            Styled::class(
                                "mt-cta mt-rise mt-d5",
                                Flex::row()
                                    .main_axis_size(MainAxisSize::Min)
                                    .main_axis_alignment(MainAxisAlignment::Center)
                                    .spacing(12.0)
                                    .children(children![
                                        button("Explore the products", "#products", true),
                                        button("Create with vieww", "#framework", false),
                                    ]),
                            ),
                            spacer(18.0),
                            Styled::class(
                                "mt-rise mt-d5",
                                Text::new(
                                    "Every product runs on vieww — one Rust framework, one tree, every screen."
                                        .to_string(),
                                )
                                .style(mono(11.5, INK_3))
                                .align(TextAlign::Center),
                            ),
                            spacer(52.0),
                            Styled::class("mt-rise mt-d6", stat_strip()),
                        ]),
                ),
            ]),
        ),
    )
    .id("top")
    .into()
}

fn status_pill() -> WidgetNode {
    Styled::css(
        "display:inline-flex;align-items:center;gap:9px;border-radius:999px;\
         border:1px solid rgba(var(--glass),0.12);background:var(--pill);\
         backdrop-filter:blur(10px);padding:6px 14px;margin:0 auto;width:fit-content;\
         max-width:100%",
        row(
            9.0,
            children![
                Styled::class(
                    "mt-live",
                    Container::new().color(LIVE).radius(999.0).size(7.0, 7.0),
                ),
                text("mytlabs", mono(11.5, INK)),
                text("·", mono(11.5, INK_3)),
                text("meet your thoughts labs", mono(11.5, INK_2)),
            ],
        ),
    )
    .into()
}

/// Four measured numbers, each a claim the repository makes about itself:
/// the crate count read off the workspace members, the products counted off
/// `apps/`, the platforms off the framework README's own table, and the
/// plates off the film lab's own census.
fn stat_strip() -> WidgetNode {
    let cell = |value: &str, note: &str| -> WidgetNode {
        Styled::class(
            "mt-stat",
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .spacing(5.0)
                .children(children![
                    Styled::class(
                        "mt-stat-v",
                        text(value, mono(25.0, INK).weight(FontWeight::Bold)),
                    ),
                    Styled::class(
                        "mt-stat-n",
                        Text::new(note.to_string())
                            .style(sans(11.5, INK_3))
                            .align(TextAlign::Center),
                    ),
                ]),
        )
        .into()
    };
    Styled::css(
        "max-width:800px;margin:0 auto;border:1px solid rgba(var(--glass),0.10);\
         border-radius:16px;overflow:hidden;background:rgba(var(--glass),0.08);\
         box-shadow:0 20px 50px var(--shadow-deep)",
        Styled::class(
            "mt-stats",
            Flex::row().children(children![
                cell("54", "crates, one framework"),
                cell("7", "products from the lab"),
                cell("6", "platforms it runs on"),
                cell("90", "plates rendered by vieww"),
            ]),
        ),
    )
    .into()
}

// ─── the vision ───────────────────────────────────────────────────────────

/// The two answers the lab gives a dream thought, one per card: the product
/// you pick up, and the framework you pick up instead.
fn vision() -> WidgetNode {
    let card = |tag: &str, tint: Color, title: &str, body: &str| -> WidgetNode {
        reveal(Styled::class(
            "mt-card mt-step",
            Container::new().padding(EdgeInsets::all(24.0)).child(col(
                10.0,
                children![
                    row(
                        12.0,
                        children![
                            product_tile(tint, 30.0),
                            text(
                                tag,
                                mono(10.0, tint)
                                    .letter_spacing(1.5)
                                    .weight(FontWeight::Medium),
                            ),
                        ],
                    ),
                    text(
                        title,
                        sans(19.0, INK).weight(FontWeight::Bold).line_height(1.25),
                    ),
                    text(body, sans(14.0, INK_2)),
                ],
            )),
        ))
    };

    section(
        "vision",
        vec![
            section_head(
                "THE VISION",
                "Every person's dream thought, made true.",
                "mytlabs exists for the thought you are carrying — a place, a tool, a world \
                 nobody has built yet. The lab's answer is two-fold, and it is the whole \
                 company: products you can pick up and use, and a framework you can create \
                 with.",
            ),
            spacer(52.0),
            Styled::class(
                "mt-grid2",
                Flex::column().spacing(16.0).children(children![
                    card(
                        "USE IT",
                        MINT,
                        "Pick up a product, made.",
                        "The lab's products are real and ready: a studio that draws screens \
                         and builds them, a workforce of agents that ships projects, and the \
                         apps under them. No blueprint required — the thought someone \
                         already had, shipped.",
                    ),
                    card(
                        "BUILD IT",
                        CYAN,
                        "Or create your own, with vieww.",
                        "Every product in the lab runs on vieww — the lab's own Rust UI \
                         framework, and its other half: one widget tree to every screen. A \
                         framework anyone can create with is how every dream thought gets \
                         its chance.",
                    ),
                ]),
            )
            .into(),
        ],
    )
}

// ─── the products ─────────────────────────────────────────────────────────

/// One product of the lab. The line is the app's own README, abridged to a
/// sentence; the status is the repository's own word for where that app
/// stands — nothing here is presented as available that is not.
struct Product {
    name: &'static str,
    role: &'static str,
    line: &'static str,
    status: &'static str,
    tint: Color,
    href: &'static str,
}

const PRODUCTS: &[Product] = &[
    Product {
        name: "viewwstudio",
        role: "THE STUDIO",
        line: "A desktop editor and device-framed preview for vieww screens: draw a \
               screen, press Render, and rustc builds what you drew.",
        status: "PRODUCT",
        tint: VIOLET,
        href: "https://github.com/tejakota/vieww_prod/tree/main/vieww_base/apps/viewwstudio",
    },
    Product {
        name: "wright",
        role: "THE AGENT WORKFORCE",
        line: "Describe the outcome. Wright drafts a typed blueprint, confirms the plan \
               in plain words, and a workforce of agents builds it in a sandbox — with \
               receipts.",
        status: "v0.5.1",
        tint: CYAN,
        href: "https://github.com/tejakota/vieww_prod/tree/main/vieww_base/apps/wright",
    },
    Product {
        name: "upxcale",
        role: "THE UPSCALER",
        line: "Upscale photographs in a window, through the same rasteriser the \
               framework ships.",
        status: "APP",
        tint: MINT,
        href: "https://github.com/tejakota/vieww_prod/tree/main/apps/upxcale",
    },
    Product {
        name: "snapsearch",
        role: "THE PHOTO FINDER",
        line: "Find a photo by describing it — or by handing it one. Your real photo \
               folder, indexed by embedding similarity.",
        status: "APP",
        tint: AMBER,
        href: "https://github.com/tejakota/vieww_prod/tree/main/apps/snapsearch",
    },
    Product {
        name: "three",
        role: "DEPTH VIDEO, SOCIALLY",
        line: "Moments recorded with measured depth in every frame. Viewers orbit the \
               subject; pinch moves closer in depth, not in zoom.",
        status: "IN THE LAB",
        tint: ROSE,
        href: "https://github.com/tejakota/vieww_prod/tree/main/apps/three",
    },
    Product {
        name: "vavlt",
        role: "THE CONSENT VAULT",
        line: "A consent-first photo and video vault: the engine core, measured on real \
               folders before any phone build.",
        status: "IN THE LAB",
        tint: PEACH,
        href: "https://github.com/tejakota/vieww_prod/tree/main/apps/vavlt",
    },
    Product {
        name: "twin",
        role: "THE DIGITAL LIFE",
        line: "A pixel world with morse code in its pixels. Your avatar is your twin: it \
               lives, learns, and you teach it. End-to-end encrypted.",
        status: "CONCEPT",
        tint: LILAC,
        href: "https://github.com/tejakota/vieww_prod/tree/main/apps/twin",
    },
];

/// One product card. The whole card is the link — a big, honest tap target,
/// and no `<a>` inside an `<a>`, which the DOM would refuse anyway.
fn product_card(product: &Product, featured: bool) -> WidgetNode {
    let tile_side = if featured { 44.0 } else { 36.0 };
    let name_size = if featured { 17.0 } else { 15.5 };
    let line_size = if featured { 14.0 } else { 13.0 };

    reveal(Tag::link(
        product.href.to_string(),
        Styled::class(
            "mt-card mt-pcard",
            Container::new()
                .padding(EdgeInsets::all(if featured { 24.0 } else { 20.0 }))
                .child(col(
                    10.0,
                    children![
                        Flex::row()
                            .cross_axis_alignment(CrossAxisAlignment::Center)
                            .spacing(12.0)
                            .children(children![
                                product_tile(product.tint, tile_side),
                                Flexible::expanded(1).child(
                                    Flex::column()
                                        .cross_axis_alignment(CrossAxisAlignment::Start)
                                        .spacing(2.0)
                                        .children(children![
                                            text(
                                                product.name,
                                                sans(name_size, INK).weight(FontWeight::Bold),
                                            ),
                                            text(
                                                product.role,
                                                mono(9.5, product.tint)
                                                    .letter_spacing(1.4)
                                                    .weight(FontWeight::Medium),
                                            ),
                                        ]),
                                ),
                                chip(product.status),
                            ]),
                        text(product.line, sans(line_size, INK_2).line_height(1.55)),
                        Styled::css(
                            "margin-top:2px",
                            text("source on GitHub", mono(10.5, ACCENT_INK)),
                        ),
                    ],
                )),
        ),
    ))
}

/// Two flagship cards, then the rest — the order the lab itself keeps:
/// the studio and the workforce first, the apps and the explorations after.
fn products() -> WidgetNode {
    let (first, rest) = PRODUCTS.split_at(2);
    let featured: Vec<WidgetNode> = first.iter().map(|p| product_card(p, true)).collect();
    let small: Vec<WidgetNode> = rest.iter().map(|p| product_card(p, false)).collect();

    section(
        "products",
        vec![
            section_head(
                "THE PRODUCTS",
                "Seven thoughts, already real.",
                "Everything under the lab's apps is a product of the same framework — from \
                 the studio that draws screens to the twin that lives in them. The status \
                 on each card is the repository's own word for where that app stands.",
            ),
            spacer(52.0),
            Styled::class("mt-grid2", Flex::column().spacing(16.0).children(featured)).into(),
            spacer(16.0),
            Styled::class("mt-grid3", Flex::column().spacing(16.0).children(small)).into(),
        ],
    )
}

// ─── the framework ────────────────────────────────────────────────────────

/// The guide's first screen, abridged — every line of it real, nothing
/// invented for the page. `docs/guide/getting-started.md` is the source.
const RUST_SAMPLE: &[&str] = &[
    "use vieww::prelude::*;",
    "use vieww_platform_winit::App;",
    "",
    "#[widget]",
    "impl Home {",
    "    fn build(&self, ctx: &BuildContext) -> impl Into<WidgetNode> {",
    "        let theme = ThemeData::of(ctx);",
    "        Container::new()",
    "            .color(theme.colors.surface)",
    "            .alignment(Alignment::CENTER)",
    "            .child(Text::new(\"Hello, my-app!\"))",
    "    }",
    "}",
];

/// The guide's own three lines, in its own order — "a real window with a
/// real widget in it".
const QUICKSTART: &[&str] = &[
    "$ cargo run -p vieww-cli -- new my-app",
    "$ cd my-app",
    "$ cargo run",
];

/// The words coloured as keywords in the sample.
const SYN_KEYWORDS: &[&str] = &[
    "use", "pub", "fn", "struct", "impl", "let", "self", "return", "for", "in", "mut",
];

/// One line of Rust, coloured by the page's own four-token pass: attributes
/// and comments, strings, keywords and capitalised types, and everything
/// else. It highlights *this sample*, not the language — the same honesty
/// Wright's highlighter keeps.
fn rust_spans(line: &str, size: f32) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut index = 0;

    while index < chars.len() {
        let character = chars[index];

        // A comment runs to the end of the line.
        if character == '/' && chars.get(index + 1) == Some(&'/') {
            let rest: String = chars[index..].iter().collect();
            spans.push(Span::new(rest).style(mono(size, INK_3)));
            break;
        }

        // An attribute, to its closing bracket.
        if character == '#' {
            let mut end = index;
            let mut depth = 0;
            while end < chars.len() {
                if chars[end] == '[' {
                    depth += 1;
                } else if chars[end] == ']' {
                    depth -= 1;
                    if depth == 0 {
                        end += 1;
                        break;
                    }
                }
                end += 1;
            }
            let attribute: String = chars[index..end].iter().collect();
            spans.push(Span::new(attribute).style(mono(size, LILAC)));
            index = end;
            continue;
        }

        // A string, quoted.
        if character == '"' {
            let mut end = index + 1;
            while end < chars.len() && chars[end] != '"' {
                end += 1;
            }
            end = (end + 1).min(chars.len());
            let string: String = chars[index..end].iter().collect();
            spans.push(Span::new(string).style(mono(size, AMBER)));
            index = end;
            continue;
        }

        // A word — keyword, type, or plain.
        if character.is_alphabetic() || character == '_' {
            let mut end = index;
            while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
                end += 1;
            }
            let word: String = chars[index..end].iter().collect();
            let color = if SYN_KEYWORDS.contains(&word.as_str()) {
                LILAC
            } else if word.starts_with(char::is_uppercase) {
                CYAN
            } else {
                INK
            };
            spans.push(Span::new(word).style(mono(size, color)));
            index = end;
            continue;
        }

        // A run of punctuation, so a line of `{` and `::` is not six spans.
        let mut end = index;
        while end < chars.len()
            && !chars[end].is_alphanumeric()
            && chars[end] != '_'
            && chars[end] != '"'
            && chars[end] != '#'
            && !(chars[end] == '/' && chars.get(end + 1) == Some(&'/'))
        {
            end += 1;
        }
        let punctuation: String = chars[index..end.max(index + 1)].iter().collect();
        spans.push(Span::new(punctuation).style(mono(size, INK_2)));
        index = end.max(index + 1);
    }

    if spans.is_empty() {
        spans.push(Span::new(" ").style(mono(size, INK_2)));
    }
    spans
}

/// One source sample, tokenised and coloured, in a window's chrome.
fn code_block(filename: &str, lang: &str, source: &[&str]) -> WidgetNode {
    let lines: Vec<WidgetNode> = source
        .iter()
        .enumerate()
        .map(|(index, line)| {
            row(
                0.0,
                children![
                    Styled::css(
                        "width:34px;flex:none;text-align:right;padding-right:16px;\
                         user-select:none;opacity:.45",
                        text(&(index + 1).to_string(), mono(12.5, INK_3)),
                    ),
                    Flexible::expanded(1).child(if line.is_empty() {
                        text(" ", mono(12.5, INK_2))
                    } else {
                        RichText::new(rust_spans(line, 12.5)).into()
                    }),
                ],
            )
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .into()
        })
        .collect();

    Styled::class(
        "mt-code",
        col(
            0.0,
            children![
                Styled::css(
                    "border-bottom:1px solid rgba(var(--glass),0.07);\
                     background:rgba(var(--glass),0.03);padding:11px 14px",
                    Flex::row()
                        .main_axis_alignment(MainAxisAlignment::SpaceBetween)
                        .cross_axis_alignment(CrossAxisAlignment::Center)
                        .children(children![
                            row(
                                8.0,
                                children![
                                    dot(Color::rgb(0xFF, 0x5F, 0x57)),
                                    dot(Color::rgb(0xFE, 0xBC, 0x2E)),
                                    dot(Color::rgb(0x28, 0xC8, 0x40)),
                                    SizedBox::width(6.0),
                                    text(filename, mono(12.0, INK_2)),
                                ],
                            ),
                            Styled::css(
                                "border:1px solid rgba(var(--glass),0.10);border-radius:6px;\
                                 padding:2px 8px",
                                text(lang, mono(10.0, INK_3).letter_spacing(1.0)),
                            ),
                        ]),
                ),
                Styled::css("padding:18px 16px;overflow-x:auto", col(3.0, lines)),
            ],
        ),
    )
    .into()
}

fn dot(color: Color) -> WidgetNode {
    Container::new()
        .color(color)
        .radius(999.0)
        .size(11.0, 11.0)
        .into()
}

/// A terminal block: the `$` in the accent, the command in ink — and a Copy
/// button, because the one thing a reader takes from this section is three
/// commands they are about to type. A real `<button>`, so the keyboard
/// reaches it.
fn terminal(title: &str, lines: &[&str]) -> WidgetNode {
    let rows: Vec<WidgetNode> = lines
        .iter()
        .map(|line| {
            if let Some(rest) = line.strip_prefix("$ ") {
                row(
                    8.0,
                    children![
                        text("$", mono(12.5, ACCENT_INK).weight(FontWeight::Bold)),
                        // In a `Flexible`, so the command is bounded and wraps
                        // rather than measuring intrinsically off the end of
                        // the block.
                        Flexible::expanded(1)
                            .child(Styled::class("mt-cmd", text(rest, mono(12.5, INK)))),
                    ],
                )
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .into()
            } else {
                text(line, mono(12.5, INK_3))
            }
        })
        .collect();

    Styled::class(
        "mt-term",
        col(
            0.0,
            children![
                Styled::css(
                    "border-bottom:1px solid rgba(var(--glass),0.07);\
                     background:rgba(var(--glass),0.03);padding:10px 14px",
                    Flex::row()
                        .main_axis_alignment(MainAxisAlignment::SpaceBetween)
                        .cross_axis_alignment(CrossAxisAlignment::Center)
                        .children(children![
                            text(title, mono(11.5, INK_3)),
                            Tag::new(
                                "button",
                                Styled::class(
                                    "mt-copy",
                                    Styled::css(
                                        "border:1px solid rgba(var(--glass),0.12);\
                                         border-radius:7px;padding:3px 10px;cursor:pointer",
                                        text("Copy", mono(10.5, INK_2)),
                                    ),
                                ),
                            ),
                        ]),
                ),
                Styled::css("padding:16px", col(5.0, rows)),
            ],
        ),
    )
    .into()
}

/// The framework's layering, in the framework README's own words — the
/// `widget -> element -> render -> scene -> graph -> planner -> CPU | GPU`
/// stack, indented the way the README draws it.
fn framework() -> WidgetNode {
    let layers: &[(&str, &str, usize)] = &[
        ("widget", "vieww-widget", 0),
        ("element", "vieww-element", 1),
        ("render", "vieww-render", 2),
        ("scene", "vieww-scene", 3),
        ("render graph", "vieww-render-graph", 4),
        ("planner", "vieww-render-planner", 5),
        (
            "CPU | GPU | Hybrid",
            "vieww-paint · vieww-gpu · vieww-hal",
            6,
        ),
    ];

    let layer_rows: Vec<WidgetNode> = layers
        .iter()
        .map(|(name, crate_name, depth)| {
            Styled::css(
                format!("padding-left:{}px", depth * 16),
                row(
                    10.0,
                    children![
                        text(name, mono(12.5, INK).weight(FontWeight::Medium)),
                        text(crate_name, mono(11.0, INK_3)),
                    ],
                ),
            )
            .into()
        })
        .collect();

    let facts: Vec<WidgetNode> = [
        "54 crates",
        "6 platforms",
        "C-ABI plugins",
        "headless tests",
    ]
    .iter()
    .map(|fact| chip(fact))
    .collect();

    let stack_card = reveal(Styled::class(
        "mt-card",
        Container::new().padding(EdgeInsets::all(22.0)).child(col(
            12.0,
            children![
                text(
                    "THE STACK",
                    mono(10.0, INK_3)
                        .letter_spacing(1.5)
                        .weight(FontWeight::Medium),
                ),
                col(6.0, layer_rows),
                spacer(6.0),
                text(
                    "Each stage depends only on the ones before it, and nothing above \
                     vieww-scene knows whether a frame will be drawn by the CPU \
                     rasteriser or a GPU backend.",
                    sans(13.0, INK_2),
                ),
                spacer(8.0),
                Styled::css("flex-wrap:wrap;gap:8px", row(8.0, facts)),
            ],
        )),
    ));

    section(
        "framework",
        vec![
            section_head(
                "THE FRAMEWORK",
                "Create with vieww.",
                "vieww is a Rust UI framework built as a stack of independently testable \
                 layers — widgets describe intent, elements hold identity across rebuilds, \
                 a scene graph says what to draw, and CPU and GPU backends draw it. The \
                 lab's products are its proof. Your products are its purpose.",
            ),
            spacer(52.0),
            Styled::class(
                "mt-grid2",
                Flex::column().spacing(16.0).children(children![
                    code_block("main.rs", "rust", RUST_SAMPLE),
                    stack_card,
                ]),
            )
            .into(),
            spacer(20.0),
            reveal(terminal(
                "from nothing to a window — the guide's own three lines",
                QUICKSTART,
            )),
            spacer(24.0),
            reveal(Styled::class(
                "mt-cta",
                Flex::row()
                    .main_axis_size(MainAxisSize::Min)
                    .main_axis_alignment(MainAxisAlignment::Center)
                    .spacing(12.0)
                    .children(children![
                        button(
                            "Read the guide",
                            &format!("{REPO}/tree/main/vieww_base/docs/guide"),
                            false,
                        ),
                        button("Read the source", REPO, false),
                    ]),
            )),
        ],
    )
}

// ─── the mission ──────────────────────────────────────────────────────────

/// The three verbs the mission is, one per card: the listening, the
/// building, and the uniting.
fn mission() -> WidgetNode {
    let pillar = |tag: &str, tint: Color, title: &str, body: &str| -> WidgetNode {
        reveal(Styled::class(
            "mt-card mt-step",
            Container::new().padding(EdgeInsets::all(24.0)).child(col(
                10.0,
                children![
                    row(
                        12.0,
                        children![
                            product_tile(tint, 30.0),
                            text(
                                tag,
                                mono(10.0, tint)
                                    .letter_spacing(1.5)
                                    .weight(FontWeight::Medium),
                            ),
                        ],
                    ),
                    text(
                        title,
                        sans(19.0, INK).weight(FontWeight::Bold).line_height(1.25),
                    ),
                    text(body, sans(14.0, INK_2)),
                ],
            )),
        ))
    };

    section(
        "mission",
        vec![
            section_head(
                "THE MISSION",
                "Understand the person. Unite the product.",
                "The lab's work begins by listening — to the customer, their preferences, \
                 their thoughts — and ends with next-age, tech-savvy products that close \
                 the distance between a person and the thing in their hands, until the \
                 two feel like one.",
            ),
            spacer(52.0),
            Styled::class(
                "mt-grid3",
                Flex::column().spacing(16.0).children(children![
                    pillar(
                        "LISTEN",
                        VIOLET,
                        "Understand first.",
                        "Every product starts from how a person actually thinks and what \
                         they prefer. The customer's thoughts are the brief, and the lab \
                         reads them before it writes anything.",
                    ),
                    pillar(
                        "BUILD",
                        CYAN,
                        "Next-age, tech-savvy.",
                        "Built on a framework that renders its own release films and tests \
                         itself headless — products that feel like the future because \
                         they are made of it.",
                    ),
                    pillar(
                        "UNITE",
                        MINT,
                        "One experience.",
                        "The product and the person on the same side: preferences shape \
                         it, thoughts steer it, and using it feels made for you — because \
                         it was.",
                    ),
                ]),
            )
            .into(),
        ],
    )
}

// ─── the closing call ─────────────────────────────────────────────────────

fn closing() -> WidgetNode {
    reveal(Styled::class(
        "mt-closing",
        Container::new().child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .spacing(0.0)
                .children(children![
                    mark(44.0),
                    spacer(22.0),
                    Styled::class(
                        "mt-h2",
                        heading("div", "Meet your thoughts.", 40.0, TextAlign::Center),
                    ),
                    Styled::class(
                        "mt-h2 mt-gradient-text",
                        heading("div", "Make them real.", 40.0, TextAlign::Center),
                    ),
                    spacer(16.0),
                    Styled::css(
                        "max-width:560px;margin:0 auto",
                        Text::new(
                            "mytlabs — meet your thoughts labs. Every product in the lab \
                             is a thought someone had, made real. The next one is yours."
                                .to_string(),
                        )
                        .style(sans(14.0, INK_2).line_height(1.6))
                        .align(TextAlign::Center),
                    ),
                ]),
        ),
    ))
}

// ─── the footer ───────────────────────────────────────────────────────────

fn footer() -> WidgetNode {
    let group = |title: &str, links: Vec<(&str, String)>| -> WidgetNode {
        col(
            10.0,
            std::iter::once(text(
                title,
                mono(10.0, INK_3)
                    .letter_spacing(1.6)
                    .weight(FontWeight::Medium),
            ))
            .chain(links.into_iter().map(|(label, href)| {
                Tag::link(
                    href,
                    Styled::class("mt-foot", text(label, sans(14.0, INK_2))),
                )
                .into()
            }))
            .collect(),
        )
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .into()
    };

    Tag::new(
        "footer",
        Styled::css(
            "border-top:1px solid rgba(var(--glass),0.08);padding:56px 0 44px",
            column(col(
                40.0,
                children![
                    Styled::class(
                        "mt-footgrid",
                        Flex::row()
                            .cross_axis_alignment(CrossAxisAlignment::Start)
                            .children(children![
                                col(
                                    12.0,
                                    children![
                                        row(
                                            10.0,
                                            children![
                                                mark(24.0),
                                                text(
                                                    "mytlabs",
                                                    sans(15.0, INK).weight(FontWeight::Bold),
                                                ),
                                            ],
                                        ),
                                        Styled::css(
                                            "max-width:260px",
                                            text(
                                                "meet your thoughts labs — products anyone \
                                                 can use, and a framework anyone can create \
                                                 with.",
                                                sans(13.5, INK_3),
                                            ),
                                        ),
                                    ],
                                )
                                .cross_axis_alignment(CrossAxisAlignment::Start),
                                group(
                                    "PRODUCTS",
                                    PRODUCTS
                                        .iter()
                                        .map(|product| {
                                            (product.name, product.href.to_string())
                                        })
                                        .collect(),
                                ),
                                group(
                                    "THE LAB",
                                    vec![
                                        ("Vision", "#vision".into()),
                                        ("Products", "#products".into()),
                                        ("Framework", "#framework".into()),
                                        ("Mission", "#mission".into()),
                                    ],
                                ),
                                group(
                                    "VIEWW",
                                    vec![
                                        ("Source", REPO.into()),
                                        (
                                            "The guide",
                                            format!("{REPO}/tree/main/vieww_base/docs/guide"),
                                        ),
                                        (
                                            "The apps",
                                            format!("{REPO}/tree/main/vieww_base/apps"),
                                        ),
                                        ("The film lab", format!("{REPO}/tree/main/film_lab")),
                                    ],
                                ),
                            ]),
                    ),
                    Styled::css(
                        "border-top:1px solid rgba(var(--glass),0.06);padding-top:22px",
                        Styled::class(
                            "mt-footbottom",
                            Flex::row()
                                .main_axis_alignment(MainAxisAlignment::SpaceBetween)
                                .cross_axis_alignment(CrossAxisAlignment::Center)
                                .children(children![
                                    text("© 2026 mytlabs — meet your thoughts labs", mono(11.5, INK_3)),
                                    text(
                                        "Built with vieww. This page is a vieww widget tree, rendered to DOM.",
                                        mono(11.5, INK_3),
                                    ),
                                ]),
                        ),
                    ),
                ],
            )),
        ),
    )
    .into()
}

// ─── the whole page ───────────────────────────────────────────────────────

/// The whole thing: nav, hero, the four sections, the closing call, the
/// footer. One tree, one walk, one page.
#[must_use]
pub fn page() -> WidgetNode {
    let main = col(
        0.0,
        vec![
            vision(),
            divider(),
            products(),
            divider(),
            framework(),
            divider(),
            mission(),
            divider(),
            closing(),
        ],
    );

    Styled::css(
        "background:var(--ground);color:var(--ink);min-height:100vh",
        col(
            0.0,
            vec![nav(), hero(), Tag::new("main", main).into(), footer()],
        ),
    )
    .into()
}

// ─── the entry point ──────────────────────────────────────────────────────

/// Mount the page on the DOM.
///
/// Called by the browser through wasm-bindgen's start shim, so the host page
/// is one `<script type="module">` and nothing else. **DOM, not canvas**: the
/// page is a vieww widget tree either way; this hands it to
/// `vieww-platform-web-dom`, which walks the element tree and emits elements
/// and CSS instead of scene commands.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() -> Result<(), wasm_bindgen::JsValue> {
    vieww_platform_web_dom::set_color_tokens(COLOR_TOKENS);
    vieww_platform_web_dom::DomApp::mount("app", page())
        .map(std::mem::forget)
        .map_err(|error| error.to_string())?;
    Ok(())
}
