//! The shared token layer and component kit (UX-01).
//!
//! Every app used to hand-roll its buttons, chips, cards, badges and toasts
//! from raw colours. This crate is the one place that spelling lives: a
//! [`Skin`] is an app's *semantic* tokens (what is the accent, what is a
//! panel, how round is a chip), and the components read only the skin — never
//! a colour literal. An app restyles by building a different `Skin`; it never
//! forks a component.
//!
//! # Byte-stable by construction
//!
//! The components are the apps' own widgets lifted out, with every number that
//! differed between apps made a skin field. An app migrating to the kit builds
//! its skin from the values it already used, and its golden screenshots do not
//! change — that is the acceptance test, and each migration checked it.
//!
//! # Not here
//!
//! Sheets, tiles and the photo grid are still per-app: they differ in
//! behaviour, not just in colour, and a kit that hid the difference would be
//! a worse component for both.

use std::rc::Rc;

use vieww_foundation::{Border, Color, EdgeInsets, FontWeight, Shadow, TextStyle};
use vieww_widget::prelude::*;
use vieww_widget::{Pressable, Semantics};

/// An app's semantic tokens. Plain data: build one next to the app's theme.
#[derive(Debug, Clone, PartialEq)]
pub struct Skin {
    /// The page colour.
    pub ground: Color,
    /// A raised surface: a card, a quiet button.
    pub panel: Color,
    /// The 1 px line around surfaces.
    pub edge: Color,
    /// The action colour, and the tint behind a primary button.
    pub accent: Color,
    pub accent_tint: Color,
    /// Text on [`ground`](Self::ground) and [`panel`](Self::panel).
    pub ink: Color,
    pub muted: Color,
    /// A destructive action: fill, label, edge.
    pub danger: (Color, Color, Color),
    /// Corner radius of buttons and cards; chips and badges have their own.
    pub radius: f32,
    pub chip_radius: f32,
    pub badge_radius: f32,
    /// Button padding (horizontal, vertical) and its label size.
    pub button_pad: (f32, f32),
    pub button_text: f32,
    /// Card padding (horizontal, vertical).
    pub card_pad: (f32, f32),
    /// Chip: padding, minimum height (the touch target), label size/spacing.
    pub chip_pad: (f32, f32),
    pub chip_height: f32,
    pub chip_text: (f32, f32),
    /// Badge: padding, label size, letter spacing.
    pub badge_pad: (f32, f32),
    pub badge_text: (f32, f32),
    /// How much a press dims a control (0 = not at all).
    pub press_dim: f32,
    /// Toast: fill, label colour, padding, label size/weight, widest line, shadow.
    pub toast_fill: Color,
    pub toast_ink: Color,
    pub toast_pad: (f32, f32),
    pub toast_text: (f32, FontWeight),
    pub toast_max_width: f32,
    pub toast_shadow: Option<Shadow>,
    /// Icon button: the drawn disc's diameter and the square touch target
    /// around it (DESIGN_SPEC §3 `IconButton`: 40 | 44 | 48).
    pub icon_button: (f32, f32),
    /// Callout: padding, and the title / body label sizes.
    pub callout_pad: (f32, f32),
    pub callout_text: (f32, f32),
}

impl Skin {
    /// A starting skin from the six colours; every metric takes the kit's
    /// default and is then overridden with struct-update syntax
    /// (`Skin { toast_pad: (16.0, 10.0), ..Skin::base(..) }`).
    #[must_use]
    pub fn base(
        ground: Color,
        panel: Color,
        edge: Color,
        accent: Color,
        ink: Color,
        muted: Color,
    ) -> Self {
        Self {
            ground,
            panel,
            edge,
            accent,
            accent_tint: accent.with_alpha(26),
            ink,
            muted,
            danger: (panel, Color::rgb(255, 138, 148), edge),
            radius: 8.0,
            chip_radius: 10.0,
            badge_radius: 6.0,
            button_pad: (14.0, 12.0),
            button_text: 14.0,
            card_pad: (14.0, 12.0),
            chip_pad: (12.0, 8.0),
            chip_height: 44.0,
            chip_text: (12.0, 0.2),
            badge_pad: (6.0, 3.0),
            badge_text: (11.0, 0.8),
            press_dim: 0.3,
            toast_fill: panel,
            toast_ink: ink,
            toast_pad: (16.0, 10.0),
            toast_text: (13.0, FontWeight::Medium),
            toast_max_width: 310.0,
            toast_shadow: None,
            icon_button: (28.0, 44.0),
            callout_pad: (12.0, 10.0),
            callout_text: (13.0, 12.0),
        }
    }
}

/// The kinds of [`button`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The one action on the screen: accent tint, accent label.
    Primary,
    /// Everything else: a panel and an edge.
    Quiet,
    /// Solid accent, ground-coloured label (a send button).
    Solid,
    /// Destructive.
    Danger,
}

