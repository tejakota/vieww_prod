//! ST-14's state layer: the update check and the two downloads, driven
//! through the jobs queue.
//!
//! # The shape, and why it is this shape
//!
//! Every download here is a plan handed to [`jobs`](crate::jobs) and walked
//! one step at a time, exactly like an export or a device mirror, because the
//! reasons are the same: the user can watch what is about to happen, a failed
//! step ends the run instead of papering over it, and nothing runs in the UI
//! thread. The pure half — URLs, refusals, the checksum and manifest
//! verification — lives in [`crate::update`], which is why the tests there can
//! cover it without a studio; this file is only the loop that connects a plan
//! to the panel and the status bar.
//!
//! # What the user actually sees
//!
//! *Check for Updates* fetches the release feed and answers in the status
//! bar: up to date, or a version with its tag. *Download the Update* (only
//! meaningful after a check found one) fetches the published `SHA256SUMS`,
//! fetches the archive, hashes it in-process and compares — a mismatch stops
//! everything with both digests in the Output panel, and nothing unverified
//! is ever placed where a person would install from. A match stages the
//! archive under the data directory and says where, because applying an
//! update means replacing the binary **and** the rlibs it is paired with,
//! under an installation this running process cannot know the shape of — see
//! `crate::update`'s module docs for the whole argument.
//!
//! *Download the vieww SDK* is the first-launch half of the split bundle: the
//! other half of this studio's own release, unpacked beside the executable
//! where `install::target_dir` already looks, then re-verified through the
//! same `sdk::Manifest` machinery the full bundle answers to. It refuses when
//! the toolchain already works — an SDK fetched over a working one is a
//! download with nothing to add — and it ends by asking for a restart,
//! because `Toolchain::discover` runs once, at launch, by design.

use super::*;

impl Studio {
    /// Ask the release feed whether a newer studio exists (ST-14).
    ///
    /// The verdict arrives in the status bar through
    /// `read_update_feed` when the fetch ends; this
    /// method is the refusal half plus the spawn.
    pub fn check_for_updates(&self) {
        if self.update_run.borrow().is_some() {
            self.append_output(vec![
                "Update: a download is already in flight — let it finish first".to_owned(),
            ]);
            self.show_output();
            return;
        }
        let Some(repo) = crate::update::repo() else {
            self.append_output(vec![
                "Update: this build does not know which repository it was published to — set \
                 VIEWW_REPO=owner/name, or install a packaged build, before asking about \
                 updates"
                    .to_owned(),
            ]);
            self.show_output();
            return;
        };
        if self.configured_env().find_on_path("curl").is_none() {
            // The refusal with its install line, not a network error in a
            // status bar. `curl` ships with every platform this studio
            // packages for; a machine without it is told so, once.
            self.append_output(vec![format!(
                "Update: {}",
                crate::export::Refusal::Missing {
                    requirement: "curl",
                    install: "it ships with Windows 10 1803+, macOS and desktop Linux; on a \
                              machine without it, `apt install curl` (or the platform's \
                              equivalent)",
                }
            )]);
            self.show_output();
            return;
        }
        self.append_output(vec![format!(
            "Checking for updates against {repo} — this is {}",
            crate::about::VERSION
        )]);
        // The fetch runs with the staging directory as its working directory,
        // so the directory has to exist before the child is forked — a cwd
        // that is not there is a `could not start` about nothing the user
        // can see.
        if let Err(error) = std::fs::create_dir_all(&self.update_dir) {
            self.append_output(vec![format!(
                "Update: {} could not be created — {error}",
                self.update_dir.display()
            )]);
            self.show_output();
            return;
        }
        let spec = crate::task::Spec::new("Check for updates", "curl", self.update_dir.clone())
            .args(["-fsSL", "--max-time", "30"])
            .arg(crate::update::feed_url(&repo))
            .timeout(std::time::Duration::from_secs(60));
        let wake = self.frame_waker();
        self.jobs
            .borrow_mut()
            .capture(spec, crate::jobs::Follow::UpdateFeed, wake);
        self.jobs_generation.update(|n| *n = n.wrapping_add(1));
    }

