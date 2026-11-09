# mytlabs

mytlabs — meet your thoughts labs — the company page, as a vieww widget tree
rendered to DOM.

```sh
# the page as static HTML, on the host — no browser, no wasm toolchain
cargo run -p mytlabs --example snapshot          # -> target/mytlabs-snapshot/

# the real thing
apps/mytlabs/build-site.sh                       # -> apps/mytlabs/dist/
```

## What it is

The page that presents the lab: the vision (every person's dream thought,
made true — a product to use, a framework to create with), the seven products
under the lab's `apps/` (viewwstudio, wright, upxcale, snapsearch, three,
vavlt, twin — each card the app's own README in one sentence, with the
repository's own word for its status), the framework those products run on
(the guide's own first-screen sample, verbatim), and the mission (understand
the person; unite the product).

It is built the way `apps/wrightsite` builds Wright's page: as a vieww
widget tree — the same widget catalogue, the same layout algorithm — walked
by `vieww-platform-web-dom`, which emits elements and CSS. There is no canvas
half and no island: everything this page does, a browser already does, so
the tree is the whole page and the DOM walk is the whole renderer.

## What is in it

| file | what it is |
|---|---|
| `src/lib.rs` | the page. `page()` builds the tree; `start()` is what wasm-bindgen calls |
| `index.html` | the host page: fonts, the stylesheet (splash, staged arrival, scroll-driven reveals, hover, fluid scale), the fallback copy, and one `<script type="module">` |
| `examples/snapshot.rs` | the same tree to static HTML on the host — the fast review loop, and a page a crawler can read |
| `build-site.sh` | cargo → wasm-bindgen → a directory a static host can serve |

## The palette

mytlabs' own, not vieww's and not Wright's: the ground is the indigo night a
thought is thought in, the accent the violet of the spark the lab turns
thoughts into, the cyan a second gradient stop. The seven product tints are
the palette's third register — one per product, registered as colour tokens
so the light scheme darkens them with everything else.

The mark is the thought-trail: three dots ascending, and the spark they
become. The splash draws it — each dot landing after the one below it, the
spark arriving last — and hands over to a hero standing in the same place.

## Serving it

Any static host. Two headers matter:

- `.wasm` → `Content-Type: application/wasm`, or the browser will not
  stream-compile it;
- `.wasm` → `Content-Encoding: br` or gzip — the wire is the wait.

On Cloudflare Workers, `[site] bucket = "apps/mytlabs/dist"` (or a Pages
project pointed at the same directory) does both by default.

## What a canvas costs, and why this page has none

The served page is DOM, not pixels: text is selectable, the find bar sees it,
a crawler reads it, and `:hover`, sticky positioning and scroll-driven
reveals are the browser's own, run at zero cost to this application. The
cost of that choice is paid in review: a rasterised page can be drawn to a
PNG on the host, and this one cannot — which is what the snapshot example is
for. It walks the same tree with the same `vieww-platform-web-dom` the live
mount uses and writes the page as static HTML, which screenshots in either
colour scheme and reads exactly as the browser will lay it out.

The `index.html` fallback carries the page's real copy for the same reason:
a screen reader reads it, a crawler indexes it, and a browser with
WebAssembly turned off shows it. If the page's copy changes, change it in
both places — they are two renderings of one text, and the fallback silently
going stale is the failure mode to watch for.
