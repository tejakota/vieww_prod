//! The browser tool (WR-11): `render`, `screenshot-project`,
//! `assert-visual`.
//!
//! The one axis every rival competes on — *look at the thing you built*
//! — lands here, behind the same permission gate as everything else
//! (the `Network` capability, the Rule-of-Two legs, approval policies)
//! and refusal-first, the house pattern: a machine with no headless
//! browser gets a sentence that says what to install, never a silent
//! skip.
//!
//! Three levels, one tool:
//!
//! * **`render`** — screenshot a public URL with the machine's own
//!   headless Chromium/Chrome. The URL goes through the same SSRF check
//!   as `http-read` (private/loopback/metadata addresses refused unless
//!   the run was given that reach). The browser is an ordinary child
//!   process, like the `npm`/`cargo` an export runs: NOT inside the
//!   kernel sandbox (it could not write its profile or the PNG there),
//!   and therefore gated and approval-eligible like any world effect.
//!   Chrome's *own* renderer sandbox is left on; containers that cannot
//!   host it can opt in with `WRIGHT_BROWSER_NO_SANDBOX=1` (the honest
//!   trade is documented in that env var's error path).
//! * **`screenshot-project`** — serve the project's built web output
//!   (`dist/`, what `wright export --web` packages) on a loopback
//!   listener THIS tool owns, and capture PNGs of it. Loopback is
//!   private-network reach for the TOOL, not the agent: it must be
//!   opted in (`wright run --browser-loopback`, or
//!   `WRIGHT_BROWSER_LOOPBACK=1`), and it never relaxes the http tools'
//!   own guards. The listener serves one directory, bounded and
//!   traversal-checked, and stops when the screenshot is taken.
//! * **`assert-visual`** — compare a golden PNG against a captured one
//!   with the in-house decoder (`crate::png`): pixel-exact or within a
//!   per-channel tolerance, integer metrics, honest refusal when either
//!   file is not a PNG this decoder reads.
//!
//! What is deliberately NOT here: no browser automation (no clicks, no
//! typing — a screenshot cannot be steered into acting on a page), no
//! cache of rendered pages, and no network reach beyond the one URL.
//!
//! Honest limit: the full render loop needs a real browser, so it is
//! CI/self-hosted-only — the hosted suites set `WRIGHT_BROWSER_BIN` to
//! a chrome they install; this sandbox has none, and every level
//! refuses with install instructions instead.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use wright_core::wire::json::Value;
use wright_runtime::gate::Permit;

use crate::png;
use crate::{ToolCtx, ToolError, ToolOutput};

/// How long the headless browser may run before it is killed.
const BROWSER_TIMEOUT: Duration = Duration::from_secs(90);
/// Largest screenshot accepted into the project (a 4K RGBA PNG is ~30 MiB).
const MAX_PNG_BYTES: usize = 32 * 1024 * 1024;
/// Largest file the loopback server will serve (assets, not datasets).
const MAX_SERVED: usize = 32 * 1024 * 1024;
/// Largest request head the loopback server reads before giving up.
const MAX_HEAD: usize = 8 * 1024;

// ── the entry point ───────────────────────────────────────────────

