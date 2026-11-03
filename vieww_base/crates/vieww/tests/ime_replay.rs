//! FW-11: input-method composition, replayed as event sequences through the
//! whole stack — the automated half of the IME proof.
//!
//! ```console
//! cargo test -p vieww --test ime_replay
//! ```
//!
//! # What this pins, and why it could not exist before
//!
//! `tap_to_caret.rs` is this file's predecessor, and its own doc says the
//! fourth clause of the roadmap's exit test — "successfully compose an IME
//! input (e.g. Japanese or emoji picker) on-device" — "is still not met and
//! still cannot be": an input method is an OS service, and no window meant
//! no events. What existed was the model (`TextEditingValue::composing`)
//! with nothing driving it but hand-written unit calls.
//!
//! This file drives the model the way the OS will: whole `ImeEvent`
//! sequences, through `FrameDriver::handle_ime` — the same entry point a
//! platform bridge calls — into a real mounted `TextField`, with the value
//! held in a signal above it and rebuilt between events exactly as a
//! controlled component is. Three languages, because the three failure
//! modes differ:
//!
//! * **Japanese romaji→kanji** — the full preedit lifecycle: provisional
//!   romaji replaced by kana, kana replaced by the converted candidate,
//!   then committed. The failure mode is a bridge that *appends*: `seka`
//!   followed by `sekai` must be `sekai`, not `sekasekai`, and the
//!   conversion must replace the composition, not push it left.
//! * **Korean double compose** — Hangul assembles a syllable from jamo
//!   live in the composing region (`ㅇ` → `아` → `안`), so the *byte
//!   length of the document must not grow* while one syllable is being
//!   built, and two consecutive syllables must compose in separate
//!   regions rather than merging. This is the sharpest append-vs-replace
//!   test there is: every step is the same field width in bytes.
//! * **Chinese pinyin** — a long romaji preedit, then a candidate
//!   selection whose cursor *spans* the converted text, then the commit.
//!   The failure mode unique to Chinese: the preedit is Latin (narrow
//!   glyphs) and the commit is CJK (wide), so any code that cached
//!   geometry across the conversion puts the caret in the wrong place.
//!
//! Each sequence also asserts the candidate-window rect —
//! `FrameDriver::ime_cursor_area`, the API a platform places the IME's
//! candidate window from — is present and *moves with the caret* as the
//! composition grows, which is the assertion that needs a real layout and
//! real fonts rather than a value's byte offsets.
//!
//! # What is honestly still not covered here
//!
//! No real input method runs in this file. The sequences are faithful
//! transcriptions of what MS-IME, the Android Gboard Korean keyboard and
//! iOS pinyin *send* (preedit-per-keystroke, conversion, commit), but the
//! OS-side of the contract — that the bridge translates `GTK_IM_MODULE`
//! events, Android `InputConnection` batches and iOS `UITextInput`
//! mutations into exactly these `ImeEvent`s — is proven only on devices:
//! `ci/mobile/device-suite.sh`'s IME step and the iOS XCUITest leg. The
//! Korean glyphs and the Japanese kana have no face in the embedded test
//! fonts (the CJK subset is Han-only), so those stages assert strings and
//! ranges while the metric assertions sit on scripts the fonts do cover —
//! see `make_test_fonts.py`'s vocabulary. A headless fcitx5 leg against
//! this same `ImeEvent` surface is the remaining optional proof (FW-11d).

use std::rc::Rc;
use std::time::Duration;

use vieww::foundation::{Offset, PointerEvent, PointerId, Size, TextRange, TextSelection};
use vieww::prelude::*;
use vieww::{FrameDriver, ImeEvent, TextEditingValue};

const SURFACE: f32 = 400.0;
const FONT: f32 = 20.0;
const POINTER: PointerId = PointerId(1);

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

/// The field under test, reading its value from the signal above it —
/// `tap_to_caret.rs`'s shape, because it is the same controlled-component
/// contract a real application's state holder satisfies. The `SizedBox`
/// is `tap_to_caret.rs`'s too: an empty field's intrinsic width is zero,
/// and a zero-width field has nowhere inside it to tap for focus.
fn field(value: &Signal<TextEditingValue>) -> WidgetNode {
    let sink = value.clone();
    Center::new()
        .child(
            SizedBox::width(SURFACE).child(TextField::new(value.get()).size(FONT).on_changed(
                Rc::new(move |next| {
                    sink.set(next);
                }),
            )),
        )
        .into()
}

