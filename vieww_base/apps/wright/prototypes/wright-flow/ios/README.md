# iOS

Built in two steps, which is what vieww Studio's Export view runs for you:

```console
cargo build --release --target aarch64-apple-ios-sim   # or aarch64-apple-ios
cd ios && xcodebuild -scheme wright-flow -sdk iphonesimulator -configuration Release build
```

macOS only. That is Apple's rule rather than a gap in the studio, and the
Export view says so on other hosts instead of offering a build that cannot run.

## How the Rust gets in

The crate builds as a `staticlib` as well as a binary. Xcode links
`libwright_flow.a` out of `../target/$(RUST_TARGET)/release`, where `RUST_TARGET` is
set per SDK in the project's build settings — `aarch64-apple-ios` for a device
and `aarch64-apple-ios-sim` for the simulator. `App/main.m` calls the one symbol
the library exports.

If the linker cannot find `-lwright_flow`, the Rust for *that* SDK's target has not
been built yet; the Export view runs both steps in the right order.

## Before you publish

* `PRODUCT_BUNDLE_IDENTIFIER` is `com.example.wright_flow`, which is a placeholder. Change it in
  the target's build settings.
* Signing is automatic, so Xcode uses whichever identity your Apple ID provides.
  There is no identity in this file and none should be added to it.
