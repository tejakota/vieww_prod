//! The studio's presentation commands: a framed screenshot, the device
//! matrix as an image, recording the preview, and sharing it on the network.
//!
//! # One engine, two faces
//!
//! The three that produce files or a server run `viewwstudio-cli` as a job —
//! the same commands a script runs, with the device, orientation and theme
//! the pane is showing — so their output lands in the Output panel, they are
//! cancellable from the task list, and a fix in the CLI is a fix here. The
//! recorder is the exception: it keeps what *this* pane draws while a person
//! drives it, which no separate process can see. See [`crate::showcase`].

use super::*;

impl Studio {
    /// The saved file the preview renders from, and the directory a job about
    /// it runs in — or a notice saying why there is none.
    fn presentable(&self) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
        let Some(buffer) = self.active() else {
            self.notify("Open a screen first — a .say or .rs file");
            return None;
        };
        let Some(path) = buffer.path.clone() else {
            self.notify("Save this buffer first — the export renders the file on disk");
            return None;
        };
        if !matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("say" | "rs")
        ) {
            self.notify("That is not a screen — open a .say or .rs file");
            return None;
        }
        if buffer.dirty {
            self.notify("Save first — the export renders what is on disk");
            return None;
        }
        let dir = self
            .root
            .peek()
            .or_else(|| path.parent().map(std::path::Path::to_path_buf))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        Some((path, dir))
    }

    /// Start `viewwstudio-cli <args>` as a job; its id, or `None` (with a
    /// notice) when the binary is not beside the studio.
    fn run_cli(&self, label: &str, args: Vec<String>, dir: std::path::PathBuf) -> Option<u64> {
        let Some(binary) = crate::showcase::cli_binary() else {
            self.notify(
                "viewwstudio-cli is not next to the studio — build it with \
                 cargo build -p viewwstudio --bins",
            );
            return None;
        };
        let mut spec =
            crate::task::Spec::new(label.to_owned(), binary.to_string_lossy().into_owned(), dir);
        spec.args = args;
        let waker = self.waker.clone();
        let wake = move || {
            use vieww_foundation::task::FrameWaker as _;
            waker.wake();
        };
        let id = self
            .jobs
            .borrow_mut()
            .start(spec, crate::jobs::Follow::None, wake);
        self.jobs_generation.update(|n| *n = n.wrapping_add(1));
        self.panel_open.set(true);
        self.panel_tab.set(PanelTab::Output);
        Some(id)
    }

    /// The device, orientation and theme flags that reproduce the pane.
    fn device_args(&self, device: Device) -> Vec<String> {
        let mut args = vec!["--device".to_owned(), crate::cli::device_flag(device)];
        if device.landscape {
            args.push("--landscape".to_owned());
        }
        if self.preview_dark.peek() {
            args.push("--dark".to_owned());
        }
        args
    }

    /// Save the previewed screen, framed in the pane's device, as a PNG
    /// under the project's `target/render/`.
    pub fn export_framed_screenshot(&self) {
        let Some((path, dir)) = self.presentable() else {
            return;
        };
        let mut args = vec![
            "render".to_owned(),
            path.to_string_lossy().into_owned(),
            "--frame".to_owned(),
        ];
        args.extend(self.device_args(self.preview_device()));
        if self.run_cli("Framed screenshot", args, dir).is_some() {
            self.notify("Saving a framed screenshot — the path is in Output");
        }
    }

    /// Save the screen on the matrix's devices (the default three when the
    /// matrix is closed), framed and side by side, as one PNG.
    pub fn export_device_matrix(&self) {
        let Some((path, dir)) = self.presentable() else {
            return;
        };
        let shown = self.preview_matrix.peek();
        let devices = if shown.is_empty() {
            Self::default_matrix()
        } else {
            shown
        };
        let names: Vec<String> = devices
            .iter()
            .map(|d| crate::cli::device_flag(*d))
            .collect();
        let mut args = vec![
            "render".to_owned(),
            path.to_string_lossy().into_owned(),
            "--devices".to_owned(),
            names.join(","),
        ];
        if self.preview_dark.peek() {
            args.push("--dark".to_owned());
        }
        if self.run_cli("Device matrix image", args, dir).is_some() {
            self.notify("Saving the device matrix — the path is in Output");
        }
    }

    /// Serve the screen to the local network (a QR code in the browser page
    /// and in Output), or stop the share that is running.
    pub fn share_preview(&self) {
        if let Some(id) = self.share_job.get() {
            let running = self
                .jobs
                .borrow()
                .job(id)
                .is_some_and(crate::jobs::Job::is_cancellable);
            self.share_job.set(None);
            if running {
                self.jobs.borrow_mut().cancel(id);
                self.jobs_generation.update(|n| *n = n.wrapping_add(1));
                self.notify("Stopped sharing the preview");
                return;
            }
        }
        let Some((path, dir)) = self.presentable() else {
            return;
        };
        let mut args = vec![
            "preview".to_owned(),
            path.to_string_lossy().into_owned(),
            "--share".to_owned(),
        ];
        args.extend(self.device_args(self.preview_device()));
        if let Some(id) = self.run_cli("Shared preview", args, dir) {
            self.share_job.set(Some(id));
            self.notify(
                "Sharing the preview on your network — scan the QR code in the browser \
                 page or in Output; run the command again to stop",
            );
        }
    }

    /// Whether the preview is being recorded.
    #[must_use]
    pub fn is_recording(&self) -> bool {
        crate::showcase::is_recording()
    }

    /// Start recording the preview, or stop and save the recording.
    ///
    /// Recording keeps the frames the *isolated* preview draws — the path
    /// where the studio holds the guest's pixels — so starting one with the
    /// preview in process turns isolation on and renders again first.
    pub fn toggle_recording(&self) {
        if let Some(recording) = crate::showcase::stop_recording() {
            self.save_recording(recording);
            return;
        }
        if self.preview_remote.peek().is_none() {
            if crate::remote::RemotePreview::sibling_binary().is_none() {
                self.notify(&format!(
                    "Recording needs the isolated preview, and `{}` is not next to the \
                     studio — build it with cargo build -p viewwstudio --bins",
                    crate::remote::CHILD_BINARY
                ));
                return;
            }
            self.isolated_preview.set(true);
            self.render();
        }
        crate::showcase::start_recording();
        self.notify("Recording the preview — use it, then run Start or Stop Recording again");
    }

    /// Write a finished recording as a framed GIF and the script that
    /// replays it, under `<project>/.vieww/recordings/`.
    fn save_recording(&self, recording: crate::showcase::Recording) {
        let base = self
            .root
            .peek()
            .unwrap_or_else(|| std::env::temp_dir().join("viewwstudio"));
        let dir = base.join(".vieww").join("recordings");
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let gif_path = dir.join(format!("recording-{stamp}.gif"));
        let script_path = dir.join(format!("recording-{stamp}.script"));

        let device = self.preview_device();
        let (width, height) = (
            device.width.round().max(1.0) as u32,
            device.height.round().max(1.0) as u32,
        );
        let dark = self.preview_dark.peek();
        let surface = crate::cli::theme_data(dark).colors.surface;
        // The frame is built here, on the thread that owns the renderer's
        // fonts and factories; the GIF encode — the slow part — is not.
        let framer =
            crate::showcase::Framer::new(device.platform, device.landscape, dark, width, height)
                .ok();
        let steps = recording.script.steps.len();
        let script_text = format!(
            "# Recorded in vieww Studio on {} ({}). Replay it with:\n\
             #   viewwstudio-cli animate <screen> --device {}{} --frame --script {}\n{}",
            device.name,
            device.size_label(),
            crate::cli::device_flag(device),
            if device.landscape { " --landscape" } else { "" },
            script_path.display(),
            recording.script.to_text()
        );
        if let Err(error) =
            std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&script_path, script_text))
        {
            self.notify(&format!("Could not save the recording: {error}"));
            return;
        }
        let shown = gif_path.clone();
        std::thread::spawn(move || {
            let result = recording
                .gif(framer.as_ref(), surface)
                .and_then(|bytes| std::fs::write(&gif_path, bytes).map_err(|e| e.to_string()));
            if let Err(error) = result {
                let _ = std::fs::write(gif_path.with_extension("error.txt"), error);
            }
        });
        self.notify(&format!(
            "Recording saved: {} ({steps} steps) and its replayable .script beside it",
            shown.display()
        ));
    }
}
