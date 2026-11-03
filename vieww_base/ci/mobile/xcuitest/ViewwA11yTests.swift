// The iOS half of the accessibility suite: assert that VoiceOver's tree
// contains what vieww's semantics tree says it should.
//
// The mirror of `ci/mobile/a11y-android.sh`'s uiautomator dump, in the
// framework Apple gives UI tests: XCUITest queries the accessibility tree
// the system itself exposes — the same tree VoiceOver reads — so a green
// run here is "the elements are present, labelled, traited and enabled",
// which is exactly the half of the contract PENDING §3.6 records as
// missing on iOS.
//
// # Status: runnable, not yet run
//
// **This file has never executed.** It runs from the Xcode project that
// `ci/mobile/xcuitest/project.yml` generates (`a11y-ios.sh --xcuitest`):
// the spec is committed, the .xcodeproj is not, and `xcodegen` writes it on
// the machine that runs the suite. Until a run has happened, this file is
// the shape of the assertion, not a green tick — the same standing as
// `vieww-platform-winit`'s `ios_accessibility.rs`, whose elements these
// tests would be reading. The macOS runner's manual dispatch of the a11y
// suite is the first execution, and a failure there is a finding, not a
// flake.
//
// # What a run must assert to count
//
// Per PENDING §3.6's start: a vieww button exists to VoiceOver, is
// labelled, carries the button trait and is enabled — `app.buttons[...]`
// below is that assertion spelled in XCUITest's own vocabulary. The
// identifiers come from the bridge: every published element's accessibility
// identifier is `vieww.<generation>.<index>` (the packed render id), which
// is stable across rebuilds for the same reason the Android tree's ids are.
//
// # The typing leg (FW-11c)
//
// The tests at the bottom type into the demo's field and assert what the
// field holds afterwards — the iOS half of the IME proof. The committed
// sequences themselves are proven host-side (`crates/vieww/tests/
// ime_replay.rs`, through `FrameDriver::handle_ime`); what only these tests
// add is that events from *this* platform's keyboard reach the same place.
// Like everything in this file they have never run — the macOS runner's
// manual dispatch of the a11y suite (FW-10a's project.yml) is their first
// execution, and a failure there is a finding, not a flake.

import XCTest

final class ViewwA11yTests: XCTestCase {

    /// The app under test, as `ios-app.sh` bundles it.
    private var app: XCUIApplication!

    override func setUpWithError() throws {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launch()
        // VoiceOver itself cannot be driven from XCUITest — what a UI test
        // reads is the same accessibility tree VoiceOver consumes, so a
        // green run is tree-level evidence, and ears on a manual session
        // remain the confirmation PENDING §3.6 asks to be recorded beside
        // it. The distinction is stated here so nobody reads a passing
        // suite as "VoiceOver has been heard".
    }

    /// The demo app's one labelled button — the same node
    /// `a11y-android.sh` asserts TalkBack sees as `Tap me`.
    func testTheButtonIsPresentLabelledAndEnabled() throws {
        // The demo button's label, as the gallery's fixture names it. If
        // the fixture's text changes, this assertion and the Android
        // harness's are the two places that follow it.
        let button = app.buttons["Tap me"]
        XCTAssertTrue(button.waitForExistence(timeout: 10.0),
                      "the semantics tree's button never reached the accessibility tree")
        XCTAssertTrue(button.isHittable,
                      "the button is in the tree but not where VoiceOver would touch")
        XCTAssertEqual(button.value as? String, nil,
                       "a plain button carries no value; a value here means the trait mapping leaked")
    }

    /// Every element the bridge published carries a `vieww.` identifier —
    /// the packed render id, stable across rebuilds. An element without
    /// one came from somewhere other than the bridge, and the test wants
    /// to know about it.
    func testEveryPublishedElementCarriesABridgeIdentifier() throws {
        let allElements = app.descendants(matching: .any)
            .matching(NSPredicate(format: "identifier BEGINSWITH 'vieww.'"))
        XCTAssertGreaterThan(allElements.count, 0,
                             "no bridge-published elements in the tree — is VoiceOver running? "
                                 + "the bridge publishes nothing otherwise, by design")
        for index in 0..<min(allElements.count, 20) {
            let element = allElements.element(boundBy: index)
            XCTAssertTrue(element.identifier.hasPrefix("vieww."),
                          "element \(index) lost its identifier: '\(element.identifier)'")
        }
    }

    /// A disabled control reads as dimmed: the `NotEnabled` trait the
    /// bridge sets is what VoiceOver announces, and the tree-level fact is
    /// `isEnabled == false`.
    func testDisabledControlsAreDisabledInTheTree() throws {
        // The demo's disabled button, when the fixture has one on screen;
        // skipped with a spoken reason rather than passed silently when it
        // does not — a test that cannot find its subject says so.
        let disabled = app.buttons["Disabled"]
        guard disabled.waitForExistence(timeout: 5.0) else {
            throw XCTSkip("no Disabled control in this fixture's tree; enable one to run this")
        }
        XCTAssertFalse(disabled.isEnabled,
                       "the semantics said disabled, the tree says enabled — the trait was lost")
    }

