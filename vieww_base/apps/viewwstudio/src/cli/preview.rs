//! `preview` — the interactive preview, served for a browser.
//!
//! # What this command is
//!
//! The preview pane without the pane: the same out-of-process child
//! (`viewwstudio-preview`) the studio's isolated preview loads a compiled
//! guest into, driven over HTTP instead of a canvas. The reasons the studio
//! goes out of process are the reasons this server does — a guest abort or
//! segfault must cost a *frame*, not the process — so [`RemotePreview`] is
//! the engine here, and a crash is answered by respawning the child and
//! reloading the same library rather than by dying.
//!
//! What flows:
//!
//! * **In**: `POST /event` — pointer contact, hover, scroll and keys, as
//!   small JSON objects in the device's own logical points. They are
//!   translated into [`RemoteEvent`]s, which are the exact events the
//!   window path feeds its own driver, and queued until the next frame.
//! * **Out**: `GET /frame` — the clock advanced by the wall time since the
//!   last frame (capped, so a background tab does not fast-forward the
//!   guest by an hour), the queued events delivered, the frame rasterised
//!   at the device's size × `--scale`, PNG-encoded by the workspace's own
//!   codec, and served with an `X-Wants-Frame` header so the page knows
//!   whether to keep polling at 30 fps or drop to an idle rate.
//!
//! The server is hand-rolled HTTP/1.1 over `std::net` — the same policy
//! every network-flavoured thing in this workspace follows (`say_ai`'s
//! model client, `vieww-network`'s test server): no third-party dependency
//! for a localhost loop that has three routes.
//!
//! # The honest limits
//!
//! * The page polls; it is not a websocket. At `--scale 1` and 30 fps on a
//!   laptop this is comfortably real-time; the knob exists if it is not.
//! * A crash recovers the *screen*, not the *state* — the respawned child
//!   starts the app over, because state capture is the studio pane's
//!   session feature, not a server's.
//! * One screen per server. `--screen` picks which; a second screen wants a
//!   second `preview` on another port.

use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use vieww_foundation::{json::Json, Accessibility, Color, KeyEvent, NamedKey, Offset, ScrollEvent};
use vieww_image::codec::png;

use crate::compile::Toolchain;
use crate::remote::{self, Environment, Pointer, RemoteError, RemoteEvent, RemotePreview};
use crate::state::{Device, Platform};

use super::{compile_mounted, resolve_input, theme_data, MountFailure};

/// The port the server listens on when nobody chose one — high enough to be
/// out of the dev-server ranges everything else grabs (3000, 5000, 8080),
/// memorable enough to type: `8788`.
pub(crate) const DEFAULT_PORT: u16 = 8788;

/// How much wall time one frame request may advance the guest clock by. A
/// tab that slept for an hour and woke up must not hand the guest an hour
/// of elapsed time — transitions would be over, springs would be at rest,
/// and nobody would have seen any of it.
const CLOCK_CAP: Duration = Duration::from_millis(250);

/// Everything one served preview carries: the child, the artefacts it needs
/// to come back after a crash, the device it is framed as, and the input
/// queued between frames.
struct Served {
    remote: RemotePreview,
    /// The preview child's own path, so a respawn does not have to re-find it.
    binary: PathBuf,
    /// The compiled library, kept so a respawn reloads the same screen.
    library: PathBuf,
    /// The toolchain's `lib` directory, for the child's `prime_libstd`.
    sysroot: Option<PathBuf>,
    env: Environment,
    scale: f32,
    base: Color,
    /// Input that arrived since the last frame, in arrival order.
    events: RefCell<Vec<RemoteEvent>>,
    /// The guest's clock — advanced by wall time, capped, never reset.
    now: RefCell<Duration>,
    /// When the last frame was served, the unit the clock advances by.
    last: RefCell<Instant>,
    /// How many frames the child has drawn, for the log line.
    frames: RefCell<u64>,
}

