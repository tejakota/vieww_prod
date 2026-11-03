//! ST-08: the preview on a real device, and the device's screen back in the
//! pane — the state loop, end to end, with a fake `adb`.
//!
//! # What this file proves, and how
//!
//! The *payload* — which `adb` invocations a mirror makes, in what order, and
//! every refusal that stops it before anything runs — is `export`'s own test
//! suite's subject. What only the state layer can prove is the loop: that
//! `start_mirror` drives its five steps through the real jobs queue (real
//! spawned processes, real stdout, real exit statuses), that a failed step
//! stops the mirror without leaving a half-frame anywhere, and that the last
//! step's pulled PNG is decoded into the pane's `device_mirror` with the
//! dimensions the file actually had.
//!
//! The `adb` here is a shell script that answers the five subcommands the
//! mirror uses — `install`, `shell am start`, `shell screencap`, `shell rm`,
//! `pull` (which copies a PNG the test wrote with the studio's own encoder) —
//! and fails loudly on anything else, so a mirror that grows a sixth
//! invocation fails here rather than silently doing something new to a
//! phone. No Android device, SDK or platform tools exist on the machine this
//! runs on, and none are needed: the assertions are about the studio's side
//! of the wire.
//!
//! The farm dispatch is tested the same way, against a `gh` that prints what
//! the real one prints and exits 0 — a dispatch is one API call whose payload
//! is pinned in `export`'s tests; what this proves is that the studio runs
//! it, and says afterwards where the results will land.
//!
//! **Run with `--test-threads=1`.** The fake tools go on `PATH`, which is
//! process-global: the file's tests take a mutex around the mutation and
//! restore the old value before releasing it, but a parallel test *binary* is
//! a separate process and unaffected only because it is. Within this file the
//! mutex serializes everything, which the argument already asks for.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use vieww_element::Runtime;
use viewwstudio::buffer::Workspace;
use viewwstudio::export::{Device, DeviceState};
use viewwstudio::state::Studio;
use viewwstudio::toolchains::Target;

/// Serializes this file's tests around the `PATH` mutation.
static TOOLS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A project with a manifest, a library, and the Gradle file that names the
/// applicationId the phone will know the app by — the mirror's launch step
/// reads exactly that line.
fn project(tag: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("vieww-device-mirror-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).expect("the project tree");
    std::fs::create_dir_all(root.join("android").join("app")).expect("the gradle tree");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"my-app\"\nversion = \"0.1.0\"\n\n[workspace]\n",
    )
    .expect("the manifest");
    std::fs::write(root.join("src").join("lib.rs"), "// the app\n").expect("the library");
    std::fs::write(
        root.join("android").join("app").join("build.gradle"),
        "android {\n    defaultConfig {\n        applicationId \"com.example.my_app\"\n    }\n}\n",
    )
    .expect("the gradle file");
    root
}

/// A screenshot-shaped PNG, written with the studio's own encoder: the decode
/// step at the end of the mirror is exercised against bytes this crate
/// produced, not a fixture file that could drift from the encoder.
fn fixture_png(path: &Path) -> (u32, u32) {
    let (width, height) = (6, 10);
    let rgba: Vec<u8> = (0..width * height)
        .flat_map(|i| {
            let lum = (i * 37 % 256) as u8;
            [lum, lum.wrapping_add(80), 255 - lum, 255]
        })
        .collect();
    viewwstudio::png::write(path, &rgba, width, height).expect("write the PNG");
    (width, height)
}

/// The bin directory holding a fake `adb` and a fake `gh`.
///
/// `launch_ok` decides whether `am start` answers `Status: ok` or the error a
/// renamed package produces — the two outcomes the mirror's second step has.
fn fake_bin(dir: &Path, fixture: &Path, launch_ok: bool) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).expect("the bin directory");

    let am = if launch_ok {
        r#"printf 'Starting: Intent { act=android.intent.action.MAIN }\nStatus: ok\n'; exit 0"#
    } else {
        // Exit 0 on purpose: `am start -W` through some shells and wrappers
        // reports failure in its output and success in its exit code, which
        // is exactly the case the mirror's Status-line parse exists for — the
        // fake is the shape that would slip past an exit-code-only verdict.
        r#"printf 'Starting: Intent\nError: Activity class does not exist.\n'; exit 0"#
    };
    let script = format!(
        r#"#!/usr/bin/env bash
# The five subcommands a mirror uses, and nothing else — see the module doc.
set -u
if [ "${{1:-}}" = "-s" ]; then shift 2; fi
cmd="${{1:-}}"
case "$cmd" in
    install) exit 0 ;;
    pull)
        # $2 is the remote path, $3 the local destination the pull writes —
        # the one step whose argv has a direction, and the fake honours it.
        cp "{fixture}" "$3"
        echo "1 file pulled"
        exit 0 ;;
    shell)
        sub="${{2:-}}"
        case "$sub" in
            am) {am} ;;
            screencap) exit 0 ;;
            rm) exit 0 ;;
        esac ;;
    devices) echo "List of devices attached"; exit 0 ;;
