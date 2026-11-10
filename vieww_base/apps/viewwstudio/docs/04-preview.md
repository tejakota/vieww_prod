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
$ viewwstudio-cli render src/screens/home.say --device mobile
wrote target/render/home.png (393×852, rustc 0.3s)

$ viewwstudio-cli preview --device android --landscape
previewing src/screens/home.say as Android phone (915 × 412) at http://127.0.0.1:8788

$ viewwstudio-cli render flow.say --all-screens
$ viewwstudio-cli animate flow.say --fps 60 --duration 5s
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