/// A mounted, focused field, and the event pump an IME bridge would be.
struct Harness {
    driver: FrameDriver,
    value: Signal<TextEditingValue>,
}

impl Harness {
    fn new(text: &str) -> Self {
        let mut driver = FrameDriver::new(Size::square(SURFACE));
        let value = driver
            .elements()
            .runtime()
            .signal(TextEditingValue::new(text));
        let mut harness = Self { driver, value };
        harness.rebuild();
        // Focus follows the press: tap where the caret for the end of the
        // text draws — where a user taps to append — which is also the caret
        // an IME opens a session against.
        harness.tap(harness.caret_point(text.len()));
        assert!(
            harness.driver.accepts_text(),
            "a focused field with an on_changed handler accepts text"
        );
        harness
    }

    /// Remount the root from the signal and draw a frame — the rebuild a
    /// keystroke costs in a real app, without which the render object's
    /// copy of the value (and so the caret) is stale.
    fn rebuild(&mut self) {
        let root = field(&self.value);
        self.driver.elements().set_root(root);
        self.driver.draw_frame();
    }

    /// One input-method event, in — and the rebuild it must be followed by.
    ///
    /// Two events in a row without the rebuild between them would apply the
    /// second to the render object's *stale* value: `handle_ime` computes
    /// from `self.value`, which only moves when the widget is rebuilt from
    /// the signal. That is not a wart to route around here; it is the
    /// controlled-component contract, and a bridge that batches events must
    /// apply them to the value it holds, not re-dispatch stale ones.
    fn ime(&mut self, event: &ImeEvent) -> bool {
        let handled = self.driver.handle_ime(event);
        self.rebuild();
        handled
    }

    fn tap(&mut self, at: Offset) {
        self.driver
            .handle_pointer(&PointerEvent::down(POINTER, at, ms(0)));
        self.rebuild();
        self.driver
            .handle_pointer(&PointerEvent::up(POINTER, at, ms(20)));
        self.rebuild();
    }

    fn value(&self) -> TextEditingValue {
        self.value.get()
    }

    /// Where a platform would put the candidate window, in surface
    /// coordinates — `None` when nothing focused accepts text.
    fn candidate_rect(&self) -> Option<vieww::foundation::Rect> {
        self.driver.ime_cursor_area()
    }

    fn field_origin(&self) -> Offset {
        let tree = self.driver.owner().tree();
        let id = tree
            .ids()
            .into_iter()
            .find(|&id| {
                tree.object(id)
                    .is_some_and(|object| object.debug_name() == "RenderEditableText")
            })
            .expect("the field is mounted");
        tree.global_offset(id)
    }

    /// Where the caret for `offset` draws, in surface coordinates —
    /// `tap_to_caret.rs`'s helper, borrowed because focusing by tapping at
    /// a fixed x would leave the caret wherever that x happens to fall.
    fn caret_point(&self, offset: usize) -> Offset {
        let origin = self.field_origin();
        let tree = self.driver.owner().tree();
        let id = tree
            .ids()
            .into_iter()
            .find(|&id| {
                tree.object(id)
                    .is_some_and(|object| object.debug_name() == "RenderEditableText")
            })
            .expect("the field is mounted");
        let object: &dyn std::any::Any = tree.object(id).expect("mounted");
        let editable = object
            .downcast_ref::<vieww::render::RenderEditableText>()
            .expect("its own type");
        let rect = editable
            .shaped()
            .expect("laid out")
            .cursor_rect(vieww::foundation::TextPosition::new(offset));
        Offset::new(
            origin.dx + rect.left,
            origin.dy + (rect.top + rect.bottom) / 2.0,
        )
    }
}