/// Run one `browser` call. See the module doc for the three actions.
pub fn run(permit: &Permit, input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolError> {
    let action = input.str_of("action").ok_or_else(|| {
        ToolError::BadInput(
            "missing string field 'action' (render | screenshot-project | assert-visual)".into(),
        )
    })?;
    let _ = permit; // the gate checked the Network capability before this ran
    match action {
        "render" => render(input, ctx),
        "screenshot-project" => screenshot_project(input, ctx),
        "assert-visual" => assert_visual(input, ctx),
        other => Err(ToolError::BadInput(format!(
            "unknown browser action '{other}' (render | screenshot-project | assert-visual)"
        ))),
    }
}

// ── finding the browser ───────────────────────────────────────────

/// The headless browsers looked for, in order, and where.
pub const BROWSER_CANDIDATES: &[&str] = &[
    "chromium",
    "chromium-browser",
    "google-chrome",
    "google-chrome-stable",
    "chrome",
];

/// Where a Chrome lives outside PATH (macOS bundle, Windows Program Files).
const BROWSER_LOCATIONS: &[&str] = &[
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
    "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
];

/// The sentence every refusal-first path uses.
fn no_browser() -> ToolError {
    ToolError::Failed(format!(
        "no headless browser was found on this machine. Wright looks for {} (and the usual Chrome install locations), or the path in WRIGHT_BROWSER_BIN. Install one (e.g. `sudo apt install chromium`), then call again; `wright doctor` shows what it looks for.",
        BROWSER_CANDIDATES.join(", ")
    ))
}

/// The browser binary to drive: `WRIGHT_BROWSER_BIN` if set, else the
/// first candidate found on PATH or in the known locations. `None` is
/// the refusal (the tool never falls back to something else).
fn find_browser() -> Result<PathBuf, ToolError> {
    if let Ok(set) = std::env::var("WRIGHT_BROWSER_BIN") {
        let p = PathBuf::from(set);
        if p.is_file() {
            return Ok(p);
        }
        return Err(ToolError::Failed(format!(
            "WRIGHT_BROWSER_BIN points at {}, which is not a file; fix it or unset it so the PATH candidates are used",
            p.display()
        )));
    }
    for name in BROWSER_CANDIDATES {
        if let Some(p) = which(name) {
            return Ok(p);
        }
    }
    for loc in BROWSER_LOCATIONS {
        let p = PathBuf::from(loc);
        if p.is_file() {
            return Ok(p);
        }
    }
    Err(no_browser())
}

/// A PATH lookup (std has none; the mcp/worker code has its own).
fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if let Ok(meta) = std::fs::metadata(&candidate) {
            if meta.is_file() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if meta.permissions().mode() & 0o111 == 0 {
                        continue; // not executable
                    }
                }
                return Some(candidate);
            }
        }
    }
    None
}

/// What `wright doctor` prints (WR-11): the browser it would drive, or
/// the same candidates the refusal names. Informational, not a failure —
/// `assert-visual` works without one, only the render loop needs a
/// browser.
pub fn doctor_line() -> String {
    match find_browser() {
        Ok(bin) => format!(
            "{} — render / screenshot-project ready (assert-visual needs no browser)",
            bin.display()
        ),
        Err(ToolError::Failed(msg)) => {
            // The refusal is a whole sentence; doctor keeps the useful
            // head of it and stays one line.
            format!("none found — {msg}")
        }
        Err(_) => format!(
            "none found — looks for {} or WRIGHT_BROWSER_BIN; assert-visual needs no browser",
            BROWSER_CANDIDATES.join(", ")
        ),
    }
}

// ── shared plumbing ───────────────────────────────────────────────

/// A safe screenshot name: `[A-Za-z0-9._-]`, no leading dot, bounded.
fn safe_name(input: &Value, default: &str) -> Result<String, ToolError> {
    let raw = input.str_of("name").unwrap_or(default);
    let clean: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .collect();
    if clean.is_empty() || clean.starts_with('.') || clean.len() > 64 {
        return Err(ToolError::BadInput(format!(
            "'{raw}' is not a usable screenshot name (letters, digits, . _ -, no leading dot, 64 chars max)"
        )));
    }
    Ok(clean)
}

/// The window size asked for, clamped to sane screenshot bounds.
fn window(input: &Value) -> (u32, u32) {
    let get = |key: &str, default: u32| {
        input
            .get(key)
            .and_then(|v| v.as_u64())
            .map(|v| v.clamp(200, 3840) as u32)
            .unwrap_or(default)
    };
    (get("width", 1280), get("height", 800))
}