/// The smallest text the kit sets (DESIGN_SPEC §1.2).
pub const MIN_TEXT: f32 = 11.0;

fn padded(pad: (f32, f32)) -> EdgeInsets {
    EdgeInsets::symmetric(pad.0, pad.1)
}

fn label_style(size: f32, weight: FontWeight, color: Color, spacing: f32) -> TextStyle {
    TextStyle::new(size)
        .weight(weight)
        .color(color)
        .letter_spacing(spacing)
}

fn dimmed(skin: &Skin, press: f32) -> f32 {
    1.0 - skin.press_dim * press
}

/// A flat rounded box: fill, 1 px edge.
fn flat(fill: Color, edge: Color, radius: f32, pad: EdgeInsets, child: WidgetNode) -> WidgetNode {
    Container::new()
        .decoration(
            BoxDecoration::new()
                .color(fill)
                .radius(radius)
                .border(Border::new(edge, 1.0)),
        )
        .padding(pad)
        .child(child)
        .into()
}

/// A tappable button: a tab stop with the theme's focus ring, dimmed while
/// pressed.
#[must_use]
pub fn button(skin: &Skin, label: &str, kind: Kind, on_tap: impl Fn() + 'static) -> WidgetNode {
    let skin = skin.clone();
    let label = label.to_owned();
    let (fill, ink, edge) = match kind {
        Kind::Primary => (skin.accent_tint, skin.accent, skin.accent.with_alpha(110)),
        Kind::Quiet => (skin.panel, skin.ink, skin.edge),
        Kind::Solid => (skin.accent, skin.ground, skin.accent),
        Kind::Danger => skin.danger,
    };
    Pressable::new(move |press| {
        Opacity::new(dimmed(&skin, press))
            .child(flat(
                fill,
                edge,
                skin.radius,
                padded(skin.button_pad),
                Center::new()
                    .child(Text::new(label.clone()).style(label_style(
                        skin.button_text,
                        FontWeight::Medium,
                        ink,
                        0.0,
                    )))
                    .into(),
            ))
            .into()
    })
    .on_tap(on_tap)
    .into()
}

/// A raised container.
#[must_use]
pub fn card(skin: &Skin, child: WidgetNode) -> WidgetNode {
    flat(
        skin.panel,
        skin.edge,
        skin.radius,
        padded(skin.card_pad),
        child,
    )
}

/// A rounded, tappable label in `color`, with a touch-sized height.
#[must_use]
pub fn chip(skin: &Skin, label: &str, color: Color, on_tap: Rc<dyn Fn()>) -> WidgetNode {
    let skin = skin.clone();
    let label = label.to_owned();
    Pressable::new(move |press| {
        Opacity::new(dimmed(&skin, press))
            .child(
                SizedBox::height(skin.chip_height).child(
                    Center::new().child(
                        Container::new()
                            .decoration(
                                BoxDecoration::new()
                                    .color(color.with_alpha(26))
                                    .radius(skin.chip_radius)
                                    .border(Border::new(color.with_alpha(76), 1.0)),
                            )
                            .padding(padded(skin.chip_pad))
                            .child(Text::new(label.clone()).style(label_style(
                                skin.chip_text.0,
                                FontWeight::Medium,
                                color,
                                skin.chip_text.1,
                            ))),
                    ),
                ),
            )
            .into()
    })
    .on_tap(move || on_tap())
    .into()
}

/// A small tinted tag: a kind, a status.
#[must_use]
pub fn badge(skin: &Skin, label: &str, color: Color) -> WidgetNode {
    Container::new()
        .decoration(
            BoxDecoration::new()
                .color(color.with_alpha(40))
                .radius(skin.badge_radius),
        )
        .padding(padded(skin.badge_pad))
        .child(Text::new(label.to_owned()).style(label_style(
            skin.badge_text.0,
            FontWeight::Bold,
            color,
            skin.badge_text.1,
        )))
        .into()
}

/// A small tracked-capitals heading above a group (the pixel face's stand-in
/// where an app has no pixel font). At the 11 px floor (DESIGN_SPEC §1.2).
#[must_use]
pub fn section_label(skin: &Skin, text: &str) -> WidgetNode {
    Text::new(text.to_uppercase())
        .style(label_style(MIN_TEXT, FontWeight::Bold, skin.muted, 2.4))
        .into()
}

/// A round icon control with a label for screen readers (DESIGN_SPEC §3
/// `IconButton`): a `fill`-tinted disc of [`Skin::icon_button`]'s diameter
/// inside a square touch target, a tab stop that dims while pressed.
#[must_use]
pub fn icon_button(
    skin: &Skin,
    label: &str,
    icon: WidgetNode,
    fill: Color,
    on_tap: impl Fn() + 'static,
) -> WidgetNode {
    let skin = skin.clone();
    let (disc, target) = skin.icon_button;
    Semantics::button(label.to_owned())
        .child(
            Pressable::new(move |press| {
                Opacity::new(dimmed(&skin, press))
                    .child(
                        SizedBox::square(target).child(
                            Center::new().child(
                                Container::new()
                                    .decoration(BoxDecoration::new().color(fill).radius(disc / 2.0))
                                    .width(disc)
                                    .height(disc)
                                    .child(Center::new().child(icon.clone())),
                            ),
                        ),
                    )
                    .into()
            })
            .on_tap(on_tap),
        )
        .into()
}