/// `Enabled` and `Disabled` with nothing in flight change nothing, and say
/// so through the `bool` a bridge schedules a frame on.
#[test]
fn session_events_alone_edit_nothing() {
    let mut harness = Harness::new("abc");
    assert!(
        !harness.ime(&ImeEvent::Enabled),
        "a session open is not an edit"
    );
    assert_eq!(harness.value().text, "abc");
    assert_eq!(harness.value().composing, None);
}

#[test]
fn japanese_romaji_to_kanji_through_the_preedit_lifecycle() {
    let mut harness = Harness::new("");

    // Romaji, keystroke by keystroke. Each preedit REPLACES the composing
    // region: the field must hold "sekai", never "sse", never "sekasekai".
    // And the candidate window's anchor — the caret — must move right with
    // every glyph the composition gains, which is the assertion a platform
    // places its candidate window by.
    let origin_dx = harness.field_origin().dx;
    let mut previous_caret: Option<vieww::foundation::Rect> = None;
    for provisional in ["s", "se", "sek", "seka", "sekai"] {
        assert!(
            harness.ime(&ImeEvent::preedit(provisional)),
            "a preedit is an edit"
        );
        let value = harness.value();
        assert_eq!(
            value.text, provisional,
            "the romaji preedit replaces, it does not append"
        );
        assert_eq!(
            value.composing,
            Some(TextRange::new(0, provisional.len())),
            "the composing region is exactly the provisional text"
        );
        let rect = harness
            .candidate_rect()
            .expect("a caret to aim at while composing");
        if let Some(previous) = previous_caret {
            assert!(
                rect.left > previous.left,
                "the candidate rect follows the caret as the composition grows: \
                 {previous:?} then {rect:?}"
            );
        }
        previous_caret = Some(rect);
    }

    // Kana conversion: the whole composition is replaced by its reading.
    // Nine bytes of Hiragana where five bytes of romaji stood. The kana
    // themselves have no face in the embedded fallback (the subset is
    // Han-only — same honesty as the Korean test below), so this stage
    // asserts the value; the metric assertions below sit on the romaji
    // (real Latin glyphs) and the kanji (real full-width ones).
    assert!(harness.ime(&ImeEvent::preedit("せかい")));
    let value = harness.value();
    assert_eq!(value.text, "せかい", "the conversion replaced the romaji");
    assert_eq!(
        value.composing,
        Some(TextRange::new(0, "せかい".len())),
        "the composing region followed the conversion"
    );
    assert!(
        harness.candidate_rect().is_some(),
        "there is a caret to aim the candidate window at while the kana are provisional"
    );

    // Candidate selection: the converted text is the preedit now, with a
    // cursor spanning it — what a candidate window's highlighted row is.
    assert!(harness.ime(&ImeEvent::Preedit {
        text: "世界".to_owned(),
        cursor: Some((0, "世界".len())),
    }));
    let value = harness.value();
    assert_eq!(value.text, "世界", "the candidate replaced the kana");
    assert_eq!(
        value.selection,
        TextSelection::new(0, "世界".len()),
        "the preedit cursor became a selection spanning the candidate"
    );

    // The candidate selection: the composition becomes two kanji that ARE
    // in the embedded fallback face, so the caret must be past two real
    // full-width glyph widths — 2×{FONT}, not two tofu boxes and not the
    // byte count.
    let candidate_caret = harness.candidate_rect().expect("a caret to aim at");
    assert!(
        (candidate_caret.left - origin_dx) >= 2.0 * FONT,
        "two full-width kanji glyphs advance the caret by at least 2×{FONT}: \
         advanced by {}",
        candidate_caret.left - origin_dx
    );

    // Commit: provisional becomes real, the underline goes, and the caret
    // stays exactly where the preedit ended — the composition becomes real
    // *in place*, which is why committing must not move it.
    assert!(harness.ime(&ImeEvent::commit("世界")));
    let value = harness.value();
    assert_eq!(value.text, "世界", "the commit is what the field keeps");
    assert_eq!(value.composing, None, "nothing is provisional any more");
    assert_eq!(
        value.selection,
        TextSelection::collapsed("世界".len()),
        "the caret landed after the committed text"
    );

    let after_commit = harness.candidate_rect().expect("a caret to aim at");
    assert_eq!(
        after_commit.left, candidate_caret.left,
        "committing the composition leaves the caret where the preedit ended: \
         {candidate_caret:?} then {after_commit:?}"
    );
}