impl Served {
    /// Draw the next frame: advance the clock, deliver the queued input,
    /// rasterise. A crashed child is respawned once with the same library —
    /// the whole reason the preview lives in a child process.
    fn frame(&mut self) -> Result<(Vec<u8>, u32, u32, bool), String> {
        if self.remote.crashed().is_some() {
            eprintln!("preview: the guest died — respawning the child");
            let mut fresh = RemotePreview::spawn(&self.binary()?)
                .map_err(|error| format!("cannot restart the preview child: {error}"))?;
            fresh
                .load(&self.library, self.sysroot.as_deref())
                .map_err(|error| {
                    format!(
                        "cannot reload {} after a crash: {error}",
                        self.library.display()
                    )
                })?;
            self.remote = fresh;
            *self.now.borrow_mut() = Duration::ZERO;
            *self.last.borrow_mut() = Instant::now();
        }
        let elapsed = self.last.borrow().elapsed();
        *self.last.borrow_mut() = Instant::now();
        *self.now.borrow_mut() += elapsed.min(CLOCK_CAP);
        let at = *self.now.borrow();
        let events = std::mem::take(&mut *self.events.borrow_mut());
        let frame = match self
            .remote
            .frame(&self.env, self.scale, at, self.base, &events)
        {
            Ok(frame) => frame,
            Err(RemoteError::Crashed { status, stderr }) => {
                return Err(format!(
                    "the guest crashed drawing a frame ({status}):\n{}",
                    stderr.trim()
                ));
            }
            Err(error) => return Err(error.to_string()),
        };
        *self.frames.borrow_mut() += 1;
        let wants = frame.wants_frame;
        let bytes = png::encode(frame.width, frame.height, &frame.rgba);
        Ok((bytes, frame.width, frame.height, wants))
    }

    /// The preview child's path — stored at construction, because a
    /// respawn must not depend on `current_exe` finding the same answer
    /// it gave before.
    fn binary(&self) -> Result<PathBuf, String> {
        Ok(self.binary.clone())
    }
}