/// A tinted, edged card that says something in `color`: an alert, a warning,
/// a status the reader must not miss. A title in `color` and a line in ink —
/// colour is never the only signal (DESIGN_SPEC §1.1 Status).
#[must_use]
pub fn callout(skin: &Skin, color: Color, title: &str, body: &str) -> WidgetNode {
    Container::new()
        .decoration(
            BoxDecoration::new()
                .color(color.with_alpha(34))
                .radius(skin.radius)
                .border(Border::new(color.with_alpha(150), 1.0)),
        )
        .padding(padded(skin.callout_pad))
        .child(
            Flex::column()
                .spacing(3.0)
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .children(vec![
                    Text::new(title.to_owned())
                        .style(label_style(
                            skin.callout_text.0,
                            FontWeight::Bold,
                            color,
                            0.2,
                        ))
                        .into(),
                    Text::new(body.to_owned())
                        .style(label_style(
                            skin.callout_text.1,
                            FontWeight::Medium,
                            skin.ink,
                            0.1,
                        ))
                        .into(),
                ]),
        )
        .into()
}

/// A pill that hugs its message and wraps a long one into a lozenge.
#[must_use]
pub fn toast(skin: &Skin, message: &str) -> WidgetNode {
    let mut pill = Container::new()
        .decoration(BoxDecoration::new().color(skin.toast_fill).stadium())
        .padding(padded(skin.toast_pad));
    if let Some(shadow) = skin.toast_shadow {
        pill = pill.shadow(shadow);
    }
    Flex::row()
        .main_axis_size(MainAxisSize::Min)
        .children(vec![pill
            .child(
                Constrained::new(Constraints::new(
                    0.0,
                    skin.toast_max_width,
                    0.0,
                    f32::INFINITY,
                ))
                .child(
                    Text::new(message.to_owned())
                        .style(label_style(
                            skin.toast_text.0,
                            skin.toast_text.1,
                            skin.toast_ink,
                            0.0,
                        ))
                        .align(TextAlign::Center),
                ),
            )
            .into()])
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skin() -> Skin {
        Skin {
            ground: Color::rgb(11, 13, 11),
            panel: Color::rgb(17, 22, 20),
            edge: Color::rgb(35, 43, 39),
            accent: Color::rgb(125, 255, 168),
            accent_tint: Color::rgb(18, 42, 30),
            ink: Color::rgb(232, 238, 232),
            muted: Color::rgb(140, 151, 140),
            danger: (
                Color::rgb(58, 20, 24),
                Color::rgb(255, 138, 148),
                Color::rgb(106, 36, 44),
            ),
            radius: 4.0,
            chip_radius: 10.0,
            badge_radius: 6.0,
            button_pad: (14.0, 12.0),
            button_text: 14.0,
            card_pad: (14.0, 12.0),
            chip_pad: (12.0, 8.0),
            chip_height: 44.0,
            chip_text: (12.0, 0.2),
            badge_pad: (6.0, 3.0),
            badge_text: (10.0, 0.8),
            press_dim: 0.3,
            toast_fill: Color::rgb(35, 43, 39),
            toast_ink: Color::rgb(232, 238, 232),
            toast_pad: (16.0, 10.0),
            toast_text: (13.0, FontWeight::Bold),
            toast_max_width: 310.0,
            toast_shadow: None,
            icon_button: (28.0, 44.0),
            callout_pad: (12.0, 10.0),
            callout_text: (13.0, 12.0),
        }
    }

    #[test]
    fn a_press_dims_by_the_skin_and_rest_is_untouched() {
        let s = skin();
        assert!((dimmed(&s, 0.0) - 1.0).abs() < f32::EPSILON);
        assert!((dimmed(&s, 1.0) - 0.7).abs() < 1e-6);
    }

    #[test]
    fn every_component_builds_from_a_skin_alone() {
        let s = skin();
        let _ = button(&s, "go", Kind::Primary, || {});
        let _ = button(&s, "stop", Kind::Danger, || {});
        let _ = card(&s, Text::new("x".to_owned()).into());
        let _ = chip(&s, "tag", s.accent, Rc::new(|| {}));
        let _ = badge(&s, "B", s.accent);
        let _ = section_label(&s, "twins");
        let _ = toast(&s, "saved");
        let _ = icon_button(
            &s,
            "Close",
            Text::new("x".to_owned()).into(),
            s.panel,
            || {},
        );
        let _ = callout(
            &s,
            s.danger.1,
            "Call 911",
            "Chest pain can be an emergency.",
        );
    }
}