#[test]
fn korean_double_compose_builds_syllables_without_growing_the_document() {
    let mut harness = Harness::new("");

    // One syllable assembled live in the composing region: ㅇ (the ieung
    // jamo) becomes 아 becomes 안. Each step is 3 bytes; the document must
    // stay 3 bytes throughout — an append-anywhere bridge fails on the
    // very first replacement.
    for (provisional, label) in [
        ("ㅇ", "the jamo"),
        ("아", "the vowel"),
        ("안", "the closed"),
    ] {
        harness.ime(&ImeEvent::preedit(provisional));
        let value = harness.value();
        assert_eq!(
            value.text, provisional,
            "{label} replaced what was being composed"
        );
        assert_eq!(
            value.text.len(),
            "안".len(),
            "the document did not grow while one syllable was being built"
        );
        assert_eq!(
            value.composing,
            Some(TextRange::new(0, provisional.len())),
            "the composing region is the syllable under construction"
        );
    }

    // Syllable one commits; the keyboard immediately begins syllable two.
    // This is the double in double-compose: a second region opens right
    // after the first becomes real, and the two must not merge.
    assert!(harness.ime(&ImeEvent::commit("안")));
    let first = harness.value();
    assert_eq!(first.text, "안");
    assert_eq!(first.composing, None);

    for provisional in ["ㄴ", "녀", "녕"] {
        harness.ime(&ImeEvent::preedit(provisional));
        let value = harness.value();
        assert_eq!(
            value.text.len(),
            "안녕".len(),
            "the second syllable grew the document by its own three bytes, no more"
        );
        assert_eq!(
            value.composing,
            Some(TextRange::new("안".len(), "안".len() + provisional.len())),
            "the second composing region starts after the committed syllable — the two \
             never merge"
        );
    }

    assert!(harness.ime(&ImeEvent::commit("녕")));
    let value = harness.value();
    assert_eq!(value.text, "안녕", "안녕하세요 is two keystrokes away");
    assert_eq!(value.composing, None);
    assert_eq!(
        value.selection,
        TextSelection::collapsed("안녕".len()),
        "the caret sits after the word, ready for 하세요"
    );
    // Hangul has no face in the embedded test fonts, so this test asserts
    // the value model, not the caret rect — see the module doc.
}

#[test]
fn chinese_pinyin_selects_a_candidate_and_moves_the_caret() {
    let mut harness = Harness::new("");

    // Pinyin, assembled one letter at a time exactly as Gboard sends it.
    for provisional in ["z", "zh", "zho", "zhon", "zhong", "zhongw", "zhongwen"] {
        harness.ime(&ImeEvent::preedit(provisional));
        assert_eq!(harness.value().text, provisional);
    }
    let latin_caret = harness.candidate_rect().expect("a caret to aim at");

    // The candidate row: the converted text arrives as a preedit whose
    // cursor SPANS it — the IME's own highlight, which the field must
    // adopt as its selection while the text is still provisional.
    assert!(harness.ime(&ImeEvent::Preedit {
        text: "中文".to_owned(),
        cursor: Some((0, "中文".len())),
    }));
    let converted = harness.value();
    assert_eq!(converted.text, "中文");
    assert_eq!(
        converted.selection,
        TextSelection::new(0, "中文".len()),
        "the candidate highlight is a selection over the provisional text"
    );

    // Seven narrow Latin glyphs became two wide CJK ones: the caret must
    // have moved to wherever the *converted* text actually ends, which is
    // the assertion that catches a caret placed from cached Latin metrics.
    let converted_caret = harness.candidate_rect().expect("a caret to aim at");
    assert!(
        converted_caret.left != latin_caret.left,
        "the caret moved when the preedit's glyph widths changed: {latin_caret:?} then \
         {converted_caret:?}"
    );

    assert!(harness.ime(&ImeEvent::commit("中文")));
    let value = harness.value();
    assert_eq!(value.text, "中文");
    assert_eq!(value.composing, None);

    // And a second word, composed the same way after the first: the caret
    // for the new composition must start past the committed one.
    for provisional in ["w", "we", "wen"] {
        harness.ime(&ImeEvent::preedit(provisional));
        let value = harness.value();
        assert_eq!(
            value.composing,
            Some(TextRange::new(
                "中文".len(),
                "中文".len() + provisional.len()
            )),
            "the second composition opens after the committed word"
        );
    }
    let second_preedit = harness.candidate_rect().expect("a caret to aim at");
    assert!(
        second_preedit.left > converted_caret.left,
        "the second composition's caret is past the first word: {converted_caret:?} then \
         {second_preedit:?}"
    );
    assert!(harness.ime(&ImeEvent::commit("文")));
    assert_eq!(harness.value().text, "中文文");
    assert_eq!(harness.value().composing, None);
}

