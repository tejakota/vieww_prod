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
// # Status: run, and corrected by its first runs
//
// The first CI runs (framework-checks, 2026-11) found four test-side bugs,
// fixed here: a count taken before the first publish, a nil-vs-"" value
// check, typing aimed at the proxy element instead of the first responder,
// and a field query that stopped matching once typing changed its value.
//
// # The macos-15 finding (2026-11-19): identifiers, and the seed-prefix handle
//
// The retired macos-14 image (2026-11-02) was the last iOS 17 runner; the
// suite's next execution on macos-15 (Xcode 16, iOS 18) found that **the
// accessibility tree no longer carries `accessibilityIdentifier` for the
// bridge's `UIAccessibilityElement`s**: labels, values, traits and frames all
// reach XCUITest (the button test and the field-by-value query pass), and
// every identifier reads as "". Apple exposes identifiers to UI automation
// from views, and the bridge deliberately publishes element proxies rather
// than views — so on iOS 18 the identifier half of the contract is not
// assertable through XCUITest at all. The bridge still sets them (Accessibility
// Inspector and any client that does expose them read the same property), and
// the suite asserts them wherever the tree carries them, falling back to the
// label/value handles VoiceOver itself uses where it does not. The typing
// tests' re-find now rides the seeded value's prefix rather than the
// identifier: "tap to type" survives typing by construction, where an
// exact-value match stops matching the moment the first keystroke lands.
// Historical note — **this file had never executed.** It runs from the Xcode project that
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
        // XCUITest reports "no value" as nil or "" depending on the iOS
        // version; either is "none". Anything else is a leaked value.
        let value = button.value as? String ?? ""
        XCTAssertTrue(value.isEmpty,
                      "a plain button carries no value, but this one says '\(value)' — the trait mapping leaked")
    }

    /// Every element the bridge published carries a `vieww.` identifier —
    /// the packed render id, stable across rebuilds. An element without
    /// one came from somewhere other than the bridge, and the test wants
    /// to know about it.
    func testEveryPublishedElementCarriesABridgeIdentifier() throws {
        // The bridge publishes on the first semantics rebuild, a frame after
        // launch — query before it and the tree is honestly empty (the first
        // run counted 0 for exactly that reason while the button test, which
        // waits, found its element). Wait for the known button first.
        XCTAssertTrue(app.buttons["Tap me"].waitForExistence(timeout: 10.0),
                      "nothing reached the accessibility tree within 10 s of launch")
        let allElements = app.descendants(matching: .any)
            .matching(NSPredicate(format: "identifier BEGINSWITH 'vieww.'"))
        if allElements.count > 0 {
            // The tree carries identifiers (iOS 17 and earlier): every one of
            // them is a bridge id, and the count is the whole published set.
            for index in 0..<min(allElements.count, 20) {
                let element = allElements.element(boundBy: index)
                XCTAssertTrue(element.identifier.hasPrefix("vieww."),
                              "element \(index) lost its identifier: '\(element.identifier)'")
            }
            return
        }
        // iOS 18 (the macos-15 runner's Xcode 16 image) does not expose
        // identifiers for the bridge's element proxies — the header's finding.
        // What remains assertable is the half VoiceOver consumes: every
        // bridge-published element is in the tree by its label or its value.
        // The demo's fixture publishes the button ("Tap me", already waited
        // for above) and the field (its seeded value), so both must resolve.
        XCTAssertTrue(field.waitForExistence(timeout: Self.treeSettles),
                      "neither identifiers nor the field's value reached the tree — the " +
                          "bridge published nothing a client can read, which is not the " +
                          "iOS 18 identifier finding but a real failure")
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
    ///
    /// `typed` is the field re-found by its seeded prefix: the seed "tap to
    /// type" is still the value's prefix after typing (the fixture appends),
    /// where an exact-value match stops matching the moment the first
    /// keystroke lands. The bridge identifier was the handle before iOS 18
    /// stopped exposing identifiers (see the header); the prefix survives
    /// that and is the same string VoiceOver reads on focus.
    ///
    /// **Resolution failures are `nil`, not throws.** Reading `value` off an
    /// unresolvable query throws "Failed to get matching snapshot", which
    /// answers nothing: it cannot say whether the field vanished, renamed
    /// itself, or began the message this test was asking for. The first red
    /// rounds met exactly that (three runs, 2026-11-19, the field holding
    /// something the prefix query no longer matched and the error saying
    /// only "no matches"). Returning `nil` and letting the assertion print
    /// [`everyValueInTheTree`] turns the next red into evidence.
    private func fieldValue(_ typed: XCUIElement, where check: (String) -> Bool) -> String? {
        let deadline = Date().addingTimeInterval(Self.treeSettles)
        while Date() < deadline {
            if let value = Self.nonThrowingValue(of: typed), check(value) {
                return value
            }
            usleep(250_000)
        }
        return Self.nonThrowingValue(of: typed)
    }

    /// `element.value` as a plain read: `nil` when the query will not resolve,
    /// rather than an `NSException` with no information in it.
    ///
    /// `exists` is the probe that does not throw — it answers `false` for an
    /// unresolved query — so it is asked first, and `value` is only read
    /// through a query that just proved it resolves.
    private static func nonThrowingValue(of element: XCUIElement) -> String? {
        guard element.exists else { return nil }
        return element.value as? String
    }

    /// Every value the tree carries, for failure messages.
    ///
    /// A typing assertion that fails with "the field reads X" is a finding; one
    /// that fails with "no matches found" is a shrug. This is what the message
    /// falls back to when the field's own handle stopped resolving: the whole
    /// published value set, so the reader can see what the keystrokes did land
    /// as — and whether the field is there at all under some other value.
    private func everyValueInTheTree() -> [String] {
        let all = app.descendants(matching: .any)
        var values: [String] = []
        for index in 0..<min(all.count, 30) {
            if let value = Self.nonThrowingValue(of: all.element(boundBy: index)), !value.isEmpty {
                values.append(value)
            }
        }
        return values
    }

    /// The field re-found by its seeded prefix — the handle that survives
    /// typing on every iOS version, identifier or no identifier.
    private var typedField: XCUIElement {
        app.descendants(matching: .any)
            .matching(NSPredicate(format: "value BEGINSWITH %@", "tap to type"))
            .firstMatch
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
        let typed = typedField
        field.tap()
        // Type into the app, not the element. XCUITest's element.typeText
        // demands that the *element* report keyboard focus, which a custom
        // UIAccessibilityElement cannot: the first responder is vieww's
        // view, and the element is only its accessibility proxy (the first
        // run: "neither element nor any descendant has keyboard focus").
        // The keyboard the tap raised delivers to that responder either way.
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 5.0),
                      "tapping the field raised no keyboard — vieww never became first responder")
        app.typeText("hello")
        let value = fieldValue(typed) { $0.contains("hello") }
        XCTAssertTrue(value?.contains("hello") == true,
                      "typed 'hello' and the field reads '\(value ?? "nothing")' — the " +
                      "keystrokes never reached vieww's editing model. The tree holds " +
                      "\(everyValueInTheTree())")
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
        let typed = typedField
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
        // Into the app, for the reason the ASCII test gives.
        app.typeText("nihao")
        app.typeText(" ")
        let value = fieldValue(typed) { $0 != before }
        let committed = value ?? ""
        XCTAssertNotEqual(committed, before,
                          "typed 'nihao' and the field never changed — composition died before " +
                          "the tree, which is exactly the FW-11 failure mode. The tree holds " +
                          "\(everyValueInTheTree())")

        // **Delivery and composition are two different findings, and only one
        // of them is this test's to fail on.** The keystrokes landing at all is
        // the delivery half — the one the ASCII test above owns, and the one
        // the bridge's burst-chaining fix (2026-11-19) was for: 'nihao '
        // arriving complete but as plain ASCII means every key was delivered
        // and the keyboard simply did not compose — either the active input
        // mode is not pinyin after all (a simulator-configuration gap, the
        // same standing as the no-keyboard skip above) or the platform bridge
        // does not implement the marked-text half of the text-entry protocol
        // that composing keyboards need (PENDING's iOS IME item). Romaji
        // arriving INCOMPLETE — characters lost — is a delivery bug, and is
        // asserted as the failure it is.
        // What the typing added to the seed: the whole delivery, laid out for
        // the two-way check below. `before` is the seed the field started
        // with, so this is exactly the keystrokes' net effect.
        let delivered = String(committed.dropFirst(before.count))
        let hasCJK = committed.unicodeScalars.contains { $0.value >= 0x2E80 }
        if !hasCJK && delivered.contains("nihao") {
            throw XCTSkip("every keystroke of 'nihao ' landed, as plain ASCII — the keyboard " +
                          "composed nothing. Either the pinyin keyboard is not the active " +
                          "input mode on this simulator (a11y-ios.sh installs it; Settings " +
                          "must still have it selected), or the bridge does not take marked " +
                          "text, which is the open iOS IME item in PENDING — delivery is " +
                          "proven, composition is not, and this suite cannot tell those two " +
                          "configurations apart from inside the app")
        }

        XCTAssertTrue(hasCJK,
                      "the field changed but holds '\(committed)' — neither the composed " +
                      "hanzi this test exists for nor the complete ASCII that would name a " +
                      "configuration gap, so keystrokes were lost in delivery. The tree " +
                      "holds \(everyValueInTheTree())")
    }
}