/// A private (0700) scratch directory for the browser profile and the
/// raw screenshot, mirroring the worker's throwaway copy.
fn scratch(tag: &str) -> Result<PathBuf, ToolError> {
    let d = std::env::temp_dir().join(format!(
        "wright-browser-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).map_err(|e| ToolError::Failed(e.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700));
    }
    Ok(d)
}

/// Drive the headless browser to screenshot `url` into `out_png`.
fn screenshot_url(
    bin: &Path,
    url: &str,
    (width, height): (u32, u32),
    out_png: &Path,
    profile: &Path,
) -> Result<(), ToolError> {
    let mut cmd = Command::new(bin);
    cmd.args([
        "--headless",
        "--disable-gpu",
        "--hide-scrollbars",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-background-networking",
        "--mute-audio",
    ]);
    if std::env::var("WRIGHT_BROWSER_NO_SANDBOX")
        .map(|v| v == "1")
        .unwrap_or(false)
    {
        // The container escape hatch: Chrome's OWN renderer sandbox uses
        // namespaces a container may not grant. This trades a layer of
        // Chrome's isolation away; wright's own boundaries (the gate, the
        // project-only writes, the decoded-and-copied PNG) are unchanged.
        cmd.arg("--no-sandbox");
    }
    cmd.args([
        format!("--user-data-dir={}", profile.display()),
        format!("--window-size={width},{height}"),
        // Deterministic-ish rendering: fast-forward timers, then shoot.
        "--virtual-time-budget=10000".to_string(),
        format!("--screenshot={}", out_png.display()),
    ]);
    cmd.arg(url);
    // Chrome's output goes to a file, never a pipe. A pipe has two ways to
    // hang this call: Chrome blocks once it fills (nothing reads it while
    // we wait), and reading it to the end waits for every process holding
    // it — and Chrome leaves helpers (crashpad, the zygote) holding its
    // stderr after the main process exits. On a CI runner where Chrome's
    // own sandbox cannot start, that read never returned.
    let log_path = profile.with_extension("stderr.log");
    let log = std::fs::File::create(&log_path)
        .map_err(|e| ToolError::Failed(format!("could not create the browser log: {e}")))?;
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(log);
    // Its own process group, so the clock (and the cleanup after a normal
    // exit) takes the helpers with it instead of leaving them running.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    // The browser never needs wright's own environment.
    for (k, _) in std::env::vars() {
        if k.starts_with("WRIGHT_") {
            cmd.env_remove(&k);
        }
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| ToolError::Failed(format!("could not start {}: {e}", bin.display())))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if started.elapsed() > BROWSER_TIMEOUT {
                    crate::worker::kill_group(&mut child);
                    let _ = child.wait();
                    return Err(ToolError::Failed(format!(
                        "the headless browser did not finish within {} seconds and was killed",
                        BROWSER_TIMEOUT.as_secs()
                    )));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(ToolError::Failed(e.to_string())),
        }
    };
    // The main process is done; whatever it left behind goes too.
    crate::worker::kill_group(&mut child);
    if !status.success() {
        let buf = std::fs::read(&log_path).unwrap_or_default();
        let tail = &buf[buf.len().saturating_sub(4096)..];
        let err = String::from_utf8_lossy(tail).to_string();
        return Err(ToolError::Failed(format!(
            "the headless browser exited with {}: {}",
            status.code().unwrap_or(-1),
            err.lines().last().unwrap_or("(no output)")
        )));
    }
    Ok(())
}

/// Decode the browser's PNG (validating it is one we can compare),
/// then write it into the project as an artifact. Returns (rel path,
/// width, height, bytes).
fn take_screenshot(
    ctx: &ToolCtx,
    bin: &Path,
    url: &str,
    size: (u32, u32),
    name: &str,
    tag: &str,
) -> Result<(String, u32, u32, usize), ToolError> {
    let dir = scratch(tag)?;
    let raw = dir.join("shot.png");
    screenshot_url(bin, url, size, &raw, &dir.join("profile"))?;
    let bytes = std::fs::read(&raw).map_err(|e| {
        ToolError::Failed(format!(
            "the headless browser ran but wrote no screenshot: {e}"
        ))
    })?;
    if bytes.len() > MAX_PNG_BYTES {
        return Err(ToolError::Failed(format!(
            "the screenshot is {} bytes; the limit is {MAX_PNG_BYTES}",
            bytes.len()
        )));
    }
    let img = png::decode(&bytes).map_err(ToolError::Failed)?;
    let rel = format!("artifacts/screenshots/{name}.png");
    let full = crate::paths::resolve(&ctx.project, &rel).map_err(ToolError::Failed)?;
    crate::paths::write_pinned(&ctx.project, &full, &bytes).map_err(ToolError::Failed)?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok((rel.clone(), img.width, img.height, bytes.len()))
}