    /// The feed fetch ended: parse it, compare, and say which of the two
    /// things a feed can honestly mean.
    pub(super) fn read_update_feed(&self, id: u64) {
        let captured = self
            .jobs
            .borrow()
            .job(id)
            .map(crate::jobs::Job::captured)
            .unwrap_or_default();
        let verdict = match crate::update::parse_feed(&captured) {
            Ok(release) => {
                if crate::update::is_newer(&release.tag, crate::about::VERSION) {
                    self.update_release.set(Some(Rc::new(release.clone())));
                    self.notify(&format!(
                        "vieww Studio {} is available — Download the Update in the palette \
                         fetches and verifies it",
                        release.tag
                    ));
                    format!(
                        "vieww Studio {} is available; this is {}",
                        release.tag,
                        crate::about::VERSION
                    )
                } else {
                    self.notify(&format!(
                        "vieww Studio {} is the newest release",
                        crate::about::VERSION
                    ));
                    format!(
                        "vieww Studio {} is the newest release ({})",
                        crate::about::VERSION,
                        release.tag
                    )
                }
            }
            Err(why) => {
                // The feed answered something this cannot read: a sentence in
                // the Output panel, not a status bar that pretends nothing
                // was asked.
                self.append_output(vec![format!("Update: {why}")]);
                self.show_output();
                format!("the feed could not be read — {why}")
            }
        };
        self.append_output(vec![verdict]);
    }

    /// Fetch and verify the update the last check found (ST-14).
    ///
    /// Refusal-first: no check has found anything, or a download is already
    /// in flight, each refuses before a byte moves.
    pub fn download_update(&self) {
        self.download(crate::update::Target::Update);
    }

    /// Fetch and verify the SDK half of this studio's split bundle (ST-14).
    ///
    /// Refuses when the preview toolchain already works, for the reason the
    /// module docs give: a working SDK has nothing to gain from the network.
    pub fn download_sdk(&self) {
        if self.toolchain.is_ok() {
            self.append_output(vec![
                "SDK: the preview toolchain already works — the download is for a split bundle \
                 whose other half has not arrived"
                    .to_owned(),
            ]);
            self.show_output();
            return;
        }
        self.download(crate::update::Target::Sdk);
    }

