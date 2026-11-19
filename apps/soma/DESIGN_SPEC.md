# DESIGN_SPEC.md — Soma (working name)

Reference: the current Soma home screen (`src/screens/home.rs`, palette in `src/lib.rs`).
The target is Tailwind + shadcn/ui conventions. Values map 1:1 to the existing vieww
constants, so the native and web builds stay identical.

---

## 1. Design tokens

### 1.1 Colour (shadcn CSS variables, dark-only)

```css
/* globals.css: :root.dark (HSL, shadcn format) */
--background:        222 38% 5%;    /* #080B12  GROUND           */
--background-top:    219 40% 10%;   /* #101725  gradient stop 0  */
--card:              220 36% 12%;   /* #131A28  CARD             */
--card-glass:        221 37% 10%;   /* #101623 @ 95%  CARD_GLASS */
--popover:           217 44% 7%;    /* #0A101A  callout chip     */
--foreground:        216 41% 95%;   /* #EDF1F7  INK              */
--muted-foreground:  216 17% 64%;   /* #93A0B4  MUTED            */
--border:            0 0% 100% / 0.08;  /* EDGE                  */
--input:             0 0% 100% / 0.06;
--ring:              169 79% 64%;   /* = primary                 */
--primary:           169 79% 64%;   /* #59EBD1  ACCENT (teal)    */
--primary-foreground:171 54% 5%;    /* #06140F                   */
--destructive:       7 81% 70%;     /* #F08274  CORAL            */
/* topic-kind hues (badges, tags) */
--kind-body:      var(--primary);
--kind-symptom:   var(--destructive);
--kind-condition: var(--destructive);
--kind-test:      224 87% 72%;   /* #7AA2F7 */
--kind-treatment: 39 81% 69%;    /* #F0C46E */
--kind-medication:315 31% 60%;   /* #B87AA9 */
--kind-general:   var(--muted-foreground);
```

| Role | Tailwind class |
|---|---|
| Page ground | `bg-background` + `bg-gradient-to-b from-[hsl(var(--background-top))] via-background to-background` |
| Glass card | `bg-[hsl(var(--card-glass)/0.95)] backdrop-blur-xl border border-border shadow-[0_16px_34px_rgba(0,0,0,0.47)]` |
| Raised row | `bg-white/[0.04] hover:bg-white/[0.07] active:bg-white/[0.10]` |
| Primary text | `text-foreground` |
| Secondary text | `text-muted-foreground` |
| Accent text/ring | `text-primary`, `ring-2 ring-ring ring-offset-0` |
| Kind tint | `bg-[hsl(var(--kind-X)/0.16)] text-[hsl(var(--kind-X))]` |
| Scene vignette | `bg-gradient-to-b from-[#0D1422]/60 via-transparent via-20% to-background/65` |

### 1.2 Typography (Geist; minimum 11 px, replacing today's 8–9 px)

```js
// tailwind.config: fontFamily.sans = ['Geist', ...defaultTheme.fontFamily.sans]
fontSize: {
  'micro':  ['11px', { lineHeight: '14px', letterSpacing: '0.08em' }], // eyebrows, tags (was 8–9.5)
  'caption':['12px', { lineHeight: '16px', letterSpacing: '0.02em' }], // meta, citations (was 10–10.5)
  'body':   ['14px', { lineHeight: '21px' }],                          // article
  'row':    ['15px', { lineHeight: '20px' }],                          // result title, input
  'title':  ['20px', { lineHeight: '26px', letterSpacing: '-0.01em' }],// topic title (was 17)
  'brand':  ['22px', { lineHeight: '26px', letterSpacing: '0.24em' }], // wordmark
}
```

| Use | Classes |
|---|---|
| Wordmark | `text-brand font-bold uppercase` |
| Eyebrow ("SOURCE", "NEARBY") | `text-micro font-semibold uppercase text-muted-foreground` |
| Result title | `text-row font-medium text-foreground line-clamp-2` |
| Meta line | `text-caption text-muted-foreground` |
| Topic title | `text-title font-bold text-foreground` |
| Article | `prose prose-invert prose-sm prose-headings:text-foreground prose-a:text-primary max-w-none` |

### 1.3 Spacing, radius, motion

| Token | Value |
|---|---|
| Screen gutter | `px-3` (12) mobile, `md:px-6` |
| Card padding | `px-3.5 py-2.5` (14/10) |
| Stack gaps | `gap-1` (rows), `gap-2` (meta), `gap-2.5` (card sections), `gap-4` (sheet sections) |
| Radius | card `rounded-3xl` (24), figure `rounded-2xl` (14 → 16), row `rounded-xl` (12), chip `rounded-lg` (9 → 8), badge `rounded-md` |
| Hit target | min `h-11 min-w-11` (44 px) for every tappable element (today: clear button about 21 px, chips about 22 px) |
| Motion | card position and height: spring (stiffness 170, damping 26), mirroring the `pill`/`panel` springs; `motion-reduce:transition-none` and no idle sway |
| Elevation (z) | scene `z-0`, vignette `z-10`, callout `z-20`, top bar `z-30`, floating card `z-40`, sheet/dialog `z-50` |