// ── render ────────────────────────────────────────────────────────

fn render(input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolError> {
    let url = wright_core::wire::http::parse_url(
        input
            .str_of("url")
            .ok_or_else(|| ToolError::BadInput("missing string field 'url'".into()))?,
    )
    .map_err(ToolError::BadInput)?;
    // The same SSRF rule as http-read: public addresses only, unless the
    // run was given private reach (WR-07's pinned answer is not reused
    // because the browser resolves on its own — which is exactly why the
    // check is all-or-nothing here, never a pin).
    crate::check_public(&url.host, url.port, ctx)?;
    let bin = find_browser()?;
    let name = safe_name(input, "shot")?;
    let size = window(input);
    let (rel, w, h, n) = take_screenshot(ctx, &bin, &url.to_string(), size, &name, "render")?;
    Ok(ToolOutput {
        text: format!(
            "rendered {} as {} ({}x{} PNG, {} bytes). The page's pixels, not its claims: compare expectations against artifacts/screenshots with browser assert-visual.",
            url, rel, w, h, n
        ),
        wrote: vec![rel],
    })
}

// ── screenshot-project ────────────────────────────────────────────

fn screenshot_project(input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolError> {
    if !ctx.browser_loopback {
        return Err(ToolError::Failed(
            "screenshot-project serves the project's built site on a loopback listener, which is private-network reach for THIS tool only — and it is off by default. Opt in with `wright run --browser-loopback` (or WRIGHT_BROWSER_LOOPBACK=1); your http tools' own guards are unchanged.".into(),
        ));
    }
    // The served root: dist/ (what `wright export --web` packages).
    let rel_root = input.str_of("path").unwrap_or("dist");
    let root = crate::paths::resolve(&ctx.project, rel_root).map_err(ToolError::Failed)?;
    if !root.is_dir() {
        return Err(ToolError::Failed(format!(
            "'{rel_root}' is not a directory — build the site first (`wright export --web`), then screenshot it"
        )));
    }
    let page = page_arg(input)?;
    let index = root.join(&page);
    if !index.is_file() {
        return Err(ToolError::Failed(format!(
            "'{rel_root}/{}' does not exist; list the built files with files-list and screenshot a page that is there",
            page.replace('\\', "/")
        )));
    }
    let bin = find_browser()?;
    let name = safe_name(input, "site")?;
    let size = window(input);
    let server = serve_dir(&root)?;
    let url = format!("http://127.0.0.1:{}/{page}", server.port);
    let shot = take_screenshot(ctx, &bin, &url, size, &name, "project");
    drop(server); // stops the listener; the thread exits on its poll
    let (rel, w, h, n) = shot?;
    Ok(ToolOutput {
        text: format!(
            "screenshot {} of the built site ({} served from {} on loopback; {}x{} PNG, {} bytes). The render loop is only as portable as the browser: CI and the self-hosted boxes run it, a bare sandbox refuses.",
            rel, page.replace('\\', "/"), rel_root, w, h, n
        ),
        wrote: vec![rel],
    })
}

/// The page to capture, from `page` (default `index.html`), verified to
/// be a plain relative path inside the served root.
fn page_arg(input: &Value) -> Result<String, ToolError> {
    let raw = input.str_of("page").unwrap_or("index.html");
    let page = raw.replace('\\', "/");
    if page.is_empty()
        || !page
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '-' | '_'))
        || page
            .split('/')
            .any(|part| part.is_empty() || part == ".." || part.starts_with('.'))
        || !page.ends_with(".html")
    {
        return Err(ToolError::BadInput(format!(
            "'{raw}' is not a page in the built site (a relative path ending in .html, no ..)"
        )));
    }
    Ok(page)
}

// ── assert-visual ─────────────────────────────────────────────────

