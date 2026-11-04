#!/usr/bin/env python3
"""The WebGPU backend against the CPU rasterizer, in a real browser (FW-09).

    cargo build --release --target wasm32-unknown-unknown \
        -p vieww-hal --example webgpu_scene --features webgpu
    python3 crates/vieww-hal/web-test/webgpu_parity.py

Headless Chromium (or the engine VIEWW_WEB_BROWSER names — the same
three-engine convention examples/test-web/verify_web.py set) loads the
webgpu_scene cdylib with crates/vieww-hal/web/vieww-gpu-web.js as its
"vieww_gpu_web" import module — the raw-import bridge, no wasm-bindgen,
exactly the shape the audio smoke test drives. The wasm renders the same
five scenes twice — once on the CPU rasterizer (the byte-certified canvas
path), once through the WebGPU backend — and compares the frames under
the workspace's performance contract: interiors within 2 per channel,
disagreements only where the reference itself has an edge.

The page reports one verdict per scene (status / worst / interior / edge /
exact) plus whether the last frame presented into the canvas. This script
asserts, per engine:

* every scene's status is 1 (interior violations == 0);
* the empty scene is byte-exact (nothing was drawn, so there is no
  antialiasing to disagree about);
* the presentation path ran (presented == 1).

An engine without WebGPU (no navigator.gpu, or requestAdapter() refusing —
Firefox and WebKitGTK on the CI runners today) is a SKIP, loudly printed,
exit code 3: the canvas-CPU backend remains the answer there, and this run
certifies nothing. Exit code 2 is the usage error, 1 a real failure.
"""

import http.server
import json
import os
import sys
import threading

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.environ.get("VIEWW_WEBGPU_ROOT") or os.path.abspath(
    os.path.join(HERE, "..", "..", ".."))
WASM = os.environ.get("WEBGPU_WASM") or os.path.join(
    ROOT, "target", "wasm32-unknown-unknown", "release", "examples",
    "webgpu_scene.wasm")
SHIM = os.path.join(ROOT, "crates", "vieww-hal", "web", "vieww-gpu-web.js")
ENGINE = os.environ.get("VIEWW_WEB_BROWSER", "chromium")

# The page: load the shim, instantiate the wasm with it as the import
# module, and poll the driver once per animation frame until its state
# settles. The failure message crosses the boundary the two-call way —
# length first, then into a page this side grew for it (the wasm heap's
# own allocator never runs between the grow and the read, so the fresh
# page is this call's scratch and nothing else's).
PAGE = b"""<!doctype html><meta charset=utf-8>
<canvas id="vieww-canvas" width="120" height="90"></canvas>
<script type=module>
import * as shim from "./vieww-gpu-web.js";
const { instance } = await WebAssembly.instantiateStreaming(
    fetch("webgpu_scene.wasm"), { vieww_gpu_web: shim });
shim.attach(instance.exports);
const ex = instance.exports;
function failureText() {
  const len = ex.webgpu_failure(0, 0);
  if (len === 0) return "";
  const fresh = ex.memory.buffer.byteLength;
  ex.memory.grow(1);
  const ptr = fresh, cap = ex.memory.buffer.byteLength - fresh;
  ex.webgpu_failure(ptr, cap);
  const bytes = new Uint8Array(ex.memory.buffer, ptr, cap);
  const end = bytes.indexOf(0);
  return new TextDecoder().decode(bytes.subarray(0, end < 0 ? cap : end));
}
function finish(state) {
  const scenes = ex.webgpu_scenes();
  const verdicts = [];
  for (let i = 0; i < scenes; i++) {
    verdicts.push({
      status: ex.webgpu_scene_status(i),
      worst: ex.webgpu_scene_worst(i),
      interior: ex.webgpu_scene_interior(i),
      edge: ex.webgpu_scene_edge(i),
      exact: ex.webgpu_scene_exact(i),
    });
  }
  window.result = { state, scenes, verdicts,
                    presented: ex.webgpu_presented(),
                    failure: failureText() };
}
function tick() {
  const state = ex.webgpu_step();
  if (state === 0) requestAnimationFrame(tick);
  else finish(state);
}
requestAnimationFrame(tick);
</script>"""


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        routes = {"/": (PAGE, "text/html"),
                  "/vieww-gpu-web.js": (SHIM, "text/javascript"),
                  "/webgpu_scene.wasm": (WASM, "application/wasm")}
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
    if not os.path.exists(WASM):
        print(f"webgpu-parity: no wasm at {WASM} — build the example first:",
              file=sys.stderr)
        print("  cargo build --release --target wasm32-unknown-unknown "
              "-p vieww-hal --example webgpu_scene --features webgpu",
              file=sys.stderr)
        return 2
    try:
        from playwright.sync_api import sync_playwright
    except ImportError:
        print("webgpu-parity: playwright is not installed — a machine skip",
              file=sys.stderr)
        return 3

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = f"http://127.0.0.1:{server.server_address[1]}/"
    with sync_playwright() as pw:
        # --enable-unsafe-webgpu is what lets a headless Chromium offer its
        # SwiftShader fallback adapter at all; a real GPU does not need the
        # flag, and it changes nothing there.
        browser = getattr(pw, ENGINE).launch(
            args=["--enable-unsafe-webgpu"] if ENGINE == "chromium" else [])
        page = browser.new_page()
        errors = []
        page.on("pageerror", lambda e: errors.append(str(e)))
        page.on("console", lambda m: m.type == "error" and errors.append(m.text))
        page.goto(url)
        page.wait_for_function("() => window.result", timeout=60000)
        r = page.evaluate("() => window.result")
        browser.close()
    server.shutdown()

    failures = []
    if errors:
        failures.append("page errors: " + " | ".join(errors[:4]))
    if r["state"] == 3:
        print(f"webgpu-parity[{ENGINE}]: SKIP — {r['failure']}")
        print("webgpu-parity: the canvas-CPU backend is the answer on this "
              "engine; nothing was certified")
        if errors:
            print("webgpu-parity: (page errors preceded the skip, though)")
            return 1
        return 3
    if r["state"] == 2:
        failures.append(f"the run failed: {r['failure']}")
    elif r["state"] != 1:
        failures.append(f"the run never settled (state {r['state']})")

    for i, v in enumerate(r["verdicts"]):
        if v["status"] != 1:
            failures.append(
                f"scene {i}: {v['interior']} interior pixels over the 2/255 "
                f"tolerance (worst {v['worst']}/255, edge {v['edge']})")
    if r["verdicts"] and r["verdicts"][0]["exact"] != 1:
        failures.append("scene 0 (the empty scene) is not byte-exact — "
                        "nothing was drawn, so there is nothing to disagree "
                        "about either")
    if r["presented"] != 1:
        failures.append("the last frame never presented into the canvas")

    summary = ", ".join(
        f"s{i}={'pass' if v['status'] == 1 else 'FAIL'}"
        f"(worst {v['worst']}, edge {v['edge']}, "
        f"{'exact' if v['exact'] else 'diff'})"
        for i, v in enumerate(r["verdicts"])) or "no scenes"
    print(f"webgpu-parity[{ENGINE}]: {summary}; presented={r['presented']}")
    print("webgpu-parity[" + ENGINE + "]: verdicts " +
          json.dumps(r["verdicts"]))
    for f in failures:
        print(f"webgpu-parity[{ENGINE}]: FAIL {f}")
    if failures:
        return 1
    print(f"webgpu-parity[{ENGINE}]: WebGPU matches the CPU rasterizer "
          "under the performance contract (interiors <= 2/255, edge-only "
          "disagreements)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