/// `preview`: compile the screen, serve it, and keep serving until killed.
///
/// # Errors
///
/// The compile (as every render path), the missing preview child, or the
/// port being taken — each named, each with its remedy.
#[expect(
    clippy::too_many_arguments,
    reason = "one parameter per command-line flag, the shape `run_animate` \
             already has; a struct would only rename them"
)]
pub fn run_preview(
    input: Option<&Path>,
    device: Device,
    dark: bool,
    screen_name: Option<&str>,
    port: u16,
    scale: f32,
    open: bool,
    share: bool,
) -> Result<String, String> {
    let input = match input {
        Some(path) => path.to_path_buf(),
        None => {
            let cwd = std::env::current_dir()
                .map_err(|error| format!("cannot tell where you are standing — {error}"))?;
            cwd
        }
    };
    let screen = resolve_input(&input)?;

    // **The child binary and the toolchain's libstd, before any compile.**
    // `sibling_binary` refuses rather than guessing, because a preview that
    // spawns the wrong process is a protocol error dressed as a hang.
    let binary = remote::RemotePreview::sibling_binary().ok_or_else(|| {
        format!(
            "the preview child ({}) is not next to this binary — build the workspace \
             (cargo build -p viewwstudio --bins), the same build that makes render work",
            remote::CHILD_BINARY
        )
    })?;
    // The same `prime_libstd` the pane does before a load: a process started
    // outside cargo has nowhere to find the toolchain's libstd otherwise.
    let toolchain = Toolchain::discover(&crate::install::target_dir()).ok();
    if let Some(toolchain) = &toolchain {
        toolchain.prime_libstd();
    }
    let sysroot = toolchain.as_ref().and_then(|t| t.sysroot_lib.clone());

    // The compile, mounted in the child — the one pipeline, this loader.
    // A fresh child per candidate: the walk's business is which rlib the
    // *guest* was compiled against, and the child is the guest. The loader
    // also reports back the library it mounted, because a respawn later
    // reloads exactly that.
    let mounted_library = RefCell::new(PathBuf::new());
    let loader = |library: &Path| -> Result<RemotePreview, MountFailure> {
        let mut child = RemotePreview::spawn(&binary).map_err(|error| {
            MountFailure::Fatal(format!("cannot start the preview child: {error}"))
        })?;
        child
            .load(library, sysroot.as_deref())
            .map_err(|error| match error {
                RemoteError::AbiMismatch(detail) => MountFailure::AbiMismatch(detail),
                other => MountFailure::Fatal(other.to_string()),
            })?;
        *mounted_library.borrow_mut() = library.to_path_buf();
        Ok(child)
    };
    let (child, rustc) = compile_mounted(&screen, screen_name, &loader)?;
    let library = mounted_library.into_inner();

    let theme = theme_data(dark);
    let env = Environment {
        platform: device.platform,
        dark,
        metrics: device.view_metrics(),
        accessibility: Accessibility::default(),
    };
    let mut served = Served {
        remote: child,
        binary,
        library,
        sysroot,
        env,
        scale,
        base: theme.colors.surface,
        events: RefCell::new(Vec::new()),
        now: RefCell::new(Duration::ZERO),
        last: RefCell::new(Instant::now()),
        frames: RefCell::new(0),
    };

    // Local only unless sharing was asked for: a preview on every interface
    // is a preview anyone on the café's network can tap through, and that
    // is a choice for the person at the keyboard to make, by name.
    let host = if share { "0.0.0.0" } else { "127.0.0.1" };
    let listener = TcpListener::bind((host, port)).map_err(|error| {
        format!("cannot listen on port {port} — is another preview there? {error}")
    })?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("bound, but cannot say where — {error}"))?;
    let url = format!("http://127.0.0.1:{}", address.port());
    let shared = if share {
        let lan = crate::showcase::lan_address().ok_or_else(|| {
            "--share serves to the local network, and this machine is not on one — \
             connect to Wi-Fi (the phone must be on the same network)"
                .to_owned()
        })?;
        let shared = format!("http://{lan}:{}", address.port());
        let code = crate::qr::QrCode::encode(shared.as_bytes())?;
        Some((shared, code))
    } else {
        None
    };

    let title = screen
        .path
        .file_stem()
        .map_or_else(|| "screen".to_owned(), |s| s.to_string_lossy().into_owned());
    let page = preview_page(
        &title,
        &device,
        scale,
        screen_name,
        theme.colors.surface,
        shared.as_ref().map(|(link, _)| link.as_str()),
    );
    let qr_svg = shared.as_ref().map(|(_, code)| code.to_svg());

    let summary = format!(
        "previewing {} as {} ({}) at {url} — Ctrl-C to stop (rustc {:.1?})",
        screen.path.display(),
        device.name,
        device.size_label(),
        rustc,
    );
    println!("{summary}");
    if let Some((link, code)) = &shared {
        println!(
            "\nshared on your network at {link} — scan this on a phone on the same Wi-Fi.\n\
             Everyone who opens it drives the same preview.\n\n{}",
            code.to_text()
        );
    }

    if open {
        open_browser(&url);
    }

    loop {
        let (mut stream, _) = listener
            .accept()
            .map_err(|error| format!("cannot accept a connection — {error}"))?;
        let _ = stream.set_nodelay(true);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
        if let Err(error) = handle(&mut stream, &mut served, &page, qr_svg.as_deref()) {
            // A browser that hangs up mid-request is not the server's
            // problem; a sentence on stderr is the whole cost.
            eprintln!("preview: {error}");
        }
    }
}