---

## 2. ASCII layout

### 2.1 Mobile, `< md` (current phone surface 402×874): one stack, card floats

```
┌──────────────────────────────┐  Layer 0 BodyViewport (inset-0)
│ SOMA            [i] [⟲]      │  Layer 3 TopBar  (top-4 inset-x-5)
│ 11,043 topics · offline      │
│                              │
│          ╭─────────╮         │  Layer 2 OrganCallout (abs @ projected anchor)
│          │● Heart  │         │
│          ╰────┬────╯         │
│            (body)            │
│              ⊙               │
│                              │
│ ╭──────────────────────────╮ │  Layer 4 FloatingSearchCard
│ │ 🔍 Ask the body…    [×]  │ │   position = lerp(low, high, pill)
│ │──────────────────────────│ │   low : bottom-[18px]
│ │ [S] Chest pain       ›   │ │   high: top-24
│ │ [D] Angina           ›   │ │   height: 72px → 44vh (results) / 55vh (topic)
│ │ [B] Heart            ›   │ │
│ ╰──────────────────────────╯ │
│ [+][−][⟳][◎]                 │  CameraControls (bottom-left, above card)
│ ⓘ Not medical advice · 911  │  Layer 4 DisclaimerBar (sticky, collapsible)
└──────────────────────────────┘
Gestures on the body: drag = orbit, pinch = zoom, 2-finger = pan, double-tap = re-frame.
Answer card = text only (matched passage → article → nearby → source), scrollable.
Rule: card.high when anchor.y_world < 0.95 (target in lower body);
      card.low  when anchor.y_world > 1.40 (target in head/neck); glide between.
```

### 2.2 Tablet, `md` (768–1023): the card docks as a bottom sheet with a snap point

```
┌──────────────────────────────────────────────┐
│ SOMA · 11,043 topics                  [i][⟲] │
│                                              │
│                 (body, centred)              │
│                       ⊙ ── ● Kidneys         │
│                                              │
├──────────────────────────────────────────────┤ ← Sheet snap: 40% | 70%
│ 🔍 query…                               [×]  │
│ [B] Kidneys   [D] Kidney stones   [X] eGFR   │  results: grid-cols-2
└──────────────────────────────────────────────┘
```

### 2.3 Desktop, `lg` (≥1024): two columns, so the body and the answer never overlap

```
┌──────────────────────────────┬─────────────────────────┐
│ SOMA · 11,043 topics         │ 🔍 Ask the body…    [×] │  grid lg:grid-cols-[1fr_420px]
│                              │─────────────────────────│  xl:grid-cols-[1fr_480px]
│                              │ ‹ Back   Condition·Heart│
│          (body)              │ Angina                  │
│             ⊙ ── ● Heart     │ ▌matched passage (hl)   │
│        (3D heart element,    │ article (prose, scroll) │
│         framed 40–60%)       │                         │
│                              │ NEARBY  [Lungs][Blood]  │
│ [−][+][⟲]  (camera controls) │ SOURCE  Cleveland Clinic│
└──────────────────────────────┴─────────────────────────┘
```

---

## 3. Component manifest

Each component is isolated, controlled (state passed in), and emits events. `Organ`, `Topic`
and `Hit` mirror `db.rs` and `search.rs`.

