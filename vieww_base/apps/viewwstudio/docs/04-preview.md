# The preview

The preview pane renders your buffer's `screen()` inside a
device frame you can switch, so a layout gets judged at the
size it will actually be seen at rather than at whatever
your editor window happens to be.

## Platforms

Three platforms, in the switcher above the frame:

```text
iOS
Android
Desktop
```

Switching platform changes the frame's size and safe-area
insets, and it changes which theme conventions apply — an
Android screen defaults to a different motion and elevation
feel than an iOS one, and `ThemeData::of(ctx)` reflects
that difference back into your widgets without you writing
a single `if platform ==` anywhere in `screen()`.

## What is simulated, and what is real

The frame, the safe area, and the theme are simulated: they
are numbers this studio applies, chosen to match each
platform's real conventions closely enough that a layout
decision made here holds up on the device. They are not the
platform itself.

What is real is everything your widget tree does with those
numbers — the actual layout algorithm, the actual paint, the
actual text shaping and font fallback. A `Flex` that
overflows here overflows on the device too; a `Text` that
wraps at the wrong width here wraps the same way there. The
preview is not a mockup of your screen, it is your screen,
looked at through a simulated window.

## What the preview cannot show you

A handful of things only exist once real platform services
are behind the widget tree: a camera feed, a GPS fix, a
push notification, a background task that keeps running
after the app is backgrounded. `screen()` can still be
built and looked at with these missing — pass in sample
data the way `LandingScreen` does above — but the preview
will not exercise the real service underneath. That is what
**Build and Run**, on the next page, is for.

## The same preview, from the command line

Everything this page describes is the studio's *engine*, and
the engine has a second door: `viewwstudio-cli`. The same
compile, the same device table, the same out-of-process
preview — behind commands a script can drive:

```console
$ viewwstudio-cli render \
    src/screens/home.say --device mobile
wrote target/render/home.png
  (393×852, rustc 0.3s)

$ viewwstudio-cli preview \
    --device android --landscape
previewing src/screens/home.say
  as Android phone (915 × 412)
  at http://127.0.0.1:8788

$ viewwstudio-cli render flow.say \
    --all-screens
$ viewwstudio-cli animate flow.say \
    --fps 60 --duration 5s
$ viewwstudio-cli test
```

`render` is the frame at a size; `preview` serves the live
screen for a browser and forwards your pointer, scroll and
key events into the same event path the pane uses;
`--device` is the switcher above the frame, as a name;
`--all-screens` is one screenshot per screen the file
defines; `test` is the pane's eyeball check made mechanical
— deterministic render, no overflow, inputs the screen must
survive, and a golden to compare against. `viewwstudio-cli`
with no arguments prints the whole surface, and
`viewwstudio::cli`'s module doc is the honest table of what
is shared with the app and what the honest limits are.

## Showing it to someone

Four things turn a preview into something you can hand over. Each is a
command in the studio (Render menu, or the palette) and a flag on the CLI —
the studio runs the CLI to do them, so the two always agree.

| In the studio (Render menu)        | On the command line                                 |
|------------------------------------|-----------------------------------------------------|
| Preview: Save Framed Screenshot    | `render home.say --device phone --frame`            |
| Preview: Save Device Matrix Image  | `render home.say --matrix` (or `--devices a,b,c`)   |
| Preview: Start or Stop Recording   | `animate home.say --device phone --frame --script recording.script` |
| Preview: Share on Network (QR Code)| `preview home.say --share`                          |

**Frames.** `--frame` draws the pane's own device around the screen — the
same bezel, notch, punch hole and home indicator — into the PNG or GIF.
A PNG keeps a transparent margin for the shadow; `--background #ffffff`
puts it on a colour.

**Interaction.** `render` and `animate` take steps that happen before the
picture, or during the animation:

```console
$ viewwstudio-cli render home.say \
    --device phone --frame --tap Details
$ viewwstudio-cli animate home.say \
    --device android --frame \
    --do 'tap "Add one" @0.5s' \
    --do 'tap "Add one"' \
    --do 'tap "Details"'
```

A target is a label (what a screen reader would say — a button's text) or
a point in device points (`120,300`). Steps are `tap`, `hold … for 800ms`,
`drag 200,600 -> 200,200 over 300ms`, `scroll -300 [at x,y]`,
`key enter|tab|escape|…` and `type "text"`, each with an optional `@time`.
`--script file` reads one step per line.

**Recording.** Start Recording in the studio, use the preview, and Stop:
the studio writes `.vieww/recordings/recording-<time>.gif` (framed) and a
`.script` beside it in the syntax above — so the same interaction can be
replayed at any device, theme or frame rate with `animate --script`.
Recording keeps what the isolated preview draws, so starting one turns
isolation on.

**Sharing.** `--share` serves the preview to your local network as well as
to this machine, and shows a QR code — in the terminal, in Output, and in
the browser page. A phone on the same Wi-Fi scans it and gets the screen
edge to edge, driven by touch. Everyone who opens the link drives the same
preview. For a link that works anywhere, export the web build
(`export --format web`) and host it.
