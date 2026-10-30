//! The widget inspector: the render tree, as it was laid out.
//!
//! # What this replaces
//!
//! Five lines of buffer statistics — name, line count, caret, platform, preview
//! state — under a heading that said "Inspector". Not a stub anybody chose:
//! there was no way to write anything else. An application holds a *widget*
//! tree and never a render one, and nothing in the framework would answer a
//! question about the objects on screen.
//!
//! Three things had to exist for this to be possible, and all three now do:
//! [`RenderObject::describe`](vieww_render::RenderObject::describe), which lets
//! an object say what it is; `RenderTree::describe_subtree`, which walks it;
//! and `FrameDriver::renders`, which is how an application reaches a tree at
//! all.
//!
//! # It shows the previous frame, and that is not a compromise
//!
//! A widget cannot read the render tree during `build` — the tree it would read
//! is the one its own build is about to replace. So `main`'s `before_frame`
//! hook leaves a description in [`Studio::tree_snapshot`], and this draws that.
//! One frame behind is the same contract `TextLayoutProbe` states, and an
//! inspector describing a frame the user has already seen is telling the truth.
//!
//! # A flat list with a depth, drawn as an outline
//!
//! Stored flat, because that is what a list view can virtualise; drawn as an
//! outline (ST-06): a row with children has a disclosure arrow that folds it,
//! a filter narrows the rows to matching names and their ancestors, and once
//! a row is clicked the arrow keys walk the tree (↑/↓ move, ← folds or goes
//! to the parent, → unfolds or goes to the first child). ⌘/Ctrl-click adds a
//! row to the selection, Shift-click a range, and the header gives the
//! selection's combined bounds. The rules are in `crate::outline`.

use vieww_foundation::{Alignment, EdgeInsets};
use vieww_widget::prelude::*;
use vieww_widget::{widget_node_from, Container, Flexible};

use crate::state::Studio;
use crate::theme::StudioTheme;
use crate::ui::chrome::{clickable, gap, hairline, label, label_bold, mono, space};

/// One row's height. Fixed, so the pane's scroll offset is a row index times a
/// number rather than a measurement.
const ROW: f32 = 22.0;
/// How far each level of depth indents.
const STEP: f32 = 12.0;

#[derive(Debug)]
pub struct InspectorPane {
    pub studio: Studio,
}

