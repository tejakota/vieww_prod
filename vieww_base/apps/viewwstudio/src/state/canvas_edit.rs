//! ST-01: direct manipulation — the preview as a workspace, not a window.
//!
//! # What this module is
//!
//! A picked render node, mapped back to the `.say` line that produced it,
//! and the four pure-property manipulations the Say vocabulary admits —
//! `padded N`, `spaced N`, `N wide`, `N tall` — applied by rewriting that
//! one line of the buffer. The 600 ms auto-render debounce (N10) turns the
//! rewrite into pixels, which is the whole trick: canvas editing *is* text
//! editing, with a different pointing device.
//!
//! # The bridge, and why it is a match rather than a link
//!
//! Nothing in the render tree knows which `.say` line built it — the
//! generated Rust is ordinary framework widgets, and their render objects
//! have no source positions. So the map is a *structural match*:
//!
//! 1. The `.say` file is read with [`vieww_say_codegen::line`], which
//!    yields the widget lines in document order with their nesting depth.
//! 2. The captured render rows ([`Studio::tree_snapshot`], itself a
//!    pre-order walk) are matched to those lines greedily: a say line of
//!    kind `a column` claims the first unmatched `RenderColumn` row that
//!    descends from the row its *parent* say line claimed.
//! 3. Wrappers the generator adds around a widget — `padded` wraps a
//!    `Container`, `N wide` a `SizedBox`, and several root lines get a
//!    framing column — never match a line of their own: they are passed
//!    through on the way down, and afterwards inherit the nearest matched
//!    *ancestor's* line, so picking a padding box selects the line whose
//!    property drew it.
//! 4. The one shape greedy order gets wrong — the framing column has the
//!    same render name as the root `a column` lines it wraps — is repaired
//!    by a run-level rematch: when a run of same-kind siblings starts on a
//!    row that is an ancestor of the next row in the run, the run is
//!    re-spread over that row's same-named descendants.
//!
//! This is honest where it is weak: two same-named siblings under one
//! parent match in document order, which is the order the generator emits
//! them in. A screen whose structure the matcher cannot reproduce simply
//! refuses to edit — a missing handle, never a wrong one.
//!
//! # Read-only where it must be
//!
//! Canvas editing applies only when the active buffer is Say and the
//! preview is the in-process one: the out-of-process preview is pixels in
//! a child process, and there is no render tree to pick from. The handles
//! do not appear rather than appearing over a frame they cannot describe.

use std::rc::Rc;

use vieww_foundation::Rect;
use vieww_say_codegen::line::{self, LineProp, WidgetLine};

use super::*;

/// One of the four manipulations a canvas handle performs.
///
/// Not every widget accepts every handle: `spaced` is a row's and a
/// column's, and the Say parser enforces that — the canvas must not offer
/// a handle the next Render would reject as a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    /// `padded N` — padding, on any widget.
    Padded,
    /// `spaced N` — the gap between children, on rows and columns.
    Spaced,
    /// `N wide` — a fixed width.
    Wide,
    /// `N tall` — a fixed height.
    Tall,
}