fn assert_visual(input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolError> {
    let golden_rel = input.str_of("golden").ok_or_else(|| {
        ToolError::BadInput("missing string field 'golden' (the reference PNG)".into())
    })?;
    let actual_rel = input.str_of("actual").ok_or_else(|| {
        ToolError::BadInput("missing string field 'actual' (the captured PNG)".into())
    })?;
    let tolerance = input
        .get("tolerance")
        .and_then(|t| t.as_u64())
        .unwrap_or(0)
        .min(255) as u8;
    let read = |rel: &str| -> Result<(String, png::Image), ToolError> {
        let full = crate::paths::resolve(&ctx.project, rel).map_err(ToolError::Failed)?;
        let bytes =
            std::fs::read(&full).map_err(|e| ToolError::Failed(format!("read {rel}: {e}")))?;
        let img = png::decode(&bytes).map_err(|e| ToolError::Failed(format!("{rel}: {e}")))?;
        Ok((rel.to_string(), img))
    };
    let (golden_name, golden) = read(golden_rel)?;
    let (actual_name, actual) = read(actual_rel)?;
    let d = match png::diff(&golden, &actual, tolerance) {
        Ok(d) => d,
        Err(e) => {
            return Err(ToolError::Failed(format!(
                "{golden_name} vs {actual_name}: {e}"
            )))
        }
    };
    let verdict = if d.ok() {
        if d.max_delta == 0 {
            "identical".to_string()
        } else {
            format!(
                "matches within tolerance {} (worst channel delta {}, total absolute error {})",
                tolerance, d.max_delta, d.total_delta
            )
        }
    } else {
        format!(
            "DIFFERS: {} of {} pixels are beyond tolerance {} (worst channel delta {}, total absolute error {})",
            d.differing, d.total, tolerance, d.max_delta, d.total_delta
        )
    };
    let text = format!("{golden_name} vs {actual_name}: {verdict}");
    if d.ok() {
        Ok(ToolOutput {
            text,
            wrote: Vec::new(),
        })
    } else {
        Err(ToolError::Failed(text))
    }
}

// ── the loopback server ───────────────────────────────────────────
//
// A static file server for ONE directory, std-only: one listener, one
// request per connection (Connection: close), bounded heads and bodies,
// and path checks before any file is opened. It exists for the seconds
// a screenshot takes and no longer — `Drop` flips the stop flag and the
// accept loop exits.

/// A running loopback static server for one directory.
struct Loopback {
    port: u16,
    stop: Arc<AtomicBool>,
}

impl Drop for Loopback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Serve `root` on 127.0.0.1 (ephemeral port) until the handle drops.
fn serve_dir(root: &Path) -> Result<Loopback, ToolError> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| ToolError::Failed(format!("cannot bind a loopback listener: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| ToolError::Failed(e.to_string()))?
        .port();
    listener
        .set_nonblocking(true)
        .map_err(|e| ToolError::Failed(e.to_string()))?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let root = root.to_path_buf();
    std::thread::Builder::new()
        .name("wright-browser-loopback".into())
        .spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let root = root.clone();
                        std::thread::spawn(move || {
                            let _ = serve_one(stream, &root);
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => return,
                }
            }
        })
        .map_err(|e| ToolError::Failed(e.to_string()))?;
    Ok(Loopback { port, stop })
}

/// One connection: read the head, answer with the file (or an error).
fn serve_one(stream: TcpStream, root: &Path) -> std::io::Result<()> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let mut stream = stream;
    let head = read_head(&mut stream)?;
    let mut lines = head.lines();
    let request = lines.next().unwrap_or("").to_string();
    let mut parts = request.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    if method != "GET" && method != "HEAD" {
        return respond(&mut stream, 405, "text/plain", b"method not allowed");
    }
    let path = match serve_path(root, target) {
        Ok(p) => p,
        Err(code_msg) => {
            let (code, msg) = code_msg;
            let body = msg.as_bytes();
            return respond(&mut stream, code, "text/plain", body);
        }
    };
    let meta = match std::fs::metadata(&path) {
        Ok(m) if m.is_file() => m,
        _ => return respond(&mut stream, 404, "text/plain", b"not found"),
    };
    if meta.len() as usize > MAX_SERVED {
        return respond(&mut stream, 413, "text/plain", b"file too large to serve");
    }
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => return respond(&mut stream, 404, "text/plain", b"not found"),
    };
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(MAX_SERVED as u64 + 1).read_to_end(&mut bytes)?;
    let ctype = content_type(&path);
    let body = if method == "HEAD" {
        &[][..]
    } else {
        &bytes[..]
    };
    respond(&mut stream, 200, ctype, body)
}