impl Widget for InspectorPane {
    fn debug_name(&self) -> &'static str {
        "InspectorPane"
    }

    fn kind(&self) -> WidgetKind<'_> {
        WidgetKind::Composed
    }

    fn build(&self, ctx: &BuildContext) -> WidgetNode {
        let chrome = *StudioTheme::of(ctx);
        let colors = ThemeData::of(ctx).colors;

        // Read so the pane rebuilds when the capture changes. The snapshot
        // itself is a `RefCell` and not a signal — it is a vector that would be
        // cloned into one on every frame, and what the pane needs to know is
        // that it *differs*. Same shape as `jobs_generation`.
        let _generation = self.studio.tree_generation.get();
        let snapshot = self.studio.tree_snapshot.borrow();

        if snapshot.is_empty() {
            return Container::new()
                .padding(EdgeInsets::all(14.0))
                .child(label(
                    "Nothing captured yet. The tree is read once a frame while this tab is open.",
                    12.0,
                    colors.on_surface_variant,
                ))
                .into();
        }

        let selected = self.studio.inspect_selected.get();
        let selection = self.studio.inspect_selection.get();
        let collapsed = self.studio.inspect_collapsed.get();
        let filter = self.studio.inspect_filter.get();
        let shown = crate::outline::visible(&snapshot, &collapsed, &filter);
        let keys = crate::outline::paths(&snapshot);
        let attribution = self.studio.attribution.borrow().clone();
        let mut rows: Vec<WidgetNode> = Vec::with_capacity(shown.len() + 2);
        for index in shown.iter().copied() {
            let (depth, node) = &snapshot[index];
            let studio = self.studio.clone();
            let toggling = self.studio.clone();
            let is_selected = selected == Some(index) || selection.contains(&index);
            let is_primary = selected == Some(index);
            let folded = node.children > 0 && collapsed.contains(&keys[index]);
            // The disclosure arrow: folds the row (a click on it does not
            // select), or nothing for a leaf.
            let leaf = node.children == 0;
            let toggling = std::rc::Rc::new(toggling);
            let arrow = move || -> WidgetNode {
                if leaf {
                    return space(12.0);
                }
                let toggling = std::rc::Rc::clone(&toggling);
                clickable(
                    move || {
                        Container::new()
                            .width(12.0)
                            .child(label(
                                if folded { "\u{25b8}" } else { "\u{25be}" },
                                10.0,
                                colors.outline,
                            ))
                            .into()
                    },
                    move || toggling.inspector_toggle(index),
                )
                .into()
            };
            let name = node.name;
            let children = node.children;
            // ST-09: what this row's subtree costs to draw, once a frame has
            // been attributed.
            let cost = attribution
                .as_ref()
                .and_then(|a| a.row(node.id))
                .map_or_else(String::new, |c| format!("{:.2} ms", c.ms));
            let indent = STEP.mul_add(
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a render tree deeper than 2^24 is not a tree"
                )]
                {
                    *depth as f32
                },
                8.0,
            );

            rows.push(
                clickable(
                    move || {
                        Container::new()
                            .height(ROW)
                            .color(if is_selected {
                                chrome.selection
                            } else {
                                chrome.chrome_1
                            })
                            .padding(EdgeInsets::only(indent, 0.0, 8.0, 0.0))
                            .child(
                                Flex::row()
                                    .cross_axis_alignment(CrossAxisAlignment::Center)
                                    .children(children![
                                        arrow(),
                                        mono(name, 11.0, colors.on_surface),
                                        gap(),
                                        // The child count, because "does this
                                        // have anything in it" is the first
                                        // question asked of every row and the
                                        // indentation only answers it for the
                                        // row above.
                                        label(
                                            &if children == 0 {
                                                String::new()
                                            } else {
                                                format!("{children}")
                                            },
                                            10.0,
                                            colors.outline
                                        ),
                                        gap(),
                                        Flexible::expanded(1).child(SizedBox::shrink()),
                                        mono(&cost, 10.0, colors.primary),
                                    ]),
                            )
                            .into()
                    },
                    move || {
                        // Clicking the only selected row clears it, so the
                        // property list can be put away; ⌘/Ctrl and Shift
                        // grow the selection (`outline::click`).
                        let m = studio.held_modifiers.get();
                        studio.inspector_click(index, m.meta() || m.control(), m.shift());
                    },
                )
                .into(),
            );

            if is_primary {
                rows.push(properties(node, colors, chrome.chrome_2));
            }
        }

        let scroll = self
            .studio
            .sidebar_scroll(crate::state::View::Inspector)
            .clone();
        let list = Scrollable::vertical(scroll.offset())
            .key("inspector")
            .on_drag(scroll.on_drag())
            .on_drag_end(scroll.on_drag_end())
            .on_extents(scroll.on_extents())
            .child(
                Flex::column()
                    .cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .children(rows),
            );

        Flex::column()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children(children![
                Container::new()
                    .padding(EdgeInsets::symmetric(10.0, 7.0))
                    .child(
                        Flex::row()
                            .cross_axis_alignment(CrossAxisAlignment::Center)
                            .children(children![
                                label_bold("Render tree", 11.0, colors.on_surface_variant),
                                gap(),
                                // Said out loud, because a truncated tree that
                                // does not admit it is a tree somebody reads as
                                // complete. The cap is in `Studio::capture_tree`.
                                label(&format!("{} nodes", snapshot.len()), 10.5, colors.outline),
                                space(8.0),
                                // The never-unload cost, made visible. Every
                                // Render maps a dylib this process will never
                                // unmap — a deliberate decision, and one whose
                                // price was estimated in a twenty-minute
                                // session and never measured over a working
                                // day. See `crate::loaded`.
                                label(
                                    &format!("{} images", crate::loaded::loaded_images()),
                                    10.5,
                                    colors.outline
                                ),
                            ])
                    ),
                state_section(&self.studio, colors, chrome.chrome_2),
                attribution_line(attribution.as_ref(), colors),
                filter_row(&self.studio, &filter, chrome),
                selection_line(&self.studio, selection.len(), colors),
                hairline(chrome.line),
                Flexible::expanded(1).child(list),
            ])
            .into()
    }
}

widget_node_from!(InspectorPane);

/// The previewed screen's state (ST-22): each element state's values, as
/// `ElementState::debug_fields` gives them — Say's `keep` values, for one —
/// captured with the tree once a frame, so a tap in the preview shows up
/// here on the next frame. Absent when the screen keeps nothing.
fn state_section(
    studio: &Studio,
    colors: ColorScheme,
    background: vieww_foundation::Color,
) -> WidgetNode {
    let fields = studio.preview_fields.borrow();
    if fields.is_empty() {
        return SizedBox::shrink().into();
    }
    let mut lines: Vec<WidgetNode> =
        vec![label_bold(crate::i18n::t("State"), 11.0, colors.on_surface_variant).into()];
    for (_path, widget, values) in fields.iter() {
        lines.push(mono(widget, 10.5, colors.outline).into());
        lines.extend(values.iter().map(|(name, value)| pair(name, value, colors)));
    }
    Container::new()
        .color(background)
        .padding(EdgeInsets::only(10.0, 6.0, 10.0, 8.0))
        .child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .children(lines),
        )
        .into()
}