impl Handle {
    /// What the nudge and the inspector call it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Padded => "padding",
            Self::Spaced => "spacing",
            Self::Wide => "width",
            Self::Tall => "height",
        }
    }

    /// The property this handle writes, with its value on `widget` — or
    /// zero when the line carries none yet, because a handle can *grow* a
    /// property that was never written.
    #[must_use]
    pub fn starting_at(self, widget: &WidgetLine) -> LineProp {
        let current = widget.props.iter().find(|p| self.covers(p)).cloned();
        match (self, current) {
            (Self::Padded, Some(LineProp::Padded(n))) => LineProp::Padded(n),
            (Self::Padded, _) => LineProp::Padded(0.0),
            (Self::Spaced, Some(LineProp::Spaced(n))) => LineProp::Spaced(n),
            (Self::Spaced, _) => LineProp::Spaced(0.0),
            (Self::Wide, Some(LineProp::Wide(n))) => LineProp::Wide(n),
            (Self::Wide, _) => LineProp::Wide(0.0),
            (Self::Tall, Some(LineProp::Tall(n))) => LineProp::Tall(n),
            (Self::Tall, _) => LineProp::Tall(0.0),
        }
    }

    /// Whether `p` is the property this handle writes.
    fn covers(self, p: &LineProp) -> bool {
        matches!(
            (self, p),
            (Self::Padded, LineProp::Padded(_))
                | (Self::Spaced, LineProp::Spaced(_))
                | (Self::Wide, LineProp::Wide(_))
                | (Self::Tall, LineProp::Tall(_))
        )
    }

    /// The value after `delta` is applied, never negative: Say's lexer
    /// turns `-7` into a word, the parser refuses it, and a handle that
    /// wrote what the next Render would reject is a handle lying about
    /// what it did.
    #[must_use]
    fn nudged(self, from: LineProp, delta: f32) -> LineProp {
        let next = |n: f32| (n + delta).max(0.0);
        match from {
            LineProp::Padded(n) => LineProp::Padded(next(n)),
            LineProp::Spaced(n) => LineProp::Spaced(next(n)),
            LineProp::Wide(n) => LineProp::Wide(next(n)),
            LineProp::Tall(n) => LineProp::Tall(next(n)),
            other => other,
        }
    }

    /// A drag's `(dx, dy)` turned into this handle's delta.
    ///
    /// Width follows the x axis, height the y. Padding is pulled *inward*
    /// from the top-left corner, so both axes agree. Spacing follows the
    /// parent's main axis: a column's children stack vertically.
    #[must_use]
    fn delta_of(self, dx: f32, dy: f32, column: bool) -> f32 {
        match self {
            Self::Padded => (dx + dy) / 2.0,
            Self::Spaced => {
                if column {
                    dy
                } else {
                    dx
                }
            }
            Self::Wide => dx,
            Self::Tall => dy,
        }
    }

    /// Whether `widget`'s kind accepts this handle's property — the same
    /// rule the Say parser enforces, so a handle never offers an edit the
    /// next Render would turn into a diagnostic.
    #[must_use]
    pub fn accepted_by(self, widget: &WidgetLine) -> bool {
        use vieww_say_codegen::line::WidgetKindName;
        match self {
            Self::Spaced => matches!(widget.kind, WidgetKindName::Row | WidgetKindName::Column),
            _ => true,
        }
    }
}

/// A drag in progress: the handle, and the property as it stood when the
/// drag began — every update rewrites from there, so a drag that wavers
/// does not accumulate its own waver into the file.
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasDrag {
    handle: Handle,
    origin: LineProp,
    column: bool,
}

impl Studio {
    /// The active buffer's widget lines, when it is a `.say` file.
    ///
    /// `None` for a Rust buffer, no buffer, or one with no widget lines —
    /// the reader is forgiving, but a file with no screens has nothing to
    /// edit.
    #[must_use]
    pub fn say_widget_lines(&self) -> Option<Vec<WidgetLine>> {
        let buffer = self.active()?;
        if buffer.language != crate::language::Language::Say {
            return None;
        }
        let lines = line::parse_widget_lines(&buffer.value.text);
        (!lines.is_empty()).then_some(lines)
    }

    /// The say-line index behind each captured render row, recomputed when
    /// the tree or the buffer has moved on since the last ask.
    ///
    /// A row maps to the say line whose render object it is, or — for the
    /// wrapper rows the generator adds — to the nearest *matched* ancestor
    /// row's line. Chrome above the first say line maps to nothing.
    fn say_line_of_rows(&self) -> Rc<Vec<Option<usize>>> {
        let generation = self.tree_generation.peek();
        let buffers = self.buffers.get();
        let key = (
            generation,
            Rc::as_ptr(&buffers) as usize,
            self.tree_snapshot.borrow().len(),
        );
        {
            let cache = self.say_map.borrow();
            if let Some((cached_key, map)) = cache.as_ref() {
                if *cached_key == key {
                    return Rc::clone(map);
                }
            }
        }

        let say = self.say_widget_lines().unwrap_or_default();
        let map = Rc::new({
            let rows = self.tree_snapshot.borrow();
            match_rows_to_say_lines(&rows[..], &say)
        });

        *self.say_map.borrow_mut() = Some((key, Rc::clone(&map)));
        map
    }

    /// The picked render row's widget line: the `.say` source span behind
    /// what the user selected in the Inspector or picked on the canvas.
    ///
    /// `None` when nothing is picked, the pick did not map into the file,
    /// or the active buffer is not Say — the callers' cue to stay
    /// read-only rather than guess.
    #[must_use]
    pub fn picked_say_line(&self) -> Option<WidgetLine> {
        let row = self.inspect_selected.get()?;
        let say = self.say_widget_lines()?;
        let map = self.say_line_of_rows();
        let index = (*map.get(row)?)?;
        say.get(index).cloned()
    }

