# Platform views: embedding what the OS draws

This file exists because PENDING §6.2 asked a question that could not be
answered from a declaration alone: *what is the story for embedded native
views?* Video, a map, a web view, a camera preview — anything the operating
system draws that vieww must position and clip but not rasterize.
`vieww_foundation::capability` has carried the shape — spec, handle,
`create`/`set_bounds`/`send` — since the capability module was written, and
until FW-20 nothing had ever implemented it. A shape with zero consumers is
not a design; it is a guess. This file records what the first consumer
learned, so the second one starts from evidence.

## The declared shape, and what each piece is for

The whole contract lives in `vieww_foundation::capability::platform_view`,
deliberately in the foundation rather than the platform bridge so every
layer above can *ask* about embedding even where only `vieww-platform-winit`
can *do* it:

- **`PlatformViewSpec`** — a `kind` string plus string parameters. A registry
  of names rather than a fixed enum, for the service registry's own reason: a
  framework that enumerated the embeddable view types would be a framework
  you have to fork to embed a new one. Strings across the language boundary
  (Kotlin, Swift) because the framework does not get to choose the
  application's serialization format.
- **`PlatformViewHandle`** — dropping it destroys the view. The same
  subscription contract as the rest of the capability module: unmounting the
  widget that owns the handle is enough, and there is no `dispose` call to
  forget. A leaked native view is a whole embedded browser or map engine
  still running, which is why the ownership is shaped so the compiler
  enforces it.
- **`create` / `set_bounds` / `send`** — position at a rectangle in logical
  pixels from the window's top-left, move it when layout does, and an
  escape-hatch message channel for the rest. `set_bounds` is called every
  frame a rectangle changes, so the trait documents that an *unchanged*
  rectangle must be cheap — it is on the path of any scroll that contains a
  view.
- **What embedding is not** — the trait's own doc says it plainly: a platform
  view is not clipped by an ancestor's rounded corner, not faded by an
  opacity above it, not drawn over without the platform's own compositing,
  and it takes its own pointer events rather than going through the gesture
  arena. These are properties of embedding, not bugs in an implementation.

## What the first consumer found: the anchor

