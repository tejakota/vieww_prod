//! Where a frame's milliseconds go (ST-09): the ablation that
//! `examples/frame_attribution.rs` runs from a terminal, as a library the
//! studio calls on the previewed screen.
//!
//! The method is the example's: rasterise the real scene, then rasterise
//! copies of it with one kind of command removed, and report the
//! difference. Nothing is a benchmark of a primitive in isolation; it is the
//! screen's own scene minus one thing at a time, which is the only
//! attribution that accounts for how the pieces interact. The rasteriser is
//! the CPU one every platform shares, so a number measured here means the
//! same thing on every machine that measures it.
//!
//! Per subtree, the cost is the subtree painted alone
//! ([`RenderTree::paint_subtree`]) and rasterised at its own size — what
//! that part of the screen costs to draw, set against the floor of
//! rasterising nothing at that size.

use std::time::Instant;

use vieww_foundation::{Color, Size};
use vieww_paint::native::NativeRenderer;
use vieww_paint::{Command, Scene};
use vieww_render::{NodeDescription, RenderId, RenderTree};

/// What a whole scene costs, and what each kind of command contributes.
#[derive(Debug, Clone, PartialEq)]
pub struct Breakdown {
    /// The surface it was measured at, in pixels.
    pub width: u32,
    pub height: u32,
    /// The commands in the scene.
    pub commands: usize,
    pub glyph_commands: usize,
    pub shadow_commands: usize,
    /// Rasterising the whole scene, best of the runs.
    pub total_ms: f64,
    /// Rasterising nothing at this size: the buffer and the readback.
    pub floor_ms: f64,
    /// What removing the text saved.
    pub text_ms: f64,
    /// What removing the shadows saved.
    pub shadows_ms: f64,
}

impl Breakdown {
    /// Whatever is not the floor, the text or the shadows: fills, paths,
    /// images, layers.
    #[must_use]
    pub fn rest_ms(&self) -> f64 {
        (self.total_ms - self.floor_ms - self.text_ms - self.shadows_ms).max(0.0)
    }

    /// One line for the status bar.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{:.1} ms at {}×{} — text {:.1}, shadows {:.1}, the rest {:.1}, floor {:.1} ({} commands)",
            self.total_ms,
            self.width,
            self.height,
            self.text_ms,
            self.shadows_ms,
            self.rest_ms(),
            self.floor_ms,
            self.commands
        )
    }
}

/// What one subtree costs to draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SubtreeCost {
    pub id: RenderId,
    /// Its own drawing, the floor at its size taken off.
    pub ms: f64,
    pub commands: usize,
}

/// A copy of `scene` keeping only the commands `keep` accepts. Layer
/// markers always travel, in pairs: dropping one half of a pair makes an
/// unbalanced scene the rasteriser rejects.
#[must_use]
pub fn filtered(scene: &Scene, keep: impl Fn(&Command) -> bool) -> Scene {
    let mut out = Scene::new();
    for command in scene.commands() {
        let is_layer = matches!(command, Command::PushLayer { .. } | Command::PopLayer);
        if is_layer || keep(command) {
            out.push_command(command.clone());
        }
    }
    out
}

/// The best of `runs` rasterisations of `scene`, in milliseconds, after one
/// to warm the renderer's caches.
#[must_use]
pub fn raster_ms(
    renderer: &mut NativeRenderer,
    scene: &Scene,
    width: u32,
    height: u32,
    runs: usize,
) -> f64 {
    let (width, height) = (width.max(1), height.max(1));
    let _ = renderer.render_to_pixels(scene, width, height, Color::TRANSPARENT);
    let mut best = f64::MAX;
    for _ in 0..runs.max(1) {
        let start = Instant::now();
        let _ = renderer.render_to_pixels(scene, width, height, Color::TRANSPARENT);
        best = best.min(start.elapsed().as_secs_f64() * 1000.0);
    }
    best
}

