# ci/mobile test data

`AndroidManifest.androguard-TestActivity.bin`: the compiled (binary XML)
manifest of `tests/data/APK/TestActivity.apk` in
[androguard](https://github.com/androguard/androguard), Apache License 2.0.
`test_axml.py` edits it the way `apk-add-code.sh --background-audio` edits a
cargo-apk manifest. The edit was checked against androguard's own AXML
decoder (an independent implementation) when it was written; this test keeps
the round trip exact without needing androguard installed.