The first real implementation is `X11PlatformViews`
(`vieww-platform-winit`'s `platform_views.rs`, Linux): real X11 child
windows, `dlopen`'d from `libX11.so.6` so a Wayland-only session is refused
rather than unrunnable. Building it found exactly one gap in the declared
shape:

**`create` positions a view "from the top-left of *the window*" — but the
service is built before any window exists.** `services::platform()` runs
before `App::run` creates one, and the registry the service goes into is the
application's own, closed by then. So whoever owns the host window has to
hand it to the service explicitly, after the fact. The X11 implementation
puts that on an inherent method — `X11PlatformViews::anchor(&window)` —
rather than on the trait, because a trait method would force every
implementation (Android's, iOS's, the web's) to answer a question only this
one needed asked.

**Where the anchor belongs is explicitly the second consumer's decision.**
The candidates, as the first consumer sees them:

1. *On the trait* — honest, but every backend inherits a method most of them
   can implement only by storing a value they were going to store anyway.
2. *In the platform layer's automatic wiring* — `App::run` anchors any
   registered `PlatformViews` to the window it creates. The friction: the
   application's registry is closed by then, so the platform layer would need
   its own hook, and the first consumer had no reason to build one.
3. *Inherent, as today* — costs nothing, but every future backend re-answers
   the question and an application that switches backends relearns it.

Until a second consumer exists, option 3 is the recorded state, and this
file is the place the decision gets recorded when it does.

## The X11 implementation's load-bearing properties

Not API — the reasons a future backend (or a reviewer of this one) needs:

- **Its own `Display` connection.** X11 window IDs are server-global, not
  per-connection, so a child of a window created on winit's connection can
  be created, moved and destroyed from a second connection without either
  side holding a pointer into the other. That is what makes the explicit
  anchor workable at all: the service borrows only an XID — a `u64` valid as
  long as the host window is mapped — never winit's `Display*`, whose
  lifetime is the window's, i.e. shorter than the service's.
- **One thread.** An X11 `Display` is not thread-safe and the service is not
  `Send`/`Sync` on purpose: created, anchored, called and dropped on the
  application's thread, which is the frame loop's. The trait's sequential,
  never-re-entered contract is the same rule the plugin ABI states.
- **`dlopen`, not a link-time dependency** — the same decision softbuffer's
  `x11-dlopen` feature makes one dependency over. A Wayland-only session
  gets `connect()` → `Err`, and the documented fallback is
  `NoPlatformViews`: embedding *unsupported*, not the application failing to
  start.

## Known limits, stated rather than implied

- **Logical == physical pixels.** `create` is documented in logical pixels;
  X11 geometry is device pixels, and the implementation passes the numbers
  through. Exactly right at scale factor 1.0, exactly wrong under XWayland
  fractional scaling. The fix — learn the host's scale at anchor time — is
  open; a second consumer that is not a fullscreen video view will force it.
- **The background assumes a TrueColor visual** with the standard channel
  masks (`0xff0000`/`0x00ff00`/`0x0000ff`) — every default visual of the
  last twenty years, and Xvfb's. An 8-bit PseudoColor screen would need a
  colormap allocation nothing here does.
- **Reparenting on re-anchor is not implemented.** A second anchor moves
  where *new* views parent; existing ones keep their old parent until
  destroyed, because `XReparentWindow` brings event-ordering traps nothing
  here has a reason to pay for yet.
- **No clipping, no opacity, own pointer routing** — inherent to embedding,
  from the trait's doc; listed here so nobody reading this file has to
  triangulate.

## What the second consumer should bring

The shape is now *known* rather than guessed, in the narrow sense PENDING
§6.2 meant: one real backend, one real lifecycle, assertions against the X
server's own answers (`XQueryTree` for existence, `XGetGeometry` for
position) rather than the implementation's bookkeeping. What would settle
the remaining questions:

- A **content-carrying** kind (a video surface, a web view) — forces the
  scale-factor fix, the visual question, and probably the message channel.
- A **non-Linux** backend — forces the anchor decision, because Android and
  iOS answer "which window" with a different shape entirely (an activity, a
  view controller).
- A **widget-tree integration** — a platform-view *widget* that owns a
  handle and feeds layout rects to `set_bounds`; the seam is proven, the
  tree plumbing is deliberately not built until something needs it.

Until one of those exists, `docs/production-readiness/STATUS.md` FW-20
records the state, and this file is the shape's declaration.

## Settled (2026-11-03)

The three things this page said would settle the open questions all exist
now, and each answered its question:

- **The widget-tree integration** — `vieww_widget::PlatformView` and its
  `PlatformViewController`. The render object (`RenderPlatformView`) opts in
  to a new `RenderObject::wants_placement`/`placed(bounds)` hook; the frame
  driver calls it after every layout with the object's window-relative
  bounds, keeping the list of askers by the tree's structure stamp so a frame
  pays only for the views on screen. The controller owns the handle (a
  rebuild does not recreate the native view), creates it on the first
  placement, sends `set_bounds` only for a changed rectangle, and drops it
  when the last render object holding it goes. `crates/vieww/tests/platform_view.rs`
  pins create/move/no-op/dispose/refusal against a recording service.
- **Non-Linux backends** — Win32 child windows (`platform_views_win32.rs`,
  kind `win32-window`) and AppKit subviews (`platform_views_appkit.rs`, kind
  `appkit-view`, an `NSBox` per view with flipped/unflipped parent
  conversion). Both type-checked from Linux; their first runs are the
  Windows and macOS CI legs.
- **The anchor decision: the platform layer's wiring** (option 2).
  `services::platform()` registers `OsPlatformViews`, which opens its
  backend lazily; `App` anchors it to the application's first window when it
  creates it and forwards `ScaleFactorChanged`. The trait is unchanged, the
  backends keep their inherent `anchor` for embedders without an `App`, and a
  session that cannot host views (a Wayland window, no X11) refuses with the
  reason in `OsPlatformViews::refusal()` rather than failing to start.
- **The scale-factor fix** — logical bounds are placed in device pixels at
  the host's scale on X11 and Win32 (AppKit lays out in points already), and
  a scale change re-places live views.

Still open, and stated: Android and iOS (an activity / view controller is
the anchor there, not a window handle), a content-carrying kind (video,
web view) and its message channel, the TrueColor-visual assumption, and
reparenting on re-anchor.

