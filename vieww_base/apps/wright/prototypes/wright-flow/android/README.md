# Android

Built in two steps, which is what vieww Studio's Export view runs for you:

```console
cargo ndk -t arm64-v8a -o target/android/jniLibs build --release
cd android && ./gradlew assembleDebug     # or: gradle assembleDebug
```

The APK lands at `android/app/build/outputs/apk/debug/app-debug.apk`.

## The wrapper

There is no `gradlew` here yet, because it needs a binary JAR that a source
scaffolder should not be writing. Run this once:

```console
cd android && gradle wrapper
```

Until you do, the studio invokes `gradle` directly and everything works the
same; afterwards the build is pinned to the version in
`gradle/wrapper/gradle-wrapper.properties`.

## Before you publish

* `applicationId` is `com.example.wright_flow`. `com.example` is reserved and the Play Store
  refuses it — change it in `app/build.gradle`, and change `namespace` with it.
* The release build type is unsigned on purpose. Add a `signingConfig` once you
  have a keystore somewhere safe.
