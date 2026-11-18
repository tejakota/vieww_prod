# Vieww on Firebase Test Lab (free)

Market Watch runs as a Firebase Test Lab **Game Loop** test: Test Lab
launches the app with `com.google.intent.action.TEST_LOOP`. Scenario 1 plays
the schedule on the CPU rasterizer, scenario 2 on the GPU path. Both include
the 60 s thermal pass. The app finishes itself when the schedule is done, and
narrates what Test Lab has no shell to measure: CPU time, RSS and battery
temperature per phase, PSS after the run, and process start to first frame.
`to_farm.py` turns each phone's device log into the layout
`compare_report.py` reads, so the report looks like the AWS one.

What it measures and what it does not:

- **Vieww only.** Valdi is pinned, so its numbers stand from the last full
  AWS report.
- **No compositor counts.** Test Lab has no SurfaceFlinger timestats, so
  presented fps and jank come from Vieww's own frame log. The report says so
  on each phone.
- **The frame-rate log.** Each phone shows what Vieww asked Android for, and
  any 120 ↔ 60 Hz change with the evidence behind it.

## Why it costs nothing

- **The Spark plan cannot be billed.** It has no payment method, and allows 5
  physical-device test runs a day. This job uses up to 3, one per phone.
- **The job refuses a billable project.** It checks the project's billing
  state first and stops if billing is on (the Blaze plan), unless the
  repository variable `FIREBASE_ALLOW_BLAZE=yes` says that was meant. Blaze
  includes 30 free physical-device minutes a day, then charges for more.
- **Each phone is capped.** Every phone stops after 6 minutes, whatever
  happens.

## One-time setup (about 10 minutes)

1. **Create the project.** In the [Firebase console](https://console.firebase.google.com),
   create a project and leave it on the **Spark** plan. Do not add billing.
   Note its project ID.
2. **Create a service account.** In the Google Cloud console for that
   project, go to IAM & Admin → Service accounts → Create. Give it these
   roles:
   - **Firebase Test Lab Admin**, which runs tests.
   - **Storage Object Viewer**, which reads the results.
   - **Project Viewer**, which lets it read the project's settings and
     catalog.
   Then open the account, go to Keys → Add key → JSON, and download the file.
3. **Add the GitHub secrets.** Under Settings → Secrets and variables →
   Actions → New repository secret, add:
   - `FIREBASE_PROJECT`: the project ID.
   - `FIREBASE_SA_KEY`: the whole JSON key file's contents.
4. **Choose phones (optional).** Add the repository *variable*
   `FIREBASE_DEVICES` to pick phones yourself, e.g. `shiba:34,e3q:34`. List
   the catalog with `gcloud firebase test android models list`. Without it,
   the job picks up to three physical phones, 90/120 Hz panels first
   (`pick_devices.py`).

## Running it

Actions → **2 · Devices** → Run workflow, with **Phone farm** set to
**"Vieww on Firebase Test Lab (free)"**. It runs from `main`. The report goes
to the run's summary and to `ci-runs/compare-main`.

## Trying the launch on your own phone

The same launch Test Lab makes, over USB:

```console
adb shell am start -W -a com.google.intent.action.TEST_LOOP \
  -t application/javascript --ei scenario 2 \
  -n dev.vieww.marketwatch/android.app.NativeActivity
adb logcat -s vieww | grep __MW__
```

The app should play the schedule on the GPU path (`"path":"gpu"`) and close
itself after the `done` line.