    /// Whether canvas editing could act right now: the mode is on and a
    /// say line is picked.
    #[must_use]
    pub fn canvas_editable(&self) -> bool {
        self.canvas_edit.get() && self.picked_say_line().is_some()
    }

    /// Rewrite the picked say line with `prop`, through the one edit path
    /// every keystroke takes.
    ///
    /// [`Studio::edit`] is the point: dirty flag, undo history, the
    /// auto-render debounce and the language server all engage exactly as
    /// if the value had been typed. Nothing about canvas editing is a
    /// second editing pipeline — that is the whole design.
    pub fn canvas_write(&self, prop: LineProp) {
        if !self.say_rewrite(|old| line::rewrite_prop(old, &prop)) {
            self.notify(
                "That line cannot take this edit — it is not a widget line the tools recognise.",
            );
        }
    }

    /// Rewrite the picked say line's text with `rewrite`, through the one
    /// edit path every keystroke takes — the machinery ST-01's handles and
    /// ST-02's property grid share, which is why the grid is not a second
    /// editor either.
    ///
    /// Returns whether the line took the rewrite as asked (a rewrite to
    /// what the line already says counts). `false` is a refusal, and the
    /// caller names which one happened in the message it notifies with:
    /// the same `None` from `rewrite` means different things to a drag
    /// (the line is not a widget line) and to a grid field (the clause is
    /// not on the line), and the message should say which one happened —
    /// which is also why the caller, not this function, holds the prose.
    pub(super) fn say_rewrite(&self, rewrite: impl FnOnce(&str) -> Option<String>) -> bool {
        let Some(widget) = self.picked_say_line() else {
            return false;
        };
        let Some(buffer) = self.active() else {
            return false;
        };
        if buffer.language != crate::language::Language::Say {
            return false;
        }
        let Some((index, old, carriage)) = say_line_text(&buffer.value.text, widget.line) else {
            self.notify(
                "That line is no longer where the render tree says it is — the buffer has moved on.",
            );
            return false;
        };
        let Some(new) = rewrite(old) else {
            return false;
        };
        if new == old {
            return true;
        }
        let text = replace_say_line(&buffer.value.text, index, &new, carriage);
        // The selection survives a one-line rewrite by staying where it
        // was, clamped to the new length: offsets past the end of a
        // shorter line would otherwise describe positions that no longer
        // exist.
        let len = text.len();
        let clamp = |o: usize| o.min(len);
        let mut value = vieww_foundation::TextEditingValue::new(text);
        value.selection = vieww_foundation::TextSelection {
            base: clamp(buffer.value.selection.base),
            extent: clamp(buffer.value.selection.extent),
            affinity: buffer.value.selection.affinity,
        };
        self.edit(value);
        true
    }

    /// Nudge the picked line's `handle` property by `delta`, from the value
    /// as written (or zero, when the property is not on the line yet — a
    /// nudge can grow one).
    ///
    /// Nudges are rounded to whole numbers: a keyboard nudge is a
    /// deliberate press, not a gesture, and `padded 12.000001` is not a
    /// line anybody wants to read back.
    pub fn canvas_nudge(&self, handle: Handle, delta: f32) {
        let Some(widget) = self.picked_say_line() else {
            return;
        };
        if !handle.accepted_by(&widget) {
            return;
        }
        let from = handle.starting_at(&widget);
        self.canvas_write(handle.nudged(from, delta.round()));
    }

    /// A drag began on `handle`.
    ///
    /// The property as it stands is captured here, because every update
    /// will rewrite from it.
    pub fn canvas_drag_start(&self, handle: Handle) {
        let Some(widget) = self.picked_say_line() else {
            return;
        };
        if !handle.accepted_by(&widget) {
            return;
        }
        *self.canvas_drag.borrow_mut() = Some(CanvasDrag {
            handle,
            origin: handle.starting_at(&widget),
            column: widget.kind == vieww_say_codegen::line::WidgetKindName::Column,
        });
    }

    /// A drag moved by `(dx, dy)` guest-logical units.
    pub fn canvas_drag_update(&self, dx: f32, dy: f32) {
        let Some(drag) = self.canvas_drag.borrow().clone() else {
            return;
        };
        let delta = drag.handle.delta_of(dx, dy, drag.column);
        self.canvas_write(drag.handle.nudged(drag.origin, delta));
    }

    /// A drag ended. The last write stands; only the bookkeeping goes.
    pub fn canvas_drag_end(&self) {
        *self.canvas_drag.borrow_mut() = None;
    }