/// The name filter (ST-06): narrows the rows to names containing it, with
/// their ancestors, so a match keeps its place in the tree.
fn filter_row(studio: &Studio, filter: &str, chrome: crate::theme::StudioTheme) -> WidgetNode {
    let setting = studio.clone();
    Container::new()
        .padding(EdgeInsets::only(10.0, 0.0, 10.0, 6.0))
        .child(
            Container::new()
                .color(chrome.chrome_1)
                .radius(4.0)
                .padding(EdgeInsets::symmetric(6.0, 3.0))
                .child(
                    TextField::text(filter.to_owned())
                        .size(11.0)
                        .placeholder(crate::i18n::t("Filter by name"))
                        .on_changed(std::rc::Rc::new(
                            move |v: vieww_foundation::TextEditingValue| {
                                setting.set_inspector_filter(v.text);
                            },
                        )),
                ),
        )
        .into()
}

/// With more than one row selected: how many, and their combined bounds.
/// ST-09: the attributed screen's breakdown, above the rows it annotates.
fn attribution_line(
    attribution: Option<&crate::state::FrameAttribution>,
    colors: ColorScheme,
) -> WidgetNode {
    let Some(screen) = attribution.and_then(|a| a.screen.as_ref()) else {
        return SizedBox::shrink().into();
    };
    Container::new()
        .padding(EdgeInsets::only(10.0, 0.0, 10.0, 6.0))
        .child(label(
            &crate::i18n::tf(
                "This frame: {} ms — text {}, shadows {}, the rest {}, floor {}",
                &[
                    &format!("{:.1}", screen.total_ms),
                    &format!("{:.1}", screen.text_ms),
                    &format!("{:.1}", screen.shadows_ms),
                    &format!("{:.1}", screen.rest_ms()),
                    &format!("{:.1}", screen.floor_ms),
                ],
            ),
            10.5,
            colors.primary,
        ))
        .into()
}

fn selection_line(studio: &Studio, count: usize, colors: ColorScheme) -> WidgetNode {
    if count < 2 {
        return SizedBox::shrink().into();
    }
    let Some(b) = studio.inspector_selection_bounds() else {
        return SizedBox::shrink().into();
    };
    Container::new()
        .padding(EdgeInsets::only(10.0, 0.0, 10.0, 6.0))
        .child(label(
            &crate::i18n::tf(
                "{} selected · {} × {} at {}, {}",
                &[
                    &count,
                    &format!("{:.1}", b.width()),
                    &format!("{:.1}", b.height()),
                    &format!("{:.1}", b.left),
                    &format!("{:.1}", b.top),
                ],
            ),
            10.5,
            colors.on_surface_variant,
        ))
        .into()
}

/// The selected node's own facts, under it.
///
/// Size and offset come from the tree and are always there; the rest is
/// whatever the object said about itself through `RenderObject::describe`,
/// which for most objects today is nothing. That is visible rather than hidden:
/// a row with no properties says so, so the gap between "this object has
/// nothing to say" and "nobody has taught it to speak yet" is one somebody can
/// see and close.
fn properties(
    node: &vieww_render::NodeDescription,
    colors: ColorScheme,
    background: vieww_foundation::Color,
) -> WidgetNode {
    let mut lines: Vec<WidgetNode> = vec![
        pair(
            "size",
            &format!("{:.1} × {:.1}", node.size.width, node.size.height),
            colors,
        ),
        pair(
            "offset",
            &format!("{:.1}, {:.1}", node.offset.dx, node.offset.dy),
            colors,
        ),
    ];
    if node.properties.is_empty() {
        lines.push(
            label(
                "No properties published — this object has no describe().",
                10.5,
                colors.outline,
            )
            .into(),
        );
    } else {
        lines.extend(
            node.properties
                .iter()
                .map(|(name, value)| pair(name, value, colors)),
        );
    }

    Container::new()
        .color(background)
        .padding(EdgeInsets::only(20.0, 6.0, 10.0, 8.0))
        .child(
            Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .children(lines),
        )
        .into()
}

/// One `name  value` line, the name dimmed and the value in the code face.
fn pair(name: &str, value: &str, colors: ColorScheme) -> WidgetNode {
    Container::new()
        .height(17.0)
        .alignment(Alignment::CENTER_LEFT)
        .child(
            Flex::row()
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .children(children![
                    Container::new().width(96.0).child(label(
                        name,
                        10.5,
                        colors.on_surface_variant
                    )),
                    space(6.0),
                    Flexible::expanded(1).child(mono(value, 10.5, colors.on_surface)),
                ]),
        )
        .into()
}