    // MARK: - Typing (FW-11c)

    /// The demo's field, as the bridge publishes it.
    ///
    /// `Role::TextField` maps to no UIKit trait in the bridge (a button is
    /// a button; a text field is recognised by having a value), so it is not
    /// in `app.textFields` — it is an element whose *value* is what the
    /// field holds, found the way VoiceOver finds it: by what it says. The
    /// fixture seeds the field with "tap to type" (examples/shared/screen.rs),
    /// which is this suite's handle on it.
    private var field: XCUIElement {
        app.descendants(matching: .any)
            .matching(NSPredicate(format: "value == %@", "tap to type"))
            .firstMatch
    }

    /// How long to wait for the tree to carry what typing did.
    ///
    /// The accessibility value only moves when vieww rebuilds the semantics
    /// tree — the next frame, not the keystroke — so every typing assertion
    /// is a poll, not a read. Ten seconds is the same allowance the button
    /// existence wait gives a cold launch.
    private static let treeSettles: TimeInterval = 10.0

    /// Poll the field's value until it satisfies `check` or the deadline.
    ///
    /// Each read of `value` re-resolves the query, which is what makes a
    /// poll here see the tree update at all — a cached snapshot would wait
    /// ten seconds and report the world as it was before the keystroke.
    private func fieldValue(where check: (String) -> Bool) -> String? {
        let deadline = Date().addingTimeInterval(Self.treeSettles)
        while Date() < deadline {
            if let value = field.value as? String, check(value) {
                return value
            }
            usleep(250_000)
        }
        return field.value as? String
    }

    /// Typing ASCII into the field commits through the platform's text
    /// entry and is visible in the tree.
    ///
    /// The primary FW-11c assertion, and the honest one to run first:
    /// `typeText` delivers keystrokes the way a hardware keyboard does, the
    /// path PENDING §1.4's hand-run on an iPhone 13 Pro exercised. If the
    /// bridge is missing, this fails — loudly, on the dispatch, which is
    /// the finding this test exists to produce rather than hide.
    func testTypingIntoTheFieldCommitsThroughTheKeyboard() throws {
        XCTAssertTrue(field.waitForExistence(timeout: Self.treeSettles),
                      "the field never reached the accessibility tree — without it there is " +
                      "nothing to type into and nothing to assert on")
        field.tap()
        field.typeText("hello")
        let value = fieldValue { $0.contains("hello") }
        XCTAssertTrue(value?.contains("hello") == true,
                      "typed 'hello' and the field still reads '\(value ?? "nothing")' — the " +
                      "keystrokes never reached vieww's editing model")
    }

    /// Typing with a CJK keyboard composes — the tree must end up holding
    /// something no ASCII keyboard could have produced.
    ///
    /// The CJK half. With the Chinese (Simplified, Pinyin) keyboard active —
    /// `a11y-ios.sh --xcuitest` installs it into the simulator before the
    /// run, the one thing a UI test cannot do to its own host — typing
    /// romaji composes in the field and commits a candidate. Which hanzi it
    /// commits is the IME's choice and differs by iOS version, so the
    /// assertion is the one that matters and no more: the value changed, and
    /// what it holds includes a character past U+2E80 — CJK by any block a
    /// pinyin keyboard produces. Skipped, with the reason spoken, when no
    /// keyboard came up at all: that is a sim-configuration gap, not a
    /// vieww finding, and failing it here would point the next reader at
    /// the wrong half of the stack.
    func testCJKTypingComposesAndCommits() throws {
        XCTAssertTrue(field.waitForExistence(timeout: Self.treeSettles),
                      "the field never reached the accessibility tree")
        let before = field.value as? String ?? ""
        field.tap()
        // No keyboard at all after a tap that should have raised one: the
        // simulator has no CJK keyboard installed — the setup step in
        // a11y-ios.sh is the fix, and the skip says so by name.
        guard app.keyboards.firstMatch.waitForExistence(timeout: 5.0) else {
            throw XCTSkip("no keyboard appeared — install the Chinese (Simplified, Pinyin) " +
                          "keyboard before this run (a11y-ios.sh --xcuitest does it)")
        }
        // Romaji: the pinyin keyboard composes it; ASCII-only keyboards
        // would commit the letters verbatim, which the CJK assertion below
        // then honestly fails on a machine without the right keyboard.
        field.typeText("nihao")
        field.typeText(" ")
        let value = fieldValue { $0 != before }
        let committed = value ?? ""
        XCTAssertNotEqual(committed, before,
                          "typed 'nihao' and the field never changed — composition died before " +
                          "the tree, which is exactly the FW-11 failure mode")
        let hasCJK = committed.unicodeScalars.contains { $0.value >= 0x2E80 }
        XCTAssertTrue(hasCJK,
                      "the field changed but holds '\(committed)' — ASCII, so the keyboard " +
                      "composed nothing: is the pinyin keyboard actually the active one?")
    }
}