    /// The picked render row's rectangle in guest coordinates — where the
    /// handles are drawn.
    ///
    /// `None` when nothing is picked or the row is past the captured cap;
    /// the handles overlay draws nothing rather than something misplaced.
    #[must_use]
    pub fn picked_row_rect(&self) -> Option<Rect> {
        let row = self.inspect_selected.get()?;
        let rows = self.tree_snapshot.borrow();
        let (_, node) = rows.get(row)?;
        let mut origin = vieww_foundation::Offset::ZERO;
        let mut at = row;
        while let Some(parent) = crate::outline::parent(&rows[..], at) {
            origin.dx += rows[parent].1.offset.dx;
            origin.dy += rows[parent].1.offset.dy;
            at = parent;
        }
        Some(Rect::from_origin_size(origin + node.offset, node.size))
    }

    /// The handles to draw for the picked say line, in guest coordinates.
    ///
    /// One square per manipulation the picked line accepts, placed on the
    /// edge that manipulates it: width on the right edge, height on the
    /// bottom, padding pulled inward from the top-left corner, spacing in
    /// the middle — dragging the middle of a column apart reads as exactly
    /// what it does.
    #[must_use]
    pub fn canvas_handles(&self) -> Option<Vec<(Handle, Rect)>> {
        let widget = self.picked_say_line()?;
        let rect = self.picked_row_rect()?;
        const SIZE: f32 = 10.0;
        const HALF: f32 = SIZE / 2.0;
        let square = |cx: f32, cy: f32| Rect::new(cx - HALF, cy - HALF, cx + HALF, cy + HALF);
        let mut handles = Vec::with_capacity(4);
        if Handle::Padded.accepted_by(&widget) {
            handles.push((Handle::Padded, square(rect.left, rect.top)));
        }
        if Handle::Spaced.accepted_by(&widget) {
            handles.push((
                Handle::Spaced,
                square(
                    rect.left + rect.width() / 2.0,
                    rect.top + rect.height() / 2.0,
                ),
            ));
        }
        if Handle::Wide.accepted_by(&widget) {
            handles.push((
                Handle::Wide,
                square(rect.right, rect.top + rect.height() / 2.0),
            ));
        }
        if Handle::Tall.accepted_by(&widget) {
            handles.push((
                Handle::Tall,
                square(rect.left + rect.width() / 2.0, rect.bottom),
            ));
        }
        Some(handles)
    }
}

/// The `line`-th (1-based) line of `text`, with whether it ended in a
/// carriage return — a CRLF buffer must keep its endings through a
/// rewrite, and the writer must not see the `\r` as part of the words.
fn say_line_text(text: &str, line: u32) -> Option<(usize, &str, bool)> {
    let index = usize::try_from(line).ok()?.checked_sub(1)?;
    let raw = text.split('\n').nth(index)?;
    let carriage = raw.ends_with('\r');
    Some((index, raw.strip_suffix('\r').unwrap_or(raw), carriage))
}

