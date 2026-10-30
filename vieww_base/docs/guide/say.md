# Say

Say is the English-facing screen language: a `.say` file describes a
screen in sentences, and `vieww-say-codegen` turns it into one Rust file
that vieww Studio compiles exactly like a hand-written screen. The
generated Rust is readable and every statement in it carries a
`// say: file:line` marker back to the sentence that produced it, so the
step from Say to Rust is a step, not a cliff.

```say
-- say-language: 1

keep a whole number called count starting at 0

screen "Home":
    a column, spaced 16, children aligned to the start:
        a heading "Counter"
        a label "Tapped \(count) times"
        a button "Add one" which when tapped:
            add 1 to count
```

The rules: four spaces per indent level, a line ending in `:` opens a
block, and `--` starts a comment. The first line names the language
version.

## A file

| Line | Means |
|---|---|
| `keep a whole number called n starting at 0` | State. Also `a number`, `a yes-or-no`, `text`, `a list of text`, `a list of numbers`. |
| `the app is called "Name"` | The app's name. |
| `the theme:` | Colour slots over the host's theme (below). |
| `rust:` | Rust items passed into the generated file as written (below). |
| `screen "Home":` | A screen. The first screen is where the app starts. Add `with a fade transition` or `with a slide transition` before the colon. |

## Widgets

Every widget line starts with `a`, `an` or `the`; properties follow after
commas; a trailing `:` opens children or, with `which when tapped:`, the
actions of a tap.

| Phrase | Notes |
|---|---|
| `a column`, `a row`, `a stack`, `a center`, `a card` | Layout. `spaced N`, `padded N`, `children aligned to the start`. |
| `a fixed area of W by H` | A sized box. |
| `a heading "…"`, `a text "…"` (`a label`) | `\(name)` interpolates state. `size N`, `bold`, `color #RRGGBB`. |
| `a button "…"`, `a floating action button with the plus icon` | `which when tapped:` |
| `a text field bound to s`, `a switch bound to b`, `a checkbox bound to b`, `a slider bound to n` | Two-way bound to state. |
| `a list of names`, `for each name in names:` | Repetition. `each row N`. |
| `an empty state "…" with the check icon`, `an icon check icon` | The icons: check, close, plus, minus, and the left, right, up, down, back and forward chevrons. |
| `an image "photos/cat.png"` | A PNG or JPEG file. `N wide`, `N tall`, `described as "…"` for screen readers. |
| `a scroll view:` | Its children in a column that scrolls when they are taller than the screen. |
| `a set of tabs:` with `a tab "Name":` blocks under it | A tab bar over the selected tab's children. |

Properties that wrap any widget: `padded N`, `color …`, `radius N`,
`N wide`, `N tall`, `pinned top N`, `only if <condition>`. Colours are
`#RRGGBB`, a name (`red`), or a theme slot: `with the primary color`.

### Images

The path is read at run time, relative to the folder the `.say` file is
in when the studio previews it, and relative to the working folder when a
built app runs. A path may not climb out with `..`. A file that is
missing or is not a picture shows what went wrong in its place instead of
failing the screen. Bundling images into an exported app is ST-15's asset
pipeline.

### Scroll views and tabs

Neither needs any state declared: the generated file keeps a scroll
view's offset and a set of tabs' selection itself, named after the line
the widget is on. Only the selected tab is built; state a tab changes
lives in `keep` state and is there when the tab comes back.

```say
screen "Profile":
    a set of tabs:
        a tab "Posts":
            a scroll view:
                for each post in posts:
                    a card, padded 16:
                        a text "\(post)"
        a tab "Likes":
            a text "\(likes) likes"
```

## Actions

`set n to E`, `add E to n`, `subtract E from n`, `toggle b`,
`append E to L`, `remove the item at E from L`, `clear L`,
`open the screen "X"`, `go back`, `show a snackbar "M"`,
`open a dialog titled "T":`, `close this dialog`. Any action can end in
`only if <condition>`.

## Transitions

```say
screen "Details" with a slide transition:
```

A transition plays when its screen comes up, opened or revealed by
`go back`: `fade` fades it in, `slide` brings it in from the right edge.
Both take 300 ms with an ease-out curve.

## The theme

```say
the theme:
    primary is #6750A4
    on primary is #FFFFFF
    the surface color is #FFFBFE
```

The slots are `primary`, `on primary`, `surface`, `on surface`,
`surface variant`, `on surface variant`, `outline`, `error` and
`on error` — the `ColorScheme` fields of the same names. Slots the block
does not name keep the host's values. Every widget that reads the theme
follows it, and so does `with the primary color`.

## Rust inside Say

Where Say runs out, write Rust. At the top level a `rust:` block holds
items — functions, types, `use` lines — passed into the generated file
as written. Inside a screen, a `rust:` block (or a fenced block) is an
expression that becomes a widget, and it can read state by name:

````say
keep a whole number called rating starting at 3

rust:
    fn stars(n: i64) -> String {
        "*".repeat(n.clamp(0, 5) as usize)
    }

screen "Home":
    a column, spaced 8:
        rust:
            Text::new(stars(rating)).size(24.0)
        ```rust
        Text::new(format!("{rating} of 5"))
        ```
````

Inside a block the code can also use `ctx` (the build context), `theme`
and `handle` (the state handle `say::act` takes). Say does not read the
code: rustc does, and its errors point into the generated file beside the
`// say:` marker for the block.

## Compiling

The studio compiles a `.say` buffer on Render. `saygen file.say` prints
the generated Rust, and a scaffolded project compiles `src/screens/*.say`
in its `build.rs`. From code:

```rust
let generated = vieww_say_codegen::compile("home.say", &source)?;
let generated = vieww_say_codegen::compile_with(
    "home.say",
    &source,
    &vieww_say_codegen::Options { asset_root: "assets".into() },
)?;
```

The same source and the same options give the same bytes, every time.

## Tests

`crates/vieww-say-codegen/tests/fixtures` holds one `.say` file per
phrase with the Rust it generates, committed. A unit test fails when the
generator's output changes, `tests/vocabulary.rs` compiles every fixture
and drives it in a frame driver, and the studio's `render_matrix` example
takes each through the studio's own compile and load path.