/// Serve one HTTP/1.1 request off `stream`: read it, route it, respond, and
/// let the connection go (`Connection: close` — the page polls, it does not
/// hold sockets open).
///
/// Split out from the accept loop so a test can drive the routing with an
/// in-memory pair instead of a socket.
fn handle(
    stream: &mut TcpStream,
    served: &mut Served,
    page: &str,
    qr_svg: Option<&str>,
) -> Result<(), String> {
    let mut reader = std::io::BufReader::new(stream);
    let request = read_request(&mut reader)?;
    let stream = reader.into_inner();
    match (&request.method[..], &request.path[..]) {
        ("GET", "/") => respond(
            stream,
            "200 OK",
            "text/html; charset=utf-8",
            &[],
            page.as_bytes(),
        ),
        ("GET", "/frame") => {
            let (bytes, width, height, wants) = served.frame()?;
            let wants = if wants { "1" } else { "0" };
            respond(
                stream,
                "200 OK",
                "image/png",
                &[
                    ("X-Wants-Frame", wants),
                    ("X-Frame-Width", &width.to_string()),
                    ("X-Frame-Height", &height.to_string()),
                ],
                &bytes,
            )
        }
        ("POST", "/event") => {
            let event = event_from_json(&request.body, *served.now.borrow())
                .map_err(|why| format!("the page sent something the server cannot read: {why}"))?;
            if let Some(event) = event {
                served.events.borrow_mut().push(event);
            }
            respond(stream, "204 No Content", "text/plain", &[], &[])
        }
        ("GET", "/qr.svg") => match qr_svg {
            Some(svg) => respond(stream, "200 OK", "image/svg+xml", &[], svg.as_bytes()),
            None => respond(
                stream,
                "404 Not Found",
                "text/plain",
                &[],
                b"this preview is not shared - run it with --share",
            ),
        },
        ("GET", "/favicon.ico") => respond(stream, "404 Not Found", "text/plain", &[], b"no icon"),
        (method, path) => respond(
            stream,
            "404 Not Found",
            "text/plain",
            &[],
            format!("{method} {path} is not a route this server has").as_bytes(),
        ),
    }
}

/// One parsed HTTP request — the method, the path, and the body, which is
/// all three routes need. Headers beyond `Content-Length` are read and
/// ignored, which is what HTTP/1.1 allows a small server to do.
struct Request {
    method: String,
    path: String,
    body: String,
}