/// Read the request head, bounded; the body is never read (GET only).
fn read_head(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                head.push(byte[0]);
                if head.ends_with(b"\r\n\r\n") || head.ends_with(b"\n\n") {
                    break;
                }
                if head.len() > MAX_HEAD {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "request head too large",
                    ));
                }
            }
            Err(e) => return Err(e),
        }
    }
    Ok(String::from_utf8_lossy(&head).into_owned())
}

/// Resolve a request target to a file under `root`, or an HTTP error.
fn serve_path(root: &Path, target: &str) -> Result<PathBuf, (u16, String)> {
    let raw = target.split(['?', '#']).next().unwrap_or("");
    if !raw.starts_with('/') || raw.contains('\0') {
        return Err((400, "bad request".into()));
    }
    let decoded = percent_decode(raw)?;
    let decoded = decoded.trim_start_matches('/');
    if decoded.contains('\\') {
        return Err((400, "bad request".into()));
    }
    let mut path = root.to_path_buf();
    for part in decoded.split('/') {
        match part {
            "" | "." => continue,
            ".." => {
                return Err((403, "path traversal is not served".into()));
            }
            p if p.len() > 128 => return Err((400, "bad request".into())),
            p => {
                // A component that could be a symlink is refused: the
                // server serves one directory, not what it points at.
                let next = path.join(p);
                match std::fs::symlink_metadata(&next) {
                    Ok(m) if m.file_type().is_symlink() => {
                        return Err((403, "symbolic links are not served".into()))
                    }
                    _ => {}
                }
                path = next;
            }
        }
    }
    if path == root {
        path = root.join("index.html");
    }
    Ok(path)
}

/// Percent-decode a request path, refusing encoded traversals and NUL.
fn percent_decode(raw: &str) -> Result<String, (u16, String)> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 3 > bytes.len() {
                return Err((400, "bad percent-encoding".into()));
            }
            let pair = [bytes[i + 1], bytes[i + 2]];
            let hex = std::str::from_utf8(&pair)
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .ok_or((400u16, "bad percent-encoding".to_string()))?;
            if hex == 0 {
                return Err((400, "encoded NUL is not served".into()));
            }
            out.push(hex);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let text = String::from_utf8(out).map_err(|_| (400u16, "bad encoding".to_string()))?;
    if text.contains("..") {
        return Err((403, "path traversal is not served".into()));
    }
    Ok(text)
}

fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        "map" => "application/json",
        _ => "application/octet-stream",
    }
}

fn respond(stream: &mut TcpStream, code: u16, ctype: &str, body: &[u8]) -> std::io::Result<()> {
    let reason = match code {
        200 => "OK",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        _ => "Bad Request",
    };
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        code,
        reason,
        ctype,
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()
}

