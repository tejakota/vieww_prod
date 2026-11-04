//! ST-08: the preview on a real device, and the device's screen back in the
//! pane.
//!
//! # The two halves, and why they are different shapes
//!
//! The **mirror** is five `adb` steps driven like an export
//! ([`Studio::start_mirror`]): install the debug APK, launch it, screenshot
//! the phone, pull the PNG, clean up. The last step's success decodes the PNG
//! into [`MirrorFrame`] and puts it in `device_mirror`, where the preview pane
//! draws it. It is a *receipt* — the phone's pixels at one moment — and the
//! pane labels it as one, because the thing this exists to kill is "it looked
//! right in the preview" being said about a phone nobody looked at.
//!
//! The **farm dispatch** is one job ([`Studio::dispatch_farm`]): `gh workflow
//! run device-farm.yml`, with the inputs the workflow's own dispatch block
//! declares. It spends real device minutes, so it is a deliberate button
//! rather than anything automatic, and the notice after it says where the
//! results land and what actually ran — the framework's conformance app, not
//! the user's project; the user's app on real hardware is what the mirror
//! just did.
//!
//! # What is deliberately not here
//!
//! No pixel *comparison* against the studio's own render. The research
//! report's arc ends there, and getting there honestly needs the device's
//! density and inset facts as inputs to a comparable render, neither of which
//! `adb devices -l` reports. A number computed from guesses would be worse
//! than the eye doing the comparison beside an honest caption.

use super::*;

impl Studio {
    /// Mirror the preview onto `device`: install, launch, screenshot, pull,
    /// clean up — or say why not, before anything runs.
    ///
    /// Same refusal-first contract as [`start_export`](Self::start_export):
    /// [`export::mirror_steps`](crate::export::mirror_steps) checks the
    /// device's state, `adb` and the artefact first, so a phone that has not
    /// accepted this computer's key is a sentence about the phone, not an adb
    /// error three steps in.
    pub fn start_mirror(&self, device: &crate::export::Device) {
        let Some(root) = self.root.get().clone() else {
            self.append_output(vec![
                "Mirror: no project is open — there is nothing to install".to_string(),
            ]);
            self.show_output();
            return;
        };
        let Some(artefact) = self.artefact.get() else {
            self.append_output(vec![
                "Mirror: nothing has been exported yet — export the Android APK first".to_string(),
            ]);
            self.show_output();
            return;
        };
        let name = self.package_name().unwrap_or_default();
        // The phone knows the app by its Gradle applicationId, not by the
        // cargo package's name — the README tells the user to change it, and a
        // launcher that guessed the old one would report a renamed app as
        // missing when it is installed and fine. `export::launch_package`
        // reads the file that decides.
        let package = crate::export::launch_package(&root, &name);
        // The pulled PNG lands under `.vieww`, which is this studio's own
        // directory and this studio's to create — `adb pull` refuses a
        // destination whose parent does not exist, and a mirror that failed
        // on its last-but-one step for that reason would be a bug dressed as
        // a device problem.
        if let Err(error) = std::fs::create_dir_all(root.join(".vieww")) {
            self.append_output(vec![format!(
                "Mirror: the .vieww folder could not be created — {error}"
            )]);
            self.show_output();
            return;
        }
        match crate::export::mirror_steps(
            &self.configured_env(),
            &root,
            device,
            &artefact,
            &package,
        ) {
            Err(refusal) => {
                self.append_output(vec![format!("Mirror: {refusal}")]);
                self.show_output();
            }
            Ok(steps) => {
                // The plan, before it starts, for the same reason an export's
                // is: five adb invocations against a phone the user owns, and
                // being able to read what is about to touch it is the
                // difference between watching and waiting.
                self.append_output(vec![format!(
                    "Mirror on {} — {} step(s)",
                    device.name,
                    steps.len()
                )]);
                for (index, step) in steps.iter().enumerate() {
                    self.append_output(vec![format!(
                        "  {}. {} — {}",
                        index + 1,
                        step.command_line(),
                        step.label
                    )]);
                }
                let png = crate::export::mirror_png(&root, device);
                *self.mirror_run.borrow_mut() = Some(MirrorRun {
                    device: device.clone(),
                    steps,
                    next: 0,
                    png,
                });
                self.show_output();
                self.advance_mirror(true);
            }
        }
    }