/// `text` with line `index` (0-based) replaced by `new`, carriage return
/// and all other lines byte-identical.
fn replace_say_line(text: &str, index: usize, new: &str, carriage: bool) -> String {
    let new = if carriage {
        format!("{new}\r")
    } else {
        new.to_owned()
    };
    text.split('\n')
        .enumerate()
        .map(|(i, old)| if i == index { new.as_str() } else { old })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The say-line index behind each render row (ST-01's bridge).
///
/// See the module doc for the matching rules. Rows that are wrappers or
/// chrome map to the nearest matched ancestor's say line; rows above the
/// first match map to nothing. A say line that found no row simply has no
/// row pointing at it — a line the bridge cannot see is a line the canvas
/// cannot edit, not an error.
fn match_rows_to_say_lines<R: crate::outline::Row>(
    rows: &[R],
    say: &[WidgetLine],
) -> Vec<Option<usize>> {
    // Row → say-line for the rows that matched a line's own render object.
    let mut matched: Vec<Option<usize>> = vec![None; rows.len()];
    // Say index → the row it matched. A parent constrains its children's
    // candidates to descendants of its row.
    let mut row_of_say: Vec<Option<usize>> = vec![None; say.len()];

    for (si, widget) in say.iter().enumerate() {
        let Some(name) = widget.kind.render_name() else {
            continue; // composite: never matched by name
        };
        // The parent's row: the nearest earlier say line one level up.
        let parent_row = widget
            .depth
            .checked_sub(1)
            .and_then(|d| {
                say.iter()
                    .rposition(|w| w.depth == d && w.line < widget.line)
            })
            .and_then(|p| row_of_say[p]);
        let descends = |row: usize| -> bool {
            match parent_row {
                None => true,
                Some(parent) => {
                    let mut at = row;
                    while let Some(up) = crate::outline::parent(rows, at) {
                        if up == parent {
                            return true;
                        }
                        at = up;
                    }
                    false
                }
            }
        };
        if let Some(row) =
            (0..rows.len()).find(|&r| descends(r) && matched[r].is_none() && rows[r].name() == name)
        {
            matched[row] = Some(si);
            row_of_say[si] = Some(row);
        }
    }

    rematch_wrapped_runs(rows, say, &mut matched, &mut row_of_say);

    // Fill in the wrappers: each unmatched row inherits the nearest
    // matched ancestor's line, so picking a padding box selects the line
    // whose property drew it — the nearest *enclosing* say line, which is
    // the honest answer for a box the line's own property drew.
    let mut inherited = vec![None; rows.len()];
    for row in 0..rows.len() {
        if matched[row].is_some() {
            inherited[row] = matched[row];
            continue;
        }
        let mut at = row;
        while let Some(up) = crate::outline::parent(rows, at) {
            if matched[up].is_some() {
                inherited[row] = matched[up];
                break;
            }
            at = up;
        }
    }
    inherited
}

/// Repair the one shape greedy order gets wrong: a run of same-kind,
/// same-depth say lines whose *first* row is an ancestor of the second's.
///
/// The generator wraps several root lines in a framing widget, and for
/// `a column` roots that wrapper is itself a `RenderColumn` — so greedy
/// order hands the wrapper to the first line and starves the run's tail.
/// When that is detected, the run is re-spread over the first row's
/// same-named strict descendants, in document order — which is exactly
/// where the generator put the real widgets. Any other shape (a column
/// genuinely containing a column, say) keeps its greedy match, because a
/// real parent's row is an ancestor of its children's rows *across
/// different depths*, and a run is same-depth by construction.
fn rematch_wrapped_runs(
    rows: &[impl crate::outline::Row],
    say: &[WidgetLine],
    matched: &mut [Option<usize>],
    row_of_say: &mut [Option<usize>],
) {
    let mut start = 0;
    while start < say.len() {
        // A run: consecutive same-depth, same-kind lines.
        let mut end = start + 1;
        while end < say.len()
            && say[end].depth == say[start].depth
            && say[end].kind == say[start].kind
        {
            end += 1;
        }
        let run: Vec<usize> = (start..end)
            .filter(|&si| row_of_say[si].is_some())
            .collect();
        if run.len() >= 2 {
            let first = row_of_say[run[0]].expect("filtered to Some");
            let second = row_of_say[run[1]].expect("filtered to Some");
            let is_ancestor = {
                let mut at = second;
                let mut found = false;
                while let Some(up) = crate::outline::parent(rows, at) {
                    if up == first {
                        found = true;
                        break;
                    }
                    at = up;
                }
                found
            };
            if is_ancestor {
                // The descendants of the wrapper row, same name, in order.
                let descendants: Vec<usize> = (0..rows.len())
                    .filter(|&r| rows[r].name() == rows[first].name())
                    .filter(|&r| {
                        let mut at = r;
                        let mut inside = false;
                        while let Some(up) = crate::outline::parent(rows, at) {
                            if up == first {
                                inside = true;
                                break;
                            }
                            at = up;
                        }
                        inside
                    })
                    .collect();
                if descendants.len() >= run.len() {
                    // Two passes, not one interleaved: a row the run is
                    // moving *to* can be a row the run is moving *from*,
                    // and clearing inside the assignment loop would wipe
                    // the claim the line before it had just laid down.
                    for &si in &run {
                        let old_row = row_of_say[si].expect("filtered to Some");
                        matched[old_row] = None;
                    }
                    for (slot, &si) in run.iter().enumerate() {
                        let new_row = descendants[slot];
                        row_of_say[si] = Some(new_row);
                        matched[new_row] = Some(si);
                    }
                }
            }
        }
        start = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row shape for the matcher's unit tests: the matcher reads names
    /// and depths, and `RenderId` cannot be constructed outside the render
    /// crate — which is fine, because the bridge is about *shapes*.
    #[derive(Debug, Clone, Copy)]
    struct TestRow {
        depth: usize,
        name: &'static str,
    }

    impl crate::outline::Row for TestRow {
        fn depth(&self) -> usize {
            self.depth
        }
        fn children(&self) -> usize {
            0
        }
        fn name(&self) -> &str {
            self.name
        }
    }

    fn rows(names: &[(&'static str, usize)]) -> Vec<TestRow> {
        names
            .iter()
            .map(|(name, depth)| TestRow {
                depth: *depth,
                name,
            })
            .collect()
    }

    fn say_line(
        line: u32,
        depth: u32,
        kind: vieww_say_codegen::line::WidgetKindName,
    ) -> WidgetLine {
        WidgetLine {
            line,
            phrase: "a column",
            kind,
            value: None,
            props: Vec::new(),
            depth,
        }
    }

    use vieww_say_codegen::line::WidgetKindName as K;

    /// The counter's shape: one column, three children in order.
    #[test]
    fn the_counter_maps_row_to_line() {
        let say = vec![
            say_line(8, 0, K::Column),
            say_line(9, 1, K::Heading),
            say_line(10, 1, K::Label),
            say_line(11, 1, K::Text),
        ];
        let rows = rows(&[
            ("RenderColumn", 0),
            ("RenderText", 1),
            ("RenderText", 1),
            ("RenderText", 1),
        ]);
        let map = match_rows_to_say_lines(&rows, &say);
        assert_eq!(map, vec![Some(0), Some(1), Some(2), Some(3)]);
    }

    /// A padding wrapper under the column maps to the column — the
    /// nearest enclosing say line, not a wrong one.
    #[test]
    fn a_wrapper_inherits_the_enclosing_line() {
        let say = vec![say_line(8, 0, K::Column), say_line(9, 1, K::Text)];
        let rows = rows(&[
            ("RenderColumn", 0),
            ("RenderDecoratedBox", 1), // the text's padding wrapper
            ("RenderText", 2),
        ]);
        let map = match_rows_to_say_lines(&rows, &say);
        assert_eq!(map, vec![Some(0), Some(0), Some(1)]);
    }

    /// Two root columns: the framing column the generator adds must not
    /// steal the first say column's match.
    #[test]
    fn two_root_columns_do_not_confuse_the_framing_wrapper() {
        let say = vec![say_line(7, 0, K::Column), say_line(11, 0, K::Column)];
        let rows = rows(&[
            ("RenderColumn", 0), // the framing wrapper
            ("RenderColumn", 1), // say line 7
            ("RenderColumn", 1), // say line 11
        ]);
        let map = match_rows_to_say_lines(&rows, &say);
        assert_eq!(
            map,
            vec![None, Some(0), Some(1)],
            "the wrapper maps to nothing; each real column to its own line"
        );
    }

    /// Three root columns, the wrapper included.
    #[test]
    fn three_root_columns_each_get_their_own_line() {
        let say = vec![
            say_line(7, 0, K::Column),
            say_line(11, 0, K::Column),
            say_line(15, 0, K::Column),
        ];
        let rows = rows(&[
            ("RenderColumn", 0),
            ("RenderColumn", 1),
            ("RenderColumn", 1),
            ("RenderColumn", 1),
        ]);
        let map = match_rows_to_say_lines(&rows, &say);
        assert_eq!(map, vec![None, Some(0), Some(1), Some(2)]);
    }

    /// A column genuinely containing a column is *not* read as a wrapper:
    /// different depths are different lines, and the nesting is real.
    #[test]
    fn a_column_inside_a_column_keeps_its_nesting() {
        let say = vec![say_line(7, 0, K::Column), say_line(8, 1, K::Column)];
        let rows = rows(&[("RenderColumn", 0), ("RenderColumn", 1)]);
        let map = match_rows_to_say_lines(&rows, &say);
        assert_eq!(map, vec![Some(0), Some(1)]);
    }

    /// A line the bridge cannot see — no row of its render name — simply
    /// has no row, and the rows that do exist keep their own lines.
    #[test]
    fn an_unmatchable_line_is_absent_rather_than_wrong() {
        let say = vec![
            say_line(8, 0, K::Column),
            say_line(9, 1, K::Button), // composite: no render name
        ];
        let rows = rows(&[
            ("RenderColumn", 0),
            ("RenderDecoratedBox", 1), // the button's own box
            ("RenderText", 2),         // the button's label
        ]);
        let map = match_rows_to_say_lines(&rows, &say);
        assert_eq!(
            map,
            vec![Some(0), Some(0), Some(0)],
            "the button's rows inherit the enclosing column: read-only, not wrong"
        );
    }
}