esac
echo "fake adb: unsupported: $*" >&2
exit 1
"#,
        fixture = fixture.display(),
        am = am,
    );
    write_executable(&bin.join("adb"), &script);

    write_executable(
        &bin.join("gh"),
        r#"#!/usr/bin/env bash
echo "✓ Created workflow dispatch event for device-farm.yml (main)"
exit 0
"#,
    );
    bin
}

/// A script on disk with its executable bit set, where the platform has one.
fn write_executable(path: &Path, body: &str) {
    std::fs::write(path, body).expect("write the fake tool");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod the fake tool");
    }
}

/// A studio with that project open, the way a launch would have it.
fn studio(root: &Path) -> Studio {
    let runtime = Runtime::new();
    Studio::with_workspace(&runtime, Workspace::open(root))
        .with_host(vieww_foundation::TargetPlatform::Linux)
}

/// A Ready Android device, the shape `parse_adb_devices` produces.
fn phone() -> Device {
    Device {
        id: "R58M12ABCDE".into(),
        name: "Pixel 8".into(),
        kind: Target::Android,
        state: DeviceState::Ready,
    }
}

/// Poll the studio until `done` holds, and fail loudly if it never does.
fn pump(studio: &Studio, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(
            Instant::now() < deadline,
            "the mirror never finished — output so far:\n{}",
            studio.output.get().join("\n")
        );
        if !studio.poll_jobs() {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// The joined Output panel, for assertions and failure messages alike.
fn output(studio: &Studio) -> String {
    studio.output.get().join("\n")
}

#[test]
fn a_mirror_runs_every_step_and_lands_the_phones_pixels_in_the_pane() {
    let _guard = TOOLS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root = project("end-to-end");
    let fixture = root.join("screenshot.png");
    let (png_width, png_height) = fixture_png(&fixture);
    let bin = fake_bin(&root, &fixture, true);
    let apk = root.join("my-app.apk");
    std::fs::write(&apk, "an apk").expect("the exported artefact");

    // The fake has to be on PATH before anything runs, and it has to stay
    // there until the run has forked its last child: every step is spawned
    // on a worker thread and inherits the live process environment at fork
    // time, so restoring PATH mid-run would hand the next `adb` a PATH
    // without the fake on it — a race the exit statuses would then blame on
    // a phone that answered fine.
    let old_path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{}:{old_path}", bin.display()));
    let studio = studio(&root);
    studio.artefact.set(Some(apk));
    let device = phone();
    studio.devices.set(Rc::new(vec![device.clone()]));

    studio.start_mirror(&device);

    assert!(
        studio.mirror_run.borrow().is_some(),
        "the mirror started — instead the Output panel says:\n{}",
        output(&studio)
    );
    pump(&studio, || studio.mirror_run.borrow().is_none());
    std::env::set_var("PATH", &old_path);

    // The phone's pixels, in the pane, with the dimensions the file had: the
    // decode step is the only place those meet, and it ran.
    let mirror = studio.device_mirror.get().expect("the mirrored frame");
    assert_eq!(mirror.device, "Pixel 8");
    assert_eq!(
        (mirror.image.width(), mirror.image.height()),
        (png_width, png_height)
    );

    let logged = output(&studio);
    assert!(
        logged.contains("Mirror on Pixel 8 — 5 step(s)"),
        "the whole plan, before it ran:\n{logged}"
    );
    // The launch addressed the applicationId the Gradle file declares, not
    // the cargo package's name — the one difference that installs cleanly
    // and then fails to start.
    assert!(
        logged.contains("com.example.my_app/android.app.NativeActivity"),
        "the component the phone was asked for:\n{logged}"
    );
    assert!(
        logged.contains("Mirror on Pixel 8 finished"),
        "the finish line:\n{logged}"
    );

    // Dismissal is honest: the frame goes, and stays gone.
    studio.clear_mirror();
    assert!(studio.device_mirror.get().is_none());
}

#[test]
fn a_launch_that_fails_stops_the_mirror_and_leaves_the_pane_as_it_was() {
    let _guard = TOOLS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root = project("failed-launch");
    let fixture = root.join("screenshot.png");
    fixture_png(&fixture);
    // `am start` answers with the error a renamed package produces: the
    // install succeeded, the launch did not, and the honest mirror stops
    // rather than screenshotting the launcher.
    let bin = fake_bin(&root, &fixture, false);
    let apk = root.join("my-app.apk");
    std::fs::write(&apk, "an apk").expect("the exported artefact");

    let old_path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{}:{old_path}", bin.display()));
    let studio = studio(&root);
    studio.artefact.set(Some(apk));
    let device = phone();
    studio.devices.set(Rc::new(vec![device.clone()]));
    studio.start_mirror(&device);
    // Held until the mirror has finished, for the reason the end-to-end test
    // above gives: the steps fork one at a time against the live PATH.
    pump(&studio, || studio.mirror_run.borrow().is_none());
    std::env::set_var("PATH", &old_path);
    assert!(
        studio.device_mirror.get().is_none(),
        "a screenshot from a phone whose app never came up is a receipt for \
         the launcher, not the app"
    );
    let logged = output(&studio);
    assert!(
        logged.contains("stopped at step 2"),
        "the step that failed, by number:\n{logged}"
    );
    // adb's own words for the failure are in the Output panel too — the job
    // streamed them — so the number is never the whole story.
    assert!(
        logged.contains("Activity class does not exist"),
        "adb's own error, not a summary of it:\n{logged}"
    );
}

#[test]
fn a_mirror_without_an_artefact_or_a_ready_device_says_so_before_running_anything() {
    // No PATH mutation, no fake tools: both refusals are decided before
    // anything is spawned, which is the point of refusal-first.
    let root = project("refused");
    let studio = studio(&root);
    let device = phone();

    studio.start_mirror(&device);
    let logged = output(&studio);
    assert!(
        logged.contains("Mirror: nothing has been exported yet"),
        "the missing artefact, named:\n{logged}"
    );
    assert!(studio.mirror_run.borrow().is_none());

    // An artefact that exists, a device that has not accepted this computer's
    // key: plugged in is not permission.
    let apk = root.join("my-app.apk");
    std::fs::write(&apk, "an apk").expect("the exported artefact");
    studio.artefact.set(Some(apk));
    let unauthorised = Device {
        state: DeviceState::Unauthorised,
        ..phone()
    };
    studio.start_mirror(&unauthorised);
    let logged = output(&studio);
    assert!(
        logged.contains("the device is not ready"),
        "the unauthorised phone, in the studio's own words:\n{logged}"
    );
    assert!(studio.mirror_run.borrow().is_none());
}

#[test]
fn a_farm_dispatch_runs_gh_and_says_where_the_results_land() {
    let _guard = TOOLS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root = project("farm");
    let bin = fake_bin(&root, &root.join("screenshot.png"), true);

    let old_path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{}:{old_path}", bin.display()));
    let studio = studio(&root);
    studio.dispatch_farm("android", 3);
    // Let the job finish before the PATH goes back: the child inherits the
    // studio process's environment at spawn time, which is while the fake is
    // the `gh` it resolves to.
    pump(&studio, || {
        studio
            .jobs
            .borrow()
            .jobs()
            .iter()
            .all(|job| !job.is_cancellable())
    });
    std::env::set_var("PATH", &old_path);

    // The notice says what ran and where the results land — a dispatch is the
    // one action in the studio whose outcome is on another machine, and the
    // status bar is where the studio says things like this.
    let notice = studio.notice.get().unwrap_or_default();
    assert!(
        notice.contains("3 phone(s)") && notice.contains("ci-runs/device-farm-main"),
        "the notice tells the truth about scope and about where to look:\n{notice}"
    );
    assert!(
        notice.contains("framework's own conformance app"),
        "the farm runs the framework's suite, not the user's project, and the \
         notice does not let that be misread:\n{notice}"
    );
    // And gh's own reply is in the Output panel, as every job's stdout is.
    assert!(
        output(&studio).contains("workflow dispatch event"),
        "gh answered:\n{}",
        output(&studio)
    );
}

#[test]
fn a_farm_dispatch_without_gh_refuses_rather_than_failing_to_start() {
    if viewwstudio::toolchains::Env::host()
        .find_on_path("gh")
        .is_some()
    {
        // A machine with a real `gh` installed cannot observe the refusal, and
        // that is the machine the refusal is for.
        eprintln!("skipping: gh is on PATH");
        return;
    }
    let root = project("no-gh");
    let studio = studio(&root);
    studio.dispatch_farm("android", 3);
    let logged = output(&studio);
    assert!(
        logged.contains("Device farm: GitHub CLI (`gh`) is missing"),
        "the CLI, its install command and its login, before anything spends \
         device minutes:\n{logged}"
    );
}