/// Read one request off `input`, up to and including its body.
fn read_request(input: &mut impl Read) -> Result<Request, String> {
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    // Headers end at the first empty line: `\r\n\r\n`. Read byte-wise — a
    // request is a few hundred bytes, and correctness beats buffering here.
    while !head.ends_with(b"\r\n\r\n") {
        match input.read(&mut byte) {
            Ok(0) => return Err("the connection closed before the request ended".to_owned()),
            Ok(_) => head.push(byte[0]),
            Err(error) => return Err(format!("cannot read the request: {error}")),
        }
        if head.len() > 16 * 1024 {
            return Err("the request's headers are larger than any page's".to_owned());
        }
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or_default().to_owned();
    let mut parts = request_line.split(' ');
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();

    let mut length = 0_usize;
    for line in lines {
        if let Some(value) = line
            .to_ascii_lowercase()
            .strip_prefix("content-length:")
            .map(str::trim)
            .and_then(|value| value.parse::<usize>().ok())
        {
            length = value;
        }
    }
    if length > 64 * 1024 {
        return Err("an event larger than any event".to_owned());
    }
    let mut body = vec![0_u8; length];
    if length > 0 {
        input
            .read_exact(&mut body)
            .map_err(|error| format!("cannot read the request's body: {error}"))?;
    }
    Ok(Request {
        method,
        path,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

/// Write one response: status line, the headers that matter (type, length,
/// connection), a blank line, the body.
fn respond(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Result<(), String> {
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.write_all(body))
        .and_then(|_| stream.flush())
        .map_err(|error| format!("cannot write the response: {error}"))
}

/// The page's JSON event, as a [`RemoteEvent`] — the one translation between
/// what a browser naturally reports and what the guest's event path takes.
///
/// The shapes are the page's own (`preview_page` writes the sender), so the
/// parser is a contract with ourselves: small, total, and refusing anything
/// that is not one of the six kinds.
///
/// # Errors
///
/// The JSON will not parse, or parses to a shape this server never sends.
pub(crate) fn event_from_json(text: &str, now: Duration) -> Result<Option<RemoteEvent>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let json = Json::parse(text).map_err(|error| format!("{error} in {text}"))?;
    let kind = json
        .get("type")
        .and_then(Json::as_str)
        .ok_or("an event with no type")?
        .to_owned();
    let point = |json: &Json| -> Option<Offset> {
        Some(Offset::new(
            json.get("x")?.as_f32()?,
            json.get("y")?.as_f32()?,
        ))
    };
    let event = match kind.as_str() {
        "pointerdown" => RemoteEvent::Pointer(Pointer::Down(
            point(&json).ok_or("a pointer event with no coordinates")?,
        )),
        "pointermove" => RemoteEvent::Pointer(Pointer::Move(
            point(&json).ok_or("a pointer event with no coordinates")?,
        )),
        "pointerup" => RemoteEvent::Pointer(Pointer::Up(
            point(&json).ok_or("a pointer event with no coordinates")?,
        )),
        "hover" => {
            // The page sends coordinates, or nothing at all for "left".
            match point(&json) {
                Some(at) => RemoteEvent::Hover(Some(at)),
                None => RemoteEvent::Hover(None),
            }
        }
        "char" => {
            let character = json
                .get("text")
                .and_then(Json::as_str)
                .ok_or("a char event with no text")?;
            RemoteEvent::Key(KeyEvent::character(character.to_owned(), now))
        }
        "key" => {
            let name = json
                .get("key")
                .and_then(Json::as_str)
                .ok_or("a key event with no key")?;
            RemoteEvent::Key(KeyEvent::named(named_key(name)?, now))
        }
        "scroll" => {
            let at = point(&json).ok_or("a scroll with no position")?;
            let dx = json.get("dx").and_then(Json::as_f32).unwrap_or(0.0);
            let dy = json.get("dy").and_then(Json::as_f32).unwrap_or(0.0);
            RemoteEvent::Scroll(ScrollEvent {
                position: at,
                delta: Offset::new(dx, dy),
                timestamp: now,
            })
        }
        other => return Err(format!("no event of kind {other}")),
    };
    Ok(Some(event))
}

/// A key's page spelling as the framework's own [`NamedKey`]. The list is
/// the keys a screen can actually do something with; everything else the
/// browser reports is not translated rather than half-translated.
fn named_key(name: &str) -> Result<NamedKey, String> {
    Ok(match name {
        "enter" => NamedKey::Enter,
        "tab" => NamedKey::Tab,
        "backspace" => NamedKey::Backspace,
        "escape" => NamedKey::Escape,
        "delete" => NamedKey::Delete,
        "arrowleft" => NamedKey::ArrowLeft,
        "arrowright" => NamedKey::ArrowRight,
        "arrowup" => NamedKey::ArrowUp,
        "arrowdown" => NamedKey::ArrowDown,
        "home" => NamedKey::Home,
        "end" => NamedKey::End,
        other => return Err(format!("{other} is not a key this preview sends")),
    })
}

/// Best-effort `xdg-open`/`open`/`start` at the URL, ignored when it fails —
/// a machine without a browser (a server, a CI box) still gets its preview;
/// the URL is printed either way.
fn open_browser(url: &str) {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "cmd"
    } else {
        "xdg-open"
    };
    let mut command = Command::new(program);
    if cfg!(target_os = "windows") {
        command.args(["/C", "start", url]);
    } else {
        command.arg(url);
    }
    let _ = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

// ---- the page ----------------------------------------------------------------

/// The page the server hands a browser: a device-framed canvas, a polling
/// loop, and the input translators. Self-contained on purpose — one file,
/// no CDN, no build step, the same "no third-party" policy the server has.
///
/// The page's own chrome (the header, the stage) is *ours*, not the
/// screen's theme: a dark stage with its own light ink, whatever theme the
/// screen inside it happens to be wearing — the preview pane's stage does
/// the same, so a light-theme screen and a dark one sit on the same
/// neutral frame.
#[must_use]
pub(super) fn preview_page(
    title: &str,
    device: &Device,
    scale: f32,
    screen: Option<&str>,
    surface: Color,
    share: Option<&str>,
) -> String {
    let width = device.width.round() as u32;
    let height = device.height.round() as u32;
    let radius = match device.platform {
        Platform::Ios => 44,
        Platform::Android => 30,
        Platform::Desktop => 10,
    };
    let notch = matches!(device.platform, Platform::Ios) && width < height && device.width <= 500.0;
    let notch_on = if notch { "block" } else { "none" };
    let heading = match screen {
        Some(name) => format!("{title} — {name}"),
        None => title.to_owned(),
    };
    r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>__TITLE__ · vieww preview</title>
<style>
  html, body { margin: 0; min-height: 100vh; }
  body { background: #17161a; color: #e4e4ea; font: 13px/1.4 system-ui, sans-serif;
         display: flex; flex-direction: column; align-items: center; gap: 14px;
         padding: 22px 0 40px; box-sizing: border-box; }
  header { display: flex; align-items: center; gap: 10px; opacity: 0.9; }
  #dot { width: 8px; height: 8px; border-radius: 50%; background: #4ade80;
         transition: background 0.3s; }
  #dot.stale { background: #f59e0b; }
  #dot.dead { background: #ef4444; }
  .device { position: relative; border-radius: __RADIUS__px;
            box-shadow: 0 0 0 8px #232227, 0 0 0 9px #33323a, 0 30px 60px rgba(0,0,0,.55);
            overflow: hidden; }
  canvas { display: block; width: __W__px; height: __H__px; background: __SURFACE__;
           outline: none; touch-action: none; }
  #share { display: flex; align-items: center; gap: 14px; padding: 12px 16px;
           background: #232227; border-radius: 12px; max-width: 520px; }
  #share[hidden] { display: none; }
  #share img { width: 132px; height: 132px; border-radius: 6px; flex: none; }
  #share a { color: #a5b4fc; word-break: break-all; }
  /* On a phone the page *is* the device: no stage, no frame, the screen
     edge to edge, so a shared preview feels like the app rather than a
     picture of one. */
  @media (max-width: 560px) {
    body { padding: 0; gap: 0; }
    header, #share, .notch { display: none; }
    .device { box-shadow: none; border-radius: 0; }
    canvas { width: 100vw; height: auto; }
  }
  .notch { position: absolute; top: 11px; left: 50%; transform: translateX(-50%);
           width: 125px; height: 36px; border-radius: 18px; background: #17161a;
           pointer-events: none; display: __NOTCH__; }
  p.error { display: none; max-width: 80ch; white-space: pre-wrap;
            font-family: ui-monospace, monospace; font-size: 12px;
            color: #fca5a5; margin: 0; }
</style>
</head>
<body>
<header><div id="dot"></div><span>__HEADING__ · __DEVICE__ (__W__×__H__)</span></header>
<div class="device">
  <canvas id="c" width="__RASTERW__" height="__RASTERH__" tabindex="0"></canvas>
  <div class="notch"></div>
</div>
<p class="error" id="error"></p>
<aside id="share" __SHARE_HIDDEN__>
  <img src="/qr.svg" alt="QR code for the shared preview">
  <div>Scan on a phone on the same Wi-Fi, or open<br><a href="__SHARE_URL__">__SHARE_URL__</a><br>
  <span style="opacity:.7">Everyone who opens it drives the same preview.</span></div>
</aside>
<script>
const W = __W__, H = __H__, RATE = 1000 / 30, IDLE = 350;
const c = document.getElementById('c'), x = c.getContext('2d');
const dot = document.getElementById('dot'), error = document.getElementById('error');
let alive = true, wanted = false, hoverAt = null;

function logical(event) {
  const r = c.getBoundingClientRect();
  return { x: Math.round((event.clientX - r.left) * (W / r.width) * 10) / 10,
           y: Math.round((event.clientY - r.top) * (H / r.height) * 10) / 10 };
}
function send(body) {
  fetch('/event', { method: 'POST', body: JSON.stringify(body) })
    .then(() => { poll(true); })
    .catch(() => {});
}
async function poll(soon) {
  if (!alive) return;
  try {
    const r = await fetch('/frame');
    if (!r.ok) { throw new Error('HTTP ' + r.status); }
    const blob = await r.blob();
    wanted = r.headers.get('X-Wants-Frame') === '1';
    const bmp = await createImageBitmap(blob);
    x.drawImage(bmp, 0, 0, c.width, c.height);
    dot.className = '';
    error.style.display = 'none';
  } catch (e) {
    dot.classList.add('stale');
  }
  setTimeout(() => poll(), soon ? 0 : (wanted ? RATE : IDLE));
}

c.addEventListener('pointerdown', e => {
  c.setPointerCapture(e.pointerId); c.focus();
  const p = logical(e); hoverAt = p;
  send({ type: 'pointerdown', x: p.x, y: p.y });
});
c.addEventListener('pointermove', e => {
  const p = logical(e);
  if (e.buttons) { send({ type: 'pointermove', x: p.x, y: p.y }); }
  else if (!hoverAt || Math.abs(p.x - hoverAt.x) > 1 || Math.abs(p.y - hoverAt.y) > 1) {
    hoverAt = p; send({ type: 'hover', x: p.x, y: p.y });
  }
});
c.addEventListener('pointerup', e => {
  const p = logical(e); send({ type: 'pointerup', x: p.x, y: p.y });
});
c.addEventListener('pointerleave', () => { hoverAt = null; send({ type: 'hover' }); });
c.addEventListener('wheel', e => {
  e.preventDefault(); c.focus();
  const p = logical(e);
  send({ type: 'scroll', x: p.x, y: p.y, dx: -e.deltaX, dy: -e.deltaY });
}, { passive: false });
c.addEventListener('keydown', e => {
  if (e.metaKey || e.ctrlKey || e.altKey) return;
  if (e.key.length === 1) { e.preventDefault(); send({ type: 'char', text: e.key }); }
  else if (['Enter','Tab','Backspace','Escape','Delete','ArrowLeft','ArrowRight',
            'ArrowUp','ArrowDown','Home','End'].includes(e.key)) {
    e.preventDefault(); send({ type: 'key', key: e.key.toLowerCase() });
  }
});
poll();
</script>
</body>
</html>
"#
    .replace("__TITLE__", &super::escape_html(&heading))
    .replace("__HEADING__", &super::escape_html(&heading))
    .replace("__DEVICE__", device.name)
    .replace("__W__", &width.to_string())
    .replace("__H__", &height.to_string())
    .replace("__RASTERW__", &(width * scale as u32).to_string())
    .replace("__RASTERH__", &(height * scale as u32).to_string())
    .replace("__RADIUS__", &radius.to_string())
    .replace(
        "__SHARE_HIDDEN__",
        if share.is_some() { "" } else { "hidden" },
    )
    .replace("__SHARE_URL__", &super::escape_html(share.unwrap_or("")))
    .replace("__NOTCH__", notch_on)
    .replace(
        "__SURFACE__",
        &format!("#{:02x}{:02x}{:02x}", surface.r, surface.g, surface.b),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_events_the_page_sends_parse_to_the_events_the_guest_takes() {
        let now = Duration::from_millis(1200);
        let event = event_from_json(r#"{"type":"pointerdown","x":10.5,"y":20}"#, now)
            .unwrap()
            .expect("an event");
        assert_eq!(
            event,
            RemoteEvent::Pointer(Pointer::Down(Offset::new(10.5, 20.0)))
        );

        let event = event_from_json(r#"{"type":"hover"}"#, now)
            .unwrap()
            .expect("an event");
        assert_eq!(event, RemoteEvent::Hover(None));

        let event = event_from_json(r#"{"type":"hover","x":3,"y":4}"#, now)
            .unwrap()
            .expect("an event");
        assert_eq!(event, RemoteEvent::Hover(Some(Offset::new(3.0, 4.0))));

        let event = event_from_json(r#"{"type":"char","text":"a"}"#, now)
            .unwrap()
            .expect("an event");
        assert!(matches!(event, RemoteEvent::Key(_)));

        let event = event_from_json(r#"{"type":"key","key":"enter"}"#, now)
            .unwrap()
            .expect("an event");
        assert!(matches!(event, RemoteEvent::Key(_)));

        let event = event_from_json(r#"{"type":"scroll","x":100,"y":400,"dx":0,"dy":-120}"#, now)
            .unwrap()
            .expect("an event");
        match event {
            RemoteEvent::Scroll(scroll) => {
                assert_eq!(scroll.position, Offset::new(100.0, 400.0));
                assert_eq!(scroll.delta, Offset::new(0.0, -120.0));
            }
            other => panic!("a scroll, not {other:?}"),
        }
    }

    #[test]
    fn an_event_this_page_would_never_send_is_refused() {
        assert!(event_from_json(r#"{"type":"detonate"}"#, Duration::ZERO).is_err());
        assert!(event_from_json(r#"{"type":"pointerdown"}"#, Duration::ZERO).is_err());
        assert!(event_from_json("not json", Duration::ZERO).is_err());
        // Empty is not an error — the page's fetch fallback sends nothing.
        assert!(event_from_json("", Duration::ZERO).unwrap().is_none());
    }

    #[test]
    fn the_page_carries_the_device_and_the_canvas_at_the_raster_scale() {
        let device = crate::state::Device::ALL
            .iter()
            .find(|d| d.name == "Phone")
            .copied()
            .unwrap();
        let page = preview_page("home", &device, 2.0, Some("Settings"), Color::WHITE, None);
        assert!(page.contains("home — Settings"), "{page}");
        assert!(page.contains("width: 393px"), "{page}");
        assert!(
            page.contains("width=\"786\""),
            "the canvas is the scaled raster"
        );
        assert!(page.contains("Phone"), "{page}");
        assert!(
            page.contains("display: block"),
            "a phone carries a notch: {page}"
        );
        // The page's own chrome is the stage's, not the screen theme's — a
        // light ink on the dark stage, whatever the screen wears.
        assert!(page.contains("color: #e4e4ea"), "{page}");
    }

    #[test]
    fn a_desktop_page_has_no_notch() {
        let device = crate::state::Device::ALL
            .iter()
            .find(|d| d.name == "Desktop")
            .copied()
            .unwrap();
        let page = preview_page("home", &device, 1.0, None, Color::WHITE, None);
        assert!(page.contains("display: none"), "{page}");
    }

    #[test]
    fn a_shared_page_shows_its_link_and_code() {
        let device = crate::state::Device::ALL[1];
        let local = preview_page("home", &device, 1.0, None, Color::WHITE, None);
        assert!(local.contains("<aside id=\"share\" hidden>"), "{local}");
        let shared = preview_page(
            "home",
            &device,
            1.0,
            None,
            Color::WHITE,
            Some("http://192.168.1.5:7878"),
        );
        assert!(shared.contains("<aside id=\"share\" >"), "{shared}");
        assert!(
            shared.contains("href=\"http://192.168.1.5:7878\""),
            "{shared}"
        );
        assert!(shared.contains("/qr.svg"));
        assert!(
            shared.contains("touch-action: none"),
            "a phone's finger drives, not scrolls"
        );
    }
}