/// The ablation of a whole scene.
#[must_use]
pub fn breakdown(scene: &Scene, size: Size, runs: usize) -> Breakdown {
    let (width, height) = (size.width.ceil() as u32, size.height.ceil() as u32);
    let mut renderer = NativeRenderer::new();
    let total_ms = raster_ms(&mut renderer, scene, width, height, runs);
    // Never above the total: a scene that draws almost nothing can time
    // under the empty one by noise alone.
    let floor_ms = raster_ms(&mut renderer, &Scene::new(), width, height, runs).min(total_ms);
    let is_glyphs = |c: &Command| matches!(c, Command::DrawGlyphs { .. });
    let is_shadow = |c: &Command| matches!(c, Command::DrawShadow { .. });
    let no_text = filtered(scene, |c| !is_glyphs(c));
    let no_shadows = filtered(scene, |c| !is_shadow(c));
    let without_text = raster_ms(&mut renderer, &no_text, width, height, runs);
    let without_shadows = raster_ms(&mut renderer, &no_shadows, width, height, runs);
    Breakdown {
        width,
        height,
        commands: scene.len(),
        glyph_commands: scene.commands().iter().filter(|c| is_glyphs(c)).count(),
        shadow_commands: scene.commands().iter().filter(|c| is_shadow(c)).count(),
        total_ms,
        floor_ms,
        text_ms: (total_ms - without_text).max(0.0),
        shadows_ms: (total_ms - without_shadows).max(0.0),
    }
}

/// What each row's subtree costs, for the rows that draw anything, at most
/// `limit` of them in row order.
#[must_use]
pub fn subtree_costs(
    tree: &RenderTree,
    rows: &[(usize, NodeDescription)],
    limit: usize,
    runs: usize,
) -> Vec<SubtreeCost> {
    let mut renderer = NativeRenderer::new();
    let mut out = Vec::new();
    for (_, row) in rows {
        if out.len() >= limit {
            break;
        }
        let mut scene = Scene::new();
        tree.paint_subtree(row.id, &mut scene);
        if scene.is_empty() {
            continue;
        }
        let (width, height) = (row.size.width.ceil() as u32, row.size.height.ceil() as u32);
        if width == 0 || height == 0 {
            continue;
        }
        let drawn = raster_ms(&mut renderer, &scene, width, height, runs);
        let floor = raster_ms(&mut renderer, &Scene::new(), width, height, runs);
        out.push(SubtreeCost {
            id: row.id,
            ms: (drawn - floor).max(0.0),
            commands: scene.len(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use vieww_render::FrameDriver;
    use vieww_widget::prelude::*;

    /// A screen whose cost is mostly text: sixty lines of it, and one card
    /// with a shadow.
    fn text_heavy() -> FrameDriver {
        let mut driver = FrameDriver::new(Size::new(400.0, 900.0));
        let mut lines: Vec<WidgetNode> = Vec::new();
        for i in 0..60 {
            lines.push(
                Text::new(format!(
                    "Line {i}: the quick brown fox jumps over the lazy dog"
                ))
                .size(12.0)
                .into(),
            );
        }
        driver.set_root(Flex::column().children(lines));
        driver.draw_frame();
        driver
    }

    #[test]
    fn removing_the_text_from_a_text_heavy_scene_saves_the_text_cost() {
        let driver = text_heavy();
        let scene = driver.scene().clone();
        let b = breakdown(&scene, Size::new(400.0, 900.0), 5);
        assert_eq!(b.glyph_commands, 60, "one glyph run per line: {b:?}");
        assert!(b.text_ms > 0.0, "text costs something: {b:?}");
        // The text is most of what this scene draws: the ablated copy is
        // cheaper by at least half of what the scene costs above the floor.
        assert!(
            b.text_ms >= (b.total_ms - b.floor_ms) * 0.5,
            "text dominates a page of text: {b:?}"
        );
        assert!(b.summary().contains("60 commands") || b.commands >= 60);
    }

    #[test]
    fn a_subtree_draws_its_own_commands_and_no_others() {
        let driver = text_heavy();
        let tree = driver.renders();
        let rows = tree.describe_subtree(tree.root().expect("root"), 1000);
        let costs = subtree_costs(tree, &rows, 1000, 2);
        let root = costs.first().expect("the column draws");
        assert_eq!(
            root.commands,
            driver.scene().len(),
            "the root's subtree is the frame"
        );
        let lines: Vec<_> = costs.iter().skip(1).collect();
        assert_eq!(lines.len(), 60, "every line is a subtree that draws");
        assert!(lines.iter().all(|c| c.commands == 1), "one glyph run each");
        let total: usize = lines.iter().map(|c| c.commands).sum();
        assert_eq!(total, root.commands, "the children add up to the parent");
    }

    #[test]
    fn a_stale_id_paints_nothing() {
        let mut driver = text_heavy();
        let id = {
            let tree = driver.renders();
            tree.describe_subtree(tree.root().unwrap(), 2)[1].1.id
        };
        driver.set_root(Text::new("one line"));
        driver.draw_frame();
        assert!(!driver.renders().is_alive(id), "the old line is gone");
        let mut scene = Scene::new();
        driver.renders().paint_subtree(id, &mut scene);
        assert!(scene.is_empty());
    }
}