- **AppShell**: `{ children, layout: 'stack' | 'split' }` (from the breakpoint). Holds the z-layers. States: `rest` · `searching` · `topic`.
- **BodyViewport**: `{ frame: ImageBitmap, blur: 0..1, xray: 0..1, selectedOrganKey?: string, onPick(organKey: string) }`. States: `idle` (blur 1, sway off when `prefers-reduced-motion`) · `preview` (blur `1-strength`) · `focused` (blur 0, xray 1) · `picking` (pressed ring at the tap point). Accessible name: "3D body. Tap a region to open it." Keyboard: organ list fallback via `Tab`.
- **OrganCallout**: `{ organ: Organ, screen: {x, y}, opacity: 0..1, citation: { label: string; url: string }, onOpen() }`. States: `hidden` (opacity < 0.08) · `preview` (name only, while typing) · `pinned` (name + **query-specific** citation, e.g. "Angina → Heart · Cleveland Clinic").
- **TopBar**: `{ topicCount: number, onInfo(), onReset() }`. Reset is disabled in `rest`.
- **FloatingSearchCard**: `{ mode: 'rest' | 'results' | 'topic', pill: 0..1, panel: 0..1, children }`. States: `collapsed` (72 px) · `expanding` · `results` (44 vh, ≤430) · `topic` (55 vh, ≤520) · `docked` (md+, no float).
- **SearchInput** (shadcn `Input` + `Command`): `{ value, placeholder, onChange(v), onClear(), onSubmit(), mode: 'text' | 'regex' | 'regex-invalid' }`. States: `empty` · `focused` (`ring-2 ring-ring`) · `typing` · `regex` (shows `ModeBadge`) · `regex-invalid` (coral helper "searching literally"). Debounce 120 ms.
- **ModeBadge**: `{ mode: 'regex' | 'literal' }`. A `Badge variant="outline"` in `text-micro`.
- **HintChips**: `{ items: string[], onPick(q) }`. Horizontal `ScrollArea`; chip uses `Button size="sm" variant="secondary"`; 44 px hit area.
- **ResultList**: `{ hits: Hit[], total: number, activeIndex, onSelect(i), onLoadMore() }`. States: `loading` (3 `Skeleton` rows) · `empty` (EmptyState) · `list` (virtualised, all 24 hits; today only 5 render) · `keyboard-nav` (↑/↓/Enter).
- **ResultRow**: `{ topic: Topic, organName: string, flags: MatchFlags, active: boolean, onSelect() }`. States: `default` · `hover` · `pressed` · `active` (`bg-primary/10 ring-1 ring-primary/40`).
- **KindBadge**: `{ kind: 'B'|'S'|'D'|'X'|'T'|'P'|'G'|'M' }`. 24×24 `rounded-md`, a distinct letter per kind (fix: P currently renders as "T").
- **MatchTag**: `{ type: 'text' | 'meaning' | 'graph' | 'expanded' }` with `Tooltip` text "why this result".
- **AnswerCard** (text only, no images): `{ topic: Topic, organ?: Organ, matchedPassage?: string, body: string, related: Topic[], onBack(), onRelated(i) }`. Order: header, then matched passage, then article, then related, then source. States: `loading` (first body parse, Skeleton) · `ready` · `no-article` (region tap: organ summary only). It has its own `ScrollArea` (separate from ResultList) with a bottom fade `bg-gradient-to-t from-[hsl(var(--card-glass))]` when more text remains.
- **TopicHeader**: `{ kind, organName, title, onBack }`. Back is a `Button variant="ghost" size="icon"`.
- **MatchedPassage**: `{ text: string, terms: string[] }`. `border-l-2 border-primary pl-3 text-body`, terms shown with `mark.bg-primary/20 text-foreground`. Always visible without scrolling.
- **ArticleBody**: `{ markdown: string, highlight?: string[] }`. Highlights query terms with `mark.bg-primary/20`.
- **RelatedChips**: `{ items: { id, title, kind, organKey, viaGraph: boolean }[], onPick(id) }`. Picking a chip also flies the camera to that organ's 3D element. Full title in `Tooltip`, `line-clamp-1`.
- **SourceCitation**: `{ publisher, title, url, retrievedAt }`. Link opens externally; `text-caption`.
- **EmptyState**: `{ query, suggestions: string[] }`. Spelling suggestions from the term-prefix index.
- **DisclaimerBar** (new, required): `{ emergency: boolean, region: 'US' | 'IN' | … }`. States: `default` ("Information only, not medical advice") · `emergency` (coral `Alert` with a local emergency number, triggered by red-flag queries such as "chest pain" or "can't breathe").
- **CameraGestureLayer** (all sizes, over BodyViewport): `{ onOrbit(dYaw, dPitch), onZoom(factor, focal), onPan(dx, dy), onDoubleTap(), onPick(x, y) }`. Touch: 1-finger drag orbits (pitch clamped ±60°), pinch zooms (distance 0.25–4), 2-finger drag pans (clamped to the body's bounds). Pointer: drag, wheel, right or Shift-drag, and arrow and +/− keys. A move over 8 px cancels the pick. States: `idle` · `orbiting` · `zooming` · `panning` · `flying` (animated to an answer; a gesture interrupts it smoothly, without a jump).
- **CameraControls** (floating, bottom-left above the card; `lg`: in the left column): `{ onZoomIn(), onZoomOut(), onRotate(-90|90), onRecenter(), canRecenter }`. A vertical `ToggleGroup` of 44 px icon buttons. Recenter appears only after a manual move.
- **ElementFocus** (3D, not DOM; drives the camera): `{ organKey, bbox, fill: 0.4..0.6, preferSide: 'current' | 'nearest-visible' }`. Each organ's 3D element is grown from its picture, so the zoom lands on it and the answer card never shows a flat image.
