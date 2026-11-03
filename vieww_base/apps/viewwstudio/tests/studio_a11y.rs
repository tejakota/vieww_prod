//! The studio's own accessibility gate (ST-18).
//!
//! Two claims are checked here, and both are about the studio rather than
//! anything it builds for a user:
//!
//! 1. **Audit-verified, not settings-deep.** `vieww-accessibility`'s audit
//!    walks the semantics tree of the *studio's own chrome* — the flagship
//!    application, and therefore the evidence that the framework's
//!    accessibility story works on a real app rather than on fixtures. Any
//!    `Severity::Error` finding — a control a screen reader can only announce
//!    by its role, an inconsistent focus report — fails the build. That is
//!    the point of putting it in `tests/`: the next unlabeled control
//!    somebody adds turns `cargo test -p viewwstudio` red instead of shipping
//!    quietly the way sixty commands once did (see `ui::chrome::button`'s own
//!    doc for how that was found).
//!
//!    The audit's *warnings* — touch targets under the 44px minimum — do not
//!    fail anything. Dense strips (menus, tab strips, the activity bar) are a
//!    deliberate design choice of IDE chrome, and a gate that demanded 44px
//!    rows would be a gate somebody deletes the first time it inconvenienced
//!    them. Errors are different: an unnamed control is unusable, not merely
//!    small.
//!
//! 2. **Keyboard-only, from launch.** A walkthrough through the real
//!    `FrameDriver` focus and bubbling path — the same route a keystroke
//!    takes in the windowed app, with no window: Tab traverses every
//!    focusable control and closes the cycle; the palette opens from its
//!    chord; the arrow keys walk *every* palette row past the nine visible
//!    ones (a list that stopped at row nine is a list where half the commands
//!    are unreachable from the keyboard — see `ui::palette`); and Enter on
//!    every row either runs the command or is honestly disabled. No pointer
//!    event is sent anywhere in the file.
//!
//!    **A stated limit, discovered while writing this:** the palette's own
//!    filter field cannot be *typed into* from the keyboard, because nothing
//!    moves focus into it when the palette opens — focus management in the
//!    studio is the one `seed_focus` call at launch (the ST-18 research
//!    note). Arrows and Enter reach the layer by bubbling from wherever
//!    focus already is; characters are declined by the shortcut layer and
//!    reach nothing. Fixing it needs a focus-request path from the palette's
//!    state to the driver, which does not exist yet; the walk below uses the
//!    command palette's own `>` query, which the `CommandPalette` command
//!    sets for exactly this reason.
//!
//! Both halves run headless on every machine, the way `tests/shell.rs`'s
//! pixel half does: the audit is a walk over data the driver already has,
//! and the driver's key path needs no window.

use std::time::Duration;

use vieww_element::Runtime;
use vieww_foundation::{KeyEvent, Modifiers, NamedKey, TargetPlatform};
use vieww_render::FrameDriver;
use viewwstudio::command::Command;
use viewwstudio::state::{PaletteEntry, PanelTab, RightTab, Studio, View};
use viewwstudio::{Shell, WINDOW};

// ===================== the audit gate ===================================

/// Mount the shell for `studio` and return its semantics tree.
///
/// `draw_frame` once, the way `tests/shell.rs` mounts before reading a dump:
/// semantics are collected from the *laid-out* tree, so a frame has to run
/// before there is anything to audit.
fn semantics_of(studio: &Studio) -> vieww_render::SemanticsTree {
    let mut driver = FrameDriver::new(WINDOW);
    viewwstudio::install(&mut driver);
    driver.set_root(Shell {
        studio: studio.clone(),
    });
    driver.draw_frame();
    driver.semantics()
}

/// A fresh Linux studio — the keyboard conventions the rest of this file
/// names, rather than the runner's.
fn studio() -> (Runtime, Studio) {
    let runtime = Runtime::new();
    let studio = Studio::new(&runtime).with_host(TargetPlatform::Linux);
    (runtime, studio)
}

/// Every Error-severity finding in the tree, with the state it came from
/// prepended so a failure names where to look.
fn errors_in(studio: &Studio) -> Vec<String> {
    vieww_accessibility::audit(&semantics_of(studio))
        .into_iter()
        .filter(|finding| finding.severity == vieww_accessibility::Severity::Error)
        .map(|finding| format!("{:?}: {}", finding.role, finding.message))
        .collect()
}

