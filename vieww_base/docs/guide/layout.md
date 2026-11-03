# Layout

## The one rule

Layout in vieww is a single conversation with one direction:

> A parent hands each child a `Constraints` — a minimum and maximum size.
> The child returns the `Size` it chose *within* those constraints.
> The parent then decides where to put it.

Three things follow from that, and they answer most sizing questions by
themselves:

- **A child never learns its own position.** It is sized in the dark and
  placed afterwards, by the parent. Position is not an input to layout; it
  is an output of it.
- **A child cannot ask how big its parent is.** "Make me as wide as my
  parent" is not something a child can request, because the child is not
  entitled to the answer. The parent has to *say so*, by handing down tight
  constraints — and in vieww, that usually means `SizedBox::expand()` inside
  a bounded parent, or `CrossAxisAlignment::Stretch` on a `Flex`.
- **Layout runs once per node per frame.** No fixed point, no second pass.
  If the child dislikes its constraints it does not get to negotiate; it
  picks the best `Size` it can and the result is what it is. This is why
  vieww can lay out a deep tree quickly, and it is a real constraint on the
  layout shapes you can express — the escape hatch is
  [`IntrinsicSize`](#intrinsics-asking-the-child-instead), and it is priced
  accordingly.

If you have used Flutter, this is the box protocol, stated the same way.
[`../architecture/ui-tree.md`](../architecture/ui-tree.md) explains why this
lives in the render tree and what the widget and element trees above it do
about it.

## Tight, loose, and the size you actually got

A `Constraints` is a minimum and a maximum on both axes. Two states cover
almost every situation you will hit:

- **Tight** — minimum equals maximum. The child has no choice: `SizedBox`
  hands its child a tight constraint, `Flexible` with the default
  `FlexFit::Tight` does the same along the flex's main axis, and a window's
  root is as tight as the platform made it. If the box is exactly 48×48 and
  you did not ask for that, look for something upstream handing down a tight
  48.
- **Loose** — minimum is zero. The child may be anywhere from invisible to
  the maximum. `Padding` loosens in the sense that matters here: it deflates
  the incoming constraints by its insets and passes what is left down. A
  `Text` in loose constraints takes the width of its longest line and no
  more — which is why unwrapped text in a wide window sits at the left with
  empty space beside it, and the fix is an alignment, not a size.

The debugging order that follows is mechanical, and worth doing in order:

1. What did the parent *offer*? Wrap the child in
   `MeasuredConstraints::new().on_constraints(...)` and log it — or, in a
   test, use the same widget against a `Constraints` you construct by hand.
2. What did the child *choose*? `Measured` reports the `Rect` the child
   ended up with, in the same no-op wrapper style.
3. Which of the two is wrong is now a fact rather than a guess, and the fix
   goes on the one that is wrong — a `SizedBox`, a `Constrained`, a
   `Flexible`, or the child's own layout code.

`LayoutBuilder` is the widget form of step 1: it rebuilds with the
constraints whenever they change, so the child can *decide* on them rather
than merely report them. Use it when the decision is real — a chart that
picks its tick count from its width — not as a substitute for understanding
the constraints you were already given.

## The widgets that decide size

There is no `width` flag on a button and no `margin` on a `Container`. Size
decisions are widgets, one job each, composed from outside:

- **`SizedBox`** — the fixed size. `SizedBox::square(48.0)`, `::width(16.0)`
  as a spacer, `::shrink()` for "as small as allowed", `::expand()` for
  "as large as allowed".
- **`Constrained`** — a `Constraints` verbatim, for the min/max cases
  `SizedBox` cannot say (`min_width`, "no taller than 200").
- **`Flexible`** — a child's share of a `Flex`'s leftover main-axis space.
  `Flexible::expanded(1)` is the tight form ("fill this share exactly");
  `Flexible::new()` is loose ("up to the share, less if the child wants
  less"). It must be the *direct* child of the `Flex`, and it stores its
  factor on itself rather than in a parent-owned slot — which is why an
  `expanded` label beside a fixed-size control is the canonical "label
  wraps instead of pushing the control off the edge" pattern.
- **`AspectRatio`** — the size one axis, derive the other.
- **`Align` / `Center`** — nothing to do with size beyond loosening: they
  hand the child loose constraints and place it within themselves.
- **`Stack` + `Positioned`** — the *only* way a child learns a position:
  `Positioned` says where, and `Stack` sizes non-positioned children to
  itself per `StackFit`.
- **`IntrinsicSize`** — the backwards question. Below.

A `Flex` itself answers two questions you should answer deliberately:

- `MainAxisSize::Max` (the default) expands to the incoming maximum;
  `MainAxisSize::Min` shrink-wraps to what the children needed. A row of
  buttons inside a card is `Min`; the card's content row is `Max`.
- `CrossAxisAlignment` positions children on the other axis. `Center`
  (the default) aligns the children's *boxes*; `Baseline` aligns their
  *letters*, which is the difference between a row that looks designed and
  one where the 13 px label rides high beside the 24 px heading. `Baseline`
  is rows-only — a column asked for it behaves as `Start` — and non-text
  children fall back to `Start` individually, so an icon in a baseline row
  does not hang from the top edge.

## Unbounded constraints, and the classic collapse

A scroll view hands its child **infinite** space along the scroll axis —
that is what scrolling *is*. Two consequences bite regularly:

- `SizedBox::expand()` inside an unbounded axis has nothing to fill, and
  the doc on the method itself says so: it is exactly the mistake it looks
  like. A full-width row inside a vertical `ListView` is
  `Flexible::expanded(1)` (bounded by the *cross* axis) or a `SizedBox`
  with an explicit height — never `expand()`.
- A `Flex` with `MainAxisSize::Max` inside an unbounded main axis has no
  maximum to take, so the content sizes to its children — usually fine,
  occasionally a surprise. `Min` says the same thing deliberately.

A nested scroll (a horizontal row inside a vertical list) passes the
unbounded main axis inward: the inner row's *cross* axis arrives unbounded
too, and a stretched child there falls back to zero rather than filling
forever. If a column of rows has collapsed-height rows, this is almost
always it, and the fix is an explicit cross-axis size or
`CrossAxisAlignment::Start` with sized children.

## Intrinsics: asking the child instead

Everything above is one-directional by design: the parent decides, the child
fits. A handful of layouts genuinely need the other direction — a card as
wide as its longest label, a row of cells all as tall as the tallest, a
divider that runs the height of the taller column. `IntrinsicSize` is the
escape hatch:

```rust
use vieww::prelude::*;

// A row whose two cells are both as tall as the taller one.
let row = IntrinsicSize::height(
    Flex::row().children(children![
        Text::new("one line"),
        Text::new("a much longer run of text that will wrap onto several lines"),
    ]),
);
```

`IntrinsicSize::width` is the same question on the other axis.

It costs what it looks like it costs: the subtree below it is walked a
second time per layout — queried without being laid out, then laid out for
real. That is fine over a card and wrong over a thousand list rows, which is
why intrinsics are opt-in rather than woven through the catalogue.

The machinery underneath is honest about its limits, and so should you be:
an intrinsic answer is an `Option`, and a render object that does not
implement one answers `None` — "I do not know" — which propagates upward and
lets `IntrinsicSize` fall back to ordinary layout, rather than reporting
zero and collapsing somewhere far from the cause. If a widget you wrote
never seems to be measured, that is the first suspect.

## Where layout ends

Layout produces geometry; painting happens afterwards, in a separate pass,
into the `Scene` that the render tree records. Hit testing, focus and
scrolling all consume the same geometry layout produced. If your box is the
right size but the wrong *colour*, you are past this page —
[`theming.md`](./theming.md). If it is the right size in the wrong *place*,
it is `Align`, `Stack`/`Positioned`, or a parent's `Flex` placement — all
still this page. If it is the right size and nothing else is,
[`testing.md`](./testing.md) is how you pin that down without a window: the
whole layout protocol runs headlessly, which is also how several hundred of
this repository's own layout tests run in CI with no display and no GPU.

## What to read next

- [`../architecture/ui-tree.md`](../architecture/ui-tree.md) — the same
  protocol from the inside: three trees and why layout lives in the third.
- [`../architecture/rendering-pipeline.md`](../architecture/rendering-pipeline.md)
  — what happens to the geometry once layout is done with it.
- [`widgets.md`](./widgets.md) — the catalogue and its tiers, and why there
  is no `full_width` flag anywhere.
- `examples/features/` — one directory per widget or feature, all headless.