#[test]
fn an_escaped_composition_leaves_the_document_untouched() {
    let mut harness = Harness::new("abc");
    let before = harness.value();

    // Typing towards 中文, then Escape: the IME signals the abandonment
    // with an EMPTY preedit — "there is no composition" — which must not
    // be read as "delete the selection".
    harness.ime(&ImeEvent::preedit("zhong"));
    assert_eq!(harness.value().text, "abczhong");
    harness.ime(&ImeEvent::Preedit {
        text: String::new(),
        cursor: None,
    });
    let after = harness.value();
    assert_eq!(after.text, "abc", "the abandoned composition is gone");
    assert_eq!(after.composing, None);
    assert_eq!(
        after.selection, before.selection,
        "and the user's caret is where it was"
    );
}

#[test]
fn a_disabled_session_keeps_the_provisional_text() {
    let mut harness = Harness::new("abc");

    harness.ime(&ImeEvent::preedit("ni"));
    assert_eq!(harness.value().text, "abcni");
    assert_eq!(harness.value().composing, Some(TextRange::new(3, 5)));

    // The keyboard closing mid-composition (focus stolen, app switched):
    // the half-typed text STAYS — the user typed it and can see it — but
    // it is nobody's to replace any more. `apply_ime`'s own contract.
    assert!(
        harness.ime(&ImeEvent::Disabled),
        "abandoning a composition is a change"
    );
    let value = harness.value();
    assert_eq!(value.text, "abcni", "the provisional text stays");
    assert_eq!(
        value.composing, None,
        "but it is no longer a composition — the next keystroke inserts"
    );
}

#[test]
fn a_commit_with_no_composition_is_a_plain_insert() {
    // How a platform delivers ordinary typing when an input method is
    // active but not composing: a commit that lands on the caret.
    let mut harness = Harness::new("abc");
    assert!(harness.ime(&ImeEvent::commit("def")));
    let value = harness.value();
    assert_eq!(value.text, "abcdef");
    assert_eq!(value.composing, None);
    assert_eq!(value.selection, TextSelection::collapsed(6));

    // An empty commit means "nothing to commit", not "delete the
    // selection" — the guard both set_preedit and commit carry.
    assert!(!harness.ime(&ImeEvent::commit("")));
    assert_eq!(harness.value().text, "abcdef");
}

#[test]
fn an_unfocused_field_hears_nothing() {
    // dispatch_ime bubbles from the FOCUSED object; with nothing focused
    // there is nowhere for an input method to go, and the driver says so
    // rather than dropping the event silently into the tree.
    let mut driver = FrameDriver::new(Size::square(SURFACE));
    let value = driver
        .elements()
        .runtime()
        .signal(TextEditingValue::new(""));
    let sink = value.clone();
    driver.elements().set_root(
        Center::new().child(TextField::new(value.get()).size(FONT).on_changed(Rc::new(
            move |next| {
                sink.set(next);
            },
        ))),
    );
    driver.draw_frame();

    assert!(
        !driver.accepts_text(),
        "an unfocused field does not ask for a keyboard"
    );
    assert!(
        !driver.handle_ime(&ImeEvent::commit("hello")),
        "nothing is focused, so nothing handled the event"
    );
    assert_eq!(value.get().text, "", "and the field never heard it");
    assert!(
        driver.ime_cursor_area().is_none(),
        "there is no candidate window without a caret"
    );
}