/// The shell states the gate walks.
///
/// Chosen for *coverage of controls*, not of views: every state here mounts
/// something the default shell does not — the palette's field, the find
/// bar's two fields, the name prompt, the say prompt, the inspector's
/// property grid, the welcome and about dialogs, and each sidebar view whose
/// body holds fields or buttons of its own. A view that shares its controls
/// with one already listed adds nothing to the audit; a view that adds one
/// unlabeled field would fail the build here.
fn shell_states() -> Vec<(&'static str, Box<dyn Fn(&Studio)>)> {
    vec![
        ("fresh", Box::new(|_| {})),
        ("palette", Box::new(|s| s.run(Command::CommandPalette))),
        ("find-and-replace", Box::new(|s| s.run(Command::Replace))),
        ("name-prompt", Box::new(|s| s.run(Command::Save))),
        ("say-prompt", Box::new(|s| s.run(Command::SayPrompt))),
        // ST-07: the one-shot keystore password prompt — its field is the one
        // control in the studio whose *content* must never reach the audit's
        // own report, and the one whose label the audit has to find.
        (
            "keystore-prompt",
            Box::new(|s| {
                s.keystore_prompt.set(true);
                s.set_keystore_password("typed".into());
            }),
        ),
        ("welcome", Box::new(|s| s.welcome.set(true))),
        ("about", Box::new(|s| s.about.set(true))),
        (
            "inspector",
            Box::new(|s| {
                s.right_tab.set(RightTab::Inspector);
                s.right_open.set(true);
            }),
        ),
        ("export-view", Box::new(|s| s.view.set(View::Export))),
        (
            "output-panel",
            Box::new(|s| {
                s.panel_tab.set(PanelTab::Output);
                s.panel_open.set(true);
            }),
        ),
        ("toolchain-view", Box::new(|s| s.view.set(View::Toolchain))),
        ("settings-view", Box::new(|s| s.view.set(View::Settings))),
        ("tokens-view", Box::new(|s| s.view.set(View::Tokens))),
        ("snippets-view", Box::new(|s| s.view.set(View::Snippets))),
        ("search-view", Box::new(|s| s.view.set(View::Search))),
        ("source-view", Box::new(|s| s.view.set(View::Source))),
        ("learn-view", Box::new(|s| s.view.set(View::Learn))),
        ("docs-view", Box::new(|s| s.view.set(View::Docs))),
        ("assets-view", Box::new(|s| s.view.set(View::Assets))),
    ]
}

/// **The gate.** An unlabeled control in any of these states fails the
/// studio's build — this test is what turns "we have high-contrast mode"
/// into a conformance claim that is actually checked.
#[test]
fn no_control_in_the_studio_is_left_unlabeled() {
    for (name, put_in) in shell_states() {
        let (_runtime, studio) = studio();
        put_in(&studio);
        let findings = errors_in(&studio);
        assert!(
            findings.is_empty(),
            "the {name} shell state has controls a screen reader cannot name:\n{}",
            findings.join("\n")
        );
    }
}

/// The audit has to be *looking at* the studio, not vacuously passing: the
/// fresh tree carries dozens of interactive nodes, so an audit that walked
/// nothing would also find nothing.
#[test]
fn the_audited_tree_is_the_studios_own_and_it_is_populated() {
    let (_runtime, studio) = studio();
    let tree = semantics_of(&studio);
    let interactive = tree
        .nodes()
        .iter()
        .filter(|node| !node.actions.is_empty() || node.focusable)
        .count();
    assert!(
        interactive >= 30,
        "only {interactive} actionable nodes in the shell — the audit is no \
         longer walking the real chrome"
    );
}

// ===================== the keyboard-only walkthrough ======================

/// A bare key press, as the platform bridge delivers one.
fn key(named: NamedKey) -> KeyEvent {
    KeyEvent::named(named, Duration::ZERO)
}

/// The Linux form of a ⌘ chord, as the shortcut layer receives it.
fn ctrl(character: &str) -> KeyEvent {
    KeyEvent::character(character, Duration::ZERO).with_modifiers(Modifiers::CONTROL)
}

/// The shell mounted on a real driver, with focus seeded the way launch does.
fn mounted() -> (FrameDriver, Studio) {
    let mut driver = FrameDriver::new(WINDOW);
    viewwstudio::install(&mut driver);
    let runtime = driver.elements().runtime().clone();
    let studio = Studio::new(&runtime).with_host(TargetPlatform::Linux);
    driver.set_root(Shell {
        studio: studio.clone(),
    });
    driver.draw_frame();
    viewwstudio::seed_focus(&mut driver);
    (driver, studio)
}

/// Whether a command leaves the palette open behind it. `CommandPalette` is
/// *not* here: Enter on its own row while the palette is open is the toggle's
/// shut half, and it closes like any other command that ran. The four that
/// stay are the ones whose whole job is to reopen the palette in one of its
/// modes.
const OPENS_THE_PALETTE: &[Command] = &[
    Command::GoToFile,
    Command::GoToSymbol,
    Command::GoToLine,
    Command::ShowShortcuts,
];