    /// The shared body of both downloads: resolve, plan, print the plan, go.
    fn download(&self, target: crate::update::Target) {
        if self.update_run.borrow().is_some() {
            self.append_output(vec![
                "Update: a download is already in flight — let it finish first".to_owned(),
            ]);
            self.show_output();
            return;
        }
        let Some(repo) = crate::update::repo() else {
            self.append_output(vec![
                "Update: this build does not know which repository it was published to — set \
                 VIEWW_REPO=owner/name, or install a packaged build, before downloading \
                 anything"
                    .to_owned(),
            ]);
            self.show_output();
            return;
        };
        let env = self.configured_env();
        let staging = self.update_dir.clone();
        let planned =
            match target {
                crate::update::Target::Update => {
                    // Only a release the studio has *seen* is downloadable: the
                    // checksums and the archive come from the same feed answer.
                    let Some(release) = self.update_release.get() else {
                        self.append_output(vec![
                            "Update: no release has been found yet — Check for Updates first"
                                .to_owned(),
                        ]);
                        self.show_output();
                        return;
                    };
                    crate::update::update_steps(&env, &staging, &release)
                        .map(|(asset, steps)| (asset, steps, release.tag.clone(), staging.clone()))
                }
                crate::update::Target::Sdk => {
                    // The SDK directory beside the executable — the one place
                    // `install::target_dir` already searches, so the bundle a
                    // restart finds is the bundle this unpacked.
                    let Some(destination) = self.sdk_destination.clone().or_else(|| {
                        crate::install::sdk_dir(std::env::current_exe().ok().as_deref())
                    }) else {
                        self.append_output(vec![
                            "SDK: the studio's own location could not be worked out, so there is \
                         nowhere beside it to unpack"
                                .to_owned(),
                        ]);
                        self.show_output();
                        return;
                    };
                    if let Err(error) = std::fs::create_dir_all(&destination) {
                        self.append_output(vec![format!(
                            "SDK: {} could not be created — {error}",
                            destination.display()
                        )]);
                        self.show_output();
                        return;
                    }
                    crate::update::sdk_steps(&env, &repo, &staging, &destination).map(|steps| {
                        (
                            crate::update::sdk_asset_name(),
                            steps,
                            crate::update::own_tag(),
                            destination,
                        )
                    })
                }
            };
        match planned {
            Err(refusal) => {
                self.append_output(vec![format!("{}: {refusal}", target.noun())]);
                self.show_output();
            }
            Ok((asset, steps, tag, destination)) => {
                // The plan, before it starts, for the same reason an export's
                // is: this is about to fetch several hundred megabytes, and
                // being able to read what is about to move is the difference
                // between watching and waiting.
                self.append_output(vec![format!(
                    "Fetching the {} for {tag} — {} step(s)",
                    target.noun(),
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
                let run = crate::update::UpdateRun {
                    target,
                    tag: tag.clone(),
                    asset: asset.clone(),
                    sums: staging.join("SHA256SUMS"),
                    archive: staging.join(&asset),
                    destination,
                    steps,
                    next: 0,
                };
                *self.update_run.borrow_mut() = Some(run);
                self.show_output();
                self.advance_update(true, "");
            }
        }
    }

    /// Run the next step of a download, or finish it. `ok` is the previous
    /// step's verdict; `captured` is its stdout, which exactly one step — the
    /// SDK's rustc probe — has anything worth saying in.
    pub(super) fn advance_update(&self, ok: bool, captured: &str) {
        let next = {
            let mut slot = self.update_run.borrow_mut();
            let Some(run) = slot.as_mut() else {
                return;
            };
            if !ok {
                let failed_at = run.next;
                let noun = run.target.noun();
                *slot = None;
                drop(slot);
                self.append_output(vec![format!(
                    "The {noun} stopped at step {failed_at} — nothing was applied"
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
                // Every step is captured: the fetches print nothing worth
                // reading, the unpack none either, and the probe's stdout is
                // its verdict — one rule, so no step's output is ever the
                // wrong side of a label string.
                let wake = self.frame_waker();
                self.jobs
                    .borrow_mut()
                    .capture(spec, crate::jobs::Follow::UpdateStep, wake);
                self.jobs_generation.update(|n| *n = n.wrapping_add(1));
            }
            None => self.finish_update(captured),
        }
    }

    /// The in-process half of a finished download: the checksum check, the
    /// staging, and — for the SDK — the manifest verify.
    fn finish_update(&self, captured: &str) {
        let Some(run) = self.update_run.borrow_mut().take() else {
            return;
        };
        // **Before anything is used**: the digest of the bytes that arrived,
        // against what the release published. This is the step the whole
        // module exists for, and it is deliberately not a child process —
        // the comparison happens in the same process that will hand the user
        // the file.
        if let Err(why) = crate::update::verify_download(&run.sums, &run.archive) {
            self.append_output(vec![format!(
                "The {} for {} failed verification — {why}",
                run.target.noun(),
                run.tag
            )]);
            // Two strings, not one with a noun filled in: the noun is the
            // only difference, and a translation that has to agree with a
            // runtime `if` about grammar is a translation nobody checked.
            if run.target == crate::update::Target::Sdk {
                self.notify(
                    "the SDK's checksum did not match the published SHA256SUMS — nothing was \
                     unpacked",
                );
            } else {
                self.notify(
                    "the update's checksum did not match the published SHA256SUMS — nothing \
                     was installed",
                );
            }
            self.show_output();
            // The failed download is left in the staging directory, named as
            // it arrived, for a person to inspect; nothing moves it anywhere
            // it could be mistaken for verified.
            self.jobs_generation.update(|n| *n = n.wrapping_add(1));
            return;
        }
        self.append_output(vec![format!(
            "The {} for {} matches the published SHA256SUMS",
            run.target.noun(),
            run.tag
        )]);
        match run.target {
            crate::update::Target::Update => {
                // Staged, not applied: the archive sits under the data
                // directory exactly where the notice says, and installing it
                // is the same act installing this one was.
                self.notify(&format!(
                    "the update is downloaded and verified — {} — install it as this one was \
                     installed, then restart",
                    run.archive.display()
                ));
                self.append_output(vec![format!(
                    "Staged at {} — install it as this one was installed, then restart",
                    run.archive.display()
                )]);
                self.show_output();
            }
            crate::update::Target::Sdk => {
                // The manifest verify: the unpacked bundle answers through
                // the same `sdk::Manifest` read + verify the full bundle
                // ships with, fed the rustc version the probe just captured.
                match crate::update::verify_sdk(&run.destination, captured) {
                    Ok(()) => {
                        self.notify(
                            "the vieww SDK is installed and verified — restart the studio to \
                             compile previews",
                        );
                        self.append_output(vec![format!(
                            "The SDK is unpacked into {} and verified — restart the studio to \
                             compile previews",
                            run.destination.display()
                        )]);
                        self.show_output();
                    }
                    Err(why) => {
                        self.append_output(vec![format!(
                            "The SDK unpacked but did not verify — {why}"
                        )]);
                        self.notify(&format!(
                            "the vieww SDK arrived but did not verify against its manifest — \
                             remove {} and download it again",
                            run.destination.display()
                        ));
                        self.show_output();
                    }
                }
            }
        }
        self.jobs_generation.update(|n| *n = n.wrapping_add(1));
    }
}