    /// Run the next step of the mirror, or finish it. `ok` is the previous
    /// step's verdict; a failed step ends the mirror exactly as it ends an
    /// export — a screenshot from a phone whose app failed to install is a
    /// receipt for the launcher, not the app.
    pub(super) fn advance_mirror(&self, ok: bool) {
        let next = {
            let mut slot = self.mirror_run.borrow_mut();
            let Some(run) = slot.as_mut() else {
                return;
            };
            if !ok {
                // `run.next` has already moved past the step that failed, so
                // its 1-based number *is* `run.next`: the step that failed is
                // the one before the index. Printing the next index instead
                // would blame a step that never ran.
                let failed_at = run.next;
                let name = run.device.name.clone();
                *slot = None;
                drop(slot);
                self.append_output(vec![format!(
                    "Mirror on {name} stopped at step {failed_at} — the pane keeps whatever it \
                     already had"
                )]);
                self.jobs_generation.update(|n| *n = n.wrapping_add(1));
                return;
            }
            let index = run.next;
            run.next += 1;
            run.steps.get(index).cloned()
        };

        match next {
            Some(spec) => {
                let wake = self.frame_waker();
                // `capture`, not `start`: the launch step's verdict is am's own
                // `Status:` line — `am start -W` can exit 0 and still have
                // failed — and the Output panel still gets every line either
                // way. The other steps print nothing worth parsing ("Success",
                // "1 file pulled"), and capturing them costs the few lines they
                // print.
                self.jobs
                    .borrow_mut()
                    .capture(spec, crate::jobs::Follow::MirrorStep, wake);
                self.jobs_generation.update(|n| *n = n.wrapping_add(1));
            }
            None => {
                let run = self.mirror_run.borrow_mut().take();
                if let Some(run) = run {
                    self.finish_mirror(run);
                }
            }
        }
    }

    /// Decode the pulled PNG into the pane's mirror frame.
    ///
    /// The one step with no child process: the file is local, the decode is
    /// the same `vieww-asset` path the Assets view uses, and a failure names
    /// the file — a PNG that arrived mangled (the classic is a piped
    /// screencap, which this mirror never does) is a file to look at, not a
    /// stack trace.
    fn finish_mirror(&self, run: MirrorRun) {
        match std::fs::read(&run.png)
            .map_err(|error| format!("{}: {error}", run.png.display()))
            .and_then(|bytes| {
                vieww_asset::decode(&bytes)
                    .map(|(image, _)| image)
                    .map_err(|error| format!("{}: {error}", run.png.display()))
            }) {
            Ok(image) => {
                self.append_output(vec![format!(
                    "Mirror on {} finished — {}×{} from {}",
                    run.device.name,
                    image.width(),
                    image.height(),
                    run.png.display()
                )]);
                self.device_mirror.set(Some(Rc::new(MirrorFrame {
                    device: run.device.name.clone(),
                    image,
                })));
            }
            Err(why) => {
                self.append_output(vec![format!(
                    "Mirror on {} pulled a screenshot it could not decode — {why}",
                    run.device.name
                )]);
                self.show_output();
            }
        }
        self.jobs_generation.update(|n| *n = n.wrapping_add(1));
    }

    /// Clear the mirrored screenshot from the pane.
    ///
    /// The screenshot is not re-taken on its own: a phone's screen changes and
    /// the studio's copy does not, so the copy has a dismiss button and no
    /// opinions.
    pub fn clear_mirror(&self) {
        self.device_mirror.set(None);
    }

    /// Dispatch the device farm workflow on `count` phones (ST-08), or say why
    /// not before anything spends device minutes.
    ///
    /// `platform` is the workflow's own input name (`android`, `ios`, `both`).
    /// The dispatch is an API call as the signed-in `gh` user, so the notice
    /// that follows says where the results land rather than pretending the
    /// studio will show them: the farm publishes to `ci-runs/device-farm-main`
    /// and the run's artifacts, not to this window.
    pub fn dispatch_farm(&self, platform: &str, count: u32) {
        let root = self.root.get().clone().unwrap_or_else(std::env::temp_dir);
        match crate::export::farm_dispatch(&self.configured_env(), &root, platform, count) {
            Ok(spec) => {
                let wake = self.frame_waker();
                self.jobs
                    .borrow_mut()
                    .start(spec, crate::jobs::Follow::None, wake);
                self.jobs_generation.update(|n| *n = n.wrapping_add(1));
                // What actually runs, said at the moment of pressing the
                // button: the farm is the *framework's* conformance suite on
                // real phones — the renderer the preview certifies itself
                // against — and not this project. A button in the Export view
                // that let that be misread would be a button that lied by
                // adjacency.
                self.notify(&format!(
                    "device farm dispatched on {count} phone(s) — it runs the framework's \
                     own conformance app, not this project; results land on \
                     ci-runs/device-farm-main and in the run's artifacts"
                ));
            }
            Err(refusal) => {
                self.append_output(vec![format!("Device farm: {refusal}")]);
                self.show_output();
            }
        }
    }
}