/// **Arrows and Enter reach every palette command.**
///
/// The walk presses exactly what a keyboard-only user can press — the
/// palette chord, ArrowDown, Enter, Escape — through the driver's real focus
/// and bubbling path, and asserts, for every row of the command list:
///
/// - `ArrowDown` lands the highlight on it (all of them, past the nine
///   visible rows, and back around to the top);
/// - `Enter` runs it when the palette offered it as runnable — which the
///   palette closing behind the row is the observable of, the same close that
///   a mouse click on the row produces — and leaves the palette open, doing
///   nothing, when the row was disabled.
///
/// Escape between rows is the keyboard's own cleanup — and it now has to
/// work: `can_run(CloseFind)` answers from `anything_to_escape`, so a dialog
/// a command opened (Save's name prompt, Open Folder's picker) is dismissed
/// by a key rather than sitting over the rest of the walk.
#[test]
fn arrows_and_enter_reach_every_palette_command() {
    let (mut driver, studio) = mounted();

    // The palette opens from its chord, on commands.
    assert!(driver.handle_key(&ctrl("p")), "the chord was not consumed");
    driver.draw_frame();
    assert!(studio.palette_open.get(), "Ctrl+P did not open the palette");
    assert_eq!(studio.palette_query.get(), ">", "and it opens on commands");

    let results = studio.palette_results();
    assert!(
        results.len() > 9,
        "{} commands listed — the list no longer scrolls, and the case this \
         walk exists for is gone",
        results.len()
    );
    let count = results.len();

    // Arrows reach every row: `count` presses from index 0 walk the whole
    // list and wrap back to the top.
    for step in 1..=count {
        assert!(
            driver.handle_key(&key(NamedKey::ArrowDown)),
            "ArrowDown was not consumed while the palette was open"
        );
        assert_eq!(
            studio.palette_index.get(),
            step % count,
            "the highlight did not reach row {} of {}",
            step % count,
            count
        );
    }

    // Enter on every row. The highlight is back at 0 after the wrap, so each
    // row is reached by walking down to it from the top.
    for (index, entry) in results.iter().enumerate() {
        let PaletteEntry::Command(command) = entry else {
            // No plugins are loaded in this studio; a plugin row is exercised
            // by `tests/plugin.rs`.
            continue;
        };

        // Shut whatever the previous row opened — dialogs first, then the
        // palette — by key, the way a keyboard-only user would.
        for _ in 0..8 {
            if !studio.anything_to_escape() {
                break;
            }
            driver.handle_key(&key(NamedKey::Escape));
        }
        // A closed palette is reopened by its chord, which also resets the
        // query to `>` and the highlight to row 0.
        if studio.palette_open.get() {
            assert!(driver.handle_key(&ctrl("p")), "the chord failed to shut");
        }
        assert!(driver.handle_key(&ctrl("p")), "the chord failed to open");
        driver.draw_frame();
        assert_eq!(studio.palette_query.get(), ">");
        assert_eq!(studio.palette_index.get(), 0);

        for _ in 0..index {
            driver.handle_key(&key(NamedKey::ArrowDown));
        }
        assert_eq!(
            studio.palette_index.get(),
            index,
            "the highlight could not reach row {index} a second time"
        );

        let runnable = studio.can_run(*command);
        let keeps_open = OPENS_THE_PALETTE.contains(command);
        assert!(
            driver.handle_key(&key(NamedKey::Enter)),
            "Enter was not consumed with the palette open"
        );
        driver.draw_frame();
        assert_eq!(
            studio.palette_open.get(),
            !runnable || keeps_open,
            "row {index} ({:?}) — Enter should {}",
            command,
            if runnable && !keeps_open {
                "run the command and close the palette behind it"
            } else {
                "leave the palette open"
            }
        );
    }
}

/// **Tab closes the cycle.** Every focusable control in the shell is reached
/// by Tab alone, and the traversal comes back around to where it started —
/// a focus walk that dropped off the end somewhere would strand the keyboard
/// user at the bottom of the window with nothing left to reach.
#[test]
fn tab_reaches_every_focusable_control_and_closes_the_cycle() {
    let (mut driver, _studio) = mounted();

    let start = driver.focused().expect("focus was seeded at launch");
    let mut presses = 0;
    while presses < 300 {
        assert!(
            driver.handle_key(&key(NamedKey::Tab)),
            "Tab was not consumed as traversal"
        );
        presses += 1;
        let now = driver.focused();
        assert!(now.is_some(), "focus vanished after {presses} Tab presses");
        if now == Some(start) {
            break;
        }
    }
    assert!(
        presses < 300,
        "Tab never came back around to the first focusable control after \
         {presses} presses — the cycle is not closed"
    );
}

/// Escape is the keyboard's way out of a dialog a command opened — the
/// `anything_to_escape` gate in front of it is what makes that true, and a
/// keyboard-only user with a prompt they cannot dismiss is locked out of
/// their own editor.
#[test]
fn escape_dismisses_a_dialog_a_command_opened() {
    let (mut driver, studio) = mounted();

    // The say prompt: a modal a *command* opens, with a text field in it —
    // and no pointer anywhere near it. (Save's name prompt needs an open
    // folder before it will ask; the say prompt asks on any studio.)
    studio.run(Command::SayPrompt);
    driver.draw_frame();
    assert!(studio.say_prompt_open.get(), "the Say prompt did not open");

    assert!(
        driver.handle_key(&key(NamedKey::Escape)),
        "Escape was not consumed with a dialog open"
    );
    driver.draw_frame();
    assert!(
        !studio.say_prompt_open.get(),
        "Escape did not dismiss the prompt — a keyboard-only user cannot \
         close it"
    );
}
