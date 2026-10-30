"""The web AudioPlayer in a real browser (AU-01).

Headless Chromium loads the audio_smoke example's wasm with
vieww-audio.js as its "vieww_audio" import module;
vieww_platform_web::services() registers the engine, a tone plays, and
the AudioWorklet pulls rendered blocks from the wasm.

    cargo build --release --target wasm32-unknown-unknown -p vieww-platform-web --example audio_smoke
    python3 crates/vieww-platform-web/web-test/audio_smoke.py

The wasm-bindgen placeholder imports (web-sys, unused on this path) are
stubs that throw: a call fails the test instead of passing silently.
"""

import functools
import http.server
import json
import os
import sys
import threading

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
WASM = os.environ.get("SMOKE_WASM") or os.path.join(
    ROOT, "target", "wasm32-unknown-unknown", "release", "examples", "audio_smoke.wasm")
SHIM = os.path.join(ROOT, "crates", "vieww-audio", "web", "vieww-audio.js")

PAGE = b"""<!doctype html><meta charset=utf-8><script type=module>
import * as shim from "./vieww-audio.js";
const stub = new Proxy({}, { get: (_, n) => () => { throw new Error("unexpected wasm-bindgen import " + String(n)); } });
let renders = 0;
const { instance } = await WebAssembly.instantiateStreaming(fetch("smoke.wasm"), {
  vieww_audio: shim, __wbindgen_placeholder__: stub, __wbindgen_externref_xform__: stub,
});
const ex = instance.exports;
const counted = { ...ex, memory: ex.memory,
  vieww_audio_render: (...a) => { renders++; return ex.vieww_audio_render(...a); } };
shim.attach(counted, { autoResume: true });
const states = [];
shim.onAudioState((_, s) => states.push(s));
const registered = ex.smoke_registered();
const played = ex.smoke_play();
setTimeout(() => { window.result = { registered, played, renders, states }; }, 1500);
</script>"""


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        routes = {"/": (PAGE, "text/html"), "/vieww-audio.js": (SHIM, "text/javascript"),
                  "/smoke.wasm": (WASM, "application/wasm")}
        if self.path not in routes:
            self.send_response(404)
            self.end_headers()
            return
        body, kind = routes[self.path]
        if not isinstance(body, bytes):
            with open(body, "rb") as f:
                body = f.read()
        self.send_response(200)
        self.send_header("content-type", kind)
        self.end_headers()
        self.wfile.write(body)


def main():
    from playwright.sync_api import sync_playwright

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = f"http://127.0.0.1:{server.server_address[1]}/"
    failures = []
    with sync_playwright() as pw:
        browser = pw.chromium.launch(args=["--autoplay-policy=no-user-gesture-required"])
        page = browser.new_page()
        errors = []
        page.on("pageerror", lambda e: errors.append(str(e)))
        page.on("console", lambda m: m.type == "error" and errors.append(m.text))
        page.goto(url)
        page.wait_for_function("() => window.result", timeout=20000)
        r = page.evaluate("() => window.result")
        browser.close()
    server.shutdown()
    print("audio-smoke:", json.dumps(r), ("errors: " + " | ".join(errors)) if errors else "")
    if r["registered"] != 1:
        failures.append("services() registered no AudioPlayer")
    if r["played"] != 1:
        failures.append("the player refused the tone")
    if r["renders"] <= 10:
        failures.append(f"the worklet pulled {r['renders']} blocks in 1.5 s")
    if errors:
        failures.append("page errors")
    for f in failures:
        print("audio-smoke: FAIL", f)
    if failures:
        return 1
    print("audio-smoke: the web AudioPlayer played through Web Audio")
    return 0


if __name__ == "__main__":
    sys.exit(main())