// ── tests ─────────────────────────────────────────────────────────

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use wright_core::wire::json;

    /// Env vars are process-wide and tests run in parallel: every test
    /// that flips WRIGHT_BROWSER_BIN takes this lock (the repo's
    /// signing.rs pattern).
    /// Serialises tests that set WRIGHT_BROWSER_* (the env is process-wide;
    /// `tests::browser_refuses_loudly` in lib.rs takes it too).
    pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn ctx(name: &str) -> ToolCtx {
        let d = std::env::temp_dir().join(format!("wright-browser-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        ToolCtx::new(&d).unwrap()
    }

    fn permit() -> wright_runtime::gate::Permit {
        wright_runtime::gate::Permit::for_tests(
            "Agent",
            wright_core::palette::find("browser").unwrap(),
            wright_core::blueprint::PermissionSet(0b11111),
        )
    }

    #[test]
    fn the_dispatch_names_its_three_actions() {
        let c = ctx("dispatch");
        let err = run(
            &permit(),
            &json::parse(r#"{"action":"click"}"#).unwrap(),
            &c,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("unknown browser action"), "{err}");
        let err = run(
            &permit(),
            &json::parse(r#"{"url":"http://x/"}"#).unwrap(),
            &c,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("missing string field 'action'"), "{err}");
    }

    #[test]
    fn render_refuses_when_no_browser_is_found() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let c = ctx("nobrowser");
        // Deterministic refusal: point the env override at nothing (a
        // dev machine with chromium installed must still refuse here).
        std::env::set_var("WRIGHT_BROWSER_BIN", "/no/such/browser/chrome");
        let err = render(
            &json::parse(r#"{"url":"http://93.184.216.34/"}"#).unwrap(),
            &c,
        )
        .unwrap_err()
        .to_string();
        std::env::remove_var("WRIGHT_BROWSER_BIN");
        assert!(err.contains("WRIGHT_BROWSER_BIN"), "{err}");
    }

    /// A browser that fails while a helper it started keeps running must
    /// not hang the call. The old code piped stderr and read it to the end
    /// after a failed exit — which waits for every holder of the pipe, so a
    /// lingering Chrome helper (crashpad, the zygote) blocked it forever on
    /// CI. The stand-in fails at once and leaves a 30s sleeper behind.
    #[cfg(unix)]
    #[test]
    fn a_failing_browser_with_a_lingering_helper_does_not_hang() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("lingering").unwrap();
        let fake = dir.join("fake-chrome");
        std::fs::write(
            &fake,
            "#!/bin/sh\nsleep 30 &\necho 'No usable sandbox!' >&2\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let started = Instant::now();
        let err = screenshot_url(
            &fake,
            "http://127.0.0.1:9/",
            (400, 300),
            &dir.join("shot.png"),
            &dir.join("profile"),
        )
        .unwrap_err()
        .to_string();
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "returned only after {:?}",
            started.elapsed()
        );
        assert!(err.contains("exited with 1"), "{err}");
        assert!(err.contains("No usable sandbox!"), "its last words: {err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn private_urls_are_refused_like_http_read() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let c = ctx("ssrf");
        std::env::set_var("WRIGHT_BROWSER_BIN", "/no/such/browser/chrome");
        let err = render(
            &json::parse(r#"{"url":"http://169.254.169.254/latest"}"#).unwrap(),
            &c,
        )
        .unwrap_err()
        .to_string();
        std::env::remove_var("WRIGHT_BROWSER_BIN");
        assert!(err.contains("private or local"), "{err}");
    }

    #[test]
    fn screenshot_project_refuses_without_the_loopback_opt_in() {
        let mut c = ctx("loopback-off");
        c.browser_loopback = false;
        let err = screenshot_project(&json::parse("{}").unwrap(), &c)
            .unwrap_err()
            .to_string();
        assert!(err.contains("browser-loopback"), "{err}");
        assert!(err.contains("THIS tool only"), "{err}");
    }

    #[test]
    fn screenshot_project_refuses_without_a_built_site() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let mut c = ctx("nodist");
        c.browser_loopback = true;
        std::env::set_var("WRIGHT_BROWSER_BIN", "/no/such/browser/chrome");
        let err = screenshot_project(&json::parse("{}").unwrap(), &c)
            .unwrap_err()
            .to_string();
        std::env::remove_var("WRIGHT_BROWSER_BIN");
        assert!(err.contains("not a directory"), "{err}");
    }

    #[test]
    fn names_and_pages_are_checked() {
        assert!(safe_name(&json::parse(r#"{"name":"ok-name_1"}"#).unwrap(), "shot").is_ok());
        assert!(safe_name(&json::parse(r#"{"name":"../evil"}"#).unwrap(), "shot").is_err());
        assert!(safe_name(&json::parse(r#"{"name":"."}"#).unwrap(), "shot").is_err());
        assert_eq!(
            page_arg(&json::parse(r#"{"page":"about.html"}"#).unwrap()).unwrap(),
            "about.html"
        );
        assert!(page_arg(&json::parse(r#"{"page":"../etc/passwd"}"#).unwrap()).is_err());
        assert!(page_arg(&json::parse(r#"{"page":"a.js"}"#).unwrap()).is_err());
        assert_eq!(window(&json::parse("{}").unwrap()), (1280, 800));
        assert_eq!(
            window(&json::parse(r#"{"width":99999,"height":10}"#).unwrap()),
            (3840, 200),
            "clamped to sane screenshot bounds"
        );
    }

    #[test]
    fn the_loopback_server_serves_a_built_site() {
        let c = ctx("serve");
        let root = c.project.join("dist");
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("index.html"), b"<html>hello</html>").unwrap();
        std::fs::write(root.join("assets/app.js"), b"console.log(1)").unwrap();
        std::fs::write(root.join("secret.txt"), b"no").unwrap();
        let server = serve_dir(&root).unwrap();
        let port = server.port;
        let get = |path: &str| -> (u16, String) {
            use std::io::Read;
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(
                s,
                "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            let mut buf = Vec::new();
            s.read_to_end(&mut buf).unwrap();
            let text = String::from_utf8_lossy(&buf).into_owned();
            let code: u16 = text
                .split_whitespace()
                .nth(1)
                .and_then(|c| c.parse().ok())
                .unwrap_or(0);
            (code, text)
        };
        let (code, body) = get("/");
        assert_eq!(code, 200);
        assert!(body.contains("hello"));
        assert!(body.contains("text/html"));
        let (code, body) = get("/assets/app.js");
        assert_eq!(code, 200);
        assert!(body.contains("console.log"));
        let (code, _) = get("/missing.js");
        assert_eq!(code, 404);
        let (code, _) = get("/../secret.txt");
        assert_eq!(code, 403, "traversal is refused");
        let (code, _) = get("/%2e%2e/secret.txt");
        assert_eq!(code, 403, "encoded traversal is refused");
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(s, "POST / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
        let mut buf = Vec::new();
        use std::io::Read;
        s.read_to_end(&mut buf).unwrap();
        assert!(
            String::from_utf8_lossy(&buf).contains("405"),
            "only GET/HEAD"
        );
        drop(server);
        // The listener stops when the handle drops (within its poll).
        std::thread::sleep(Duration::from_millis(80));
        assert!(
            TcpStream::connect(("127.0.0.1", port)).is_err(),
            "the loopback listener is gone"
        );
    }

    #[test]
    fn assert_visual_compares_goldens() {
        let c = ctx("golden");
        // Two tiny PNGs via the stored-block encoder (filters all zero):
        // the golden and a capture that differs by one channel.
        let mk = |p: &Path, px: &[u8]| {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            let mut raw = vec![0u8];
            raw.extend_from_slice(px);
            let mut z = vec![0x78, 0x01, 0x01];
            z.extend_from_slice(&(raw.len() as u16).to_le_bytes());
            z.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
            z.extend_from_slice(&raw);
            z.extend_from_slice(&png::adler32(&raw).to_be_bytes());
            let mut file = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
            let chunk = |f: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]| {
                f.extend_from_slice(&(data.len() as u32).to_be_bytes());
                let mut ci = Vec::new();
                ci.extend_from_slice(kind);
                ci.extend_from_slice(data);
                f.extend_from_slice(kind);
                f.extend_from_slice(data);
                f.extend_from_slice(&png::crc32(&ci).to_be_bytes());
            };
            chunk(&mut file, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]);
            chunk(&mut file, b"IDAT", &z);
            chunk(&mut file, b"IEND", &[]);
            std::fs::write(p, file).unwrap();
        };
        mk(&c.project.join("goldens/home.png"), &[10, 20, 30]);
        mk(
            &c.project.join("artifacts/screenshots/home.png"),
            &[10, 23, 30],
        );
        let ok = assert_visual(
            &json::parse(
                r#"{"golden":"goldens/home.png","actual":"artifacts/screenshots/home.png","tolerance":3}"#,
            )
            .unwrap(),
            &c,
        )
        .unwrap();
        assert!(ok.text.contains("within tolerance"), "{}", ok.text);
        let exact = assert_visual(
            &json::parse(
                r#"{"golden":"goldens/home.png","actual":"artifacts/screenshots/home.png"}"#,
            )
            .unwrap(),
            &c,
        )
        .unwrap_err()
        .to_string();
        assert!(exact.contains("DIFFERS"), "{exact}");
        assert!(exact.contains("1 of 1"), "{exact}");
        // A missing file is a plain sentence.
        let err = assert_visual(
            &json::parse(
                r#"{"golden":"goldens/nope.png","actual":"artifacts/screenshots/home.png"}"#,
            )
            .unwrap(),
            &c,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("read goldens/nope.png"), "{err}");
    }
}
