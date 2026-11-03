//! ST-05: the component system — extract a subtree, reuse it anywhere.
//!
//! # Why this module exists
//!
//! Say screens accrete: a card that looked right in one place gets copied,
//! by hand, into the next three. The copy is text, so it drifts — the
//! original gains a tweak, the copies do not — and nothing in the studio
//! knew a subtree was *the same thing* twice. A component system is the
//! fix, and its honest smallest shape has exactly two verbs:
//!
//! * **Extract**: the picked subtree's source span becomes
//!   `pub struct <Name>` + `impl Widget` in `src/widgets/<name>.rs`, and the
//!   span in the `.say` buffer becomes the one-word reference `<Name>`.
//!   Say's own compiler does the translation ([`vieww_say_codegen::compile_component`]),
//!   which also states what a component may not contain: a pure subtree, no
//!   state, no events, no scrolling — the things that belong to the screen.
//! * **Scaffold**: a component in the local registry is written into another
//!   project and referenced the same way — the registry is how a subtree
//!   outlives the screen it came from.
//!
//! # The preview, and why extraction ends at "Build once"
//!
//! The generated Rust for `<Name>` is `crate::widgets::<name>::<Name>`. In a
//! built project that is the project's own module; in the studio's preview
//! the screen is compiled *outside* the project, so the studio links the
//! project's rlib and rewrites that one path (see `compile::with_say_project`).
//! Until the project has been built there is nothing to link, and the
//! existing `project_lib_refusal` says so with its own instructions — the
//! command's job ends at writing correct files, not at building them.
//!
//! # The registry, and what it is not
//!
//! `<data_dir>/components/<Name>/component.say` — the extracted subtree as
//! Say, beside the snippets file, on the user's own disk. It is surfaced in
//! the Snippets view. It is **not** a package index: there is no versioning,
//! no dependency resolution and no network, and pretending any of those
//! would be the studio shipping a registry it cannot stand behind.
//!
//! # Honest limits
//!
//! * Only a picked *widget* line can head an extraction — a `for each` body
//!   reads state and is refused by the same purity rule.
//! * The component is written for the project that extracted it; scaffolding
//!   into a project without `vieww` (or with a different asset layout) is
//!   refused rather than best-efforted.
//! * Extracting the same subtree twice makes two components; nothing merges
//!   them, because two things that are equal today are not one thing.

use std::io;
use std::path::{Path, PathBuf};

use vieww_say_codegen::line::WidgetLine;

use crate::state::Studio;

/// Where the studio's component registry lives, under the data directory the
/// studio already keeps `snippets.txt` and `recovery/` in.
#[must_use]
pub fn registry_dir() -> PathBuf {
    crate::about::data_dir().join("components")
}

/// The one path a generated Say screen reaches outside its own file with:
/// `crate::widgets::`, emitted by a `<Name>` component reference and by
/// nothing else the generator writes.
///
/// `Studio::render_preview` gates the project-library lookup — and its
/// refusal — on this string rather than on `needs_project`'s broader
/// `crate::`/`use super::` heuristic, because generated Say matches that
/// heuristic on every screen ever written (`use super::SayState;`) and only
/// a component reference actually wants the project.
pub const PROJECT_REACH: &str = "crate::widgets::";

/// One component in the registry: a name and the Say subtree it stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    /// The component's name, UpperCamel — the Rust type and the `<Name>` the
    /// screen writes.
    pub name: String,
    /// The extracted subtree, dedented to its own root, as Say.
    pub source: String,
    /// The file it was extracted from, as recorded when it was saved.
    pub from: String,
}

/// Write one component into a registry directory.
///
/// `source` is the dedented subtree; `from` is the `.say` file it came out
/// of, recorded so a component browsed months later says where it was born.
/// Overwrites an existing component of the same name — the newest extraction
/// of the same subtree is the one the user just asked for.
///
/// # Errors
///
/// If the directory cannot be made or the file cannot be written.
pub fn write_registry_entry(
    dir: &Path,
    name: &str,
    from: &str,
    source: &str,
) -> io::Result<PathBuf> {
    let entry = dir.join(name);
    std::fs::create_dir_all(&entry)?;
    let path = entry.join("component.say");
    std::fs::write(
        &path,
        format!(
            "-- extracted from {from} by vieww Studio's Extract Widget\n\
             -- referenced as <{name}>; scaffold it into a project from the Snippets view\n{source}"
        ),
    )?;
    Ok(path)
}

/// Every component a registry directory holds, sorted by name.
///
/// A directory that does not exist is an empty registry, not an error: a
/// studio that has never extracted anything has no components, which is a
/// fact, not a failure. An entry without `component.say` is skipped and said
/// nothing about — the Snippets view lists what works.
#[must_use]
pub fn registry_components(dir: &Path) -> Vec<Component> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Component> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let text = std::fs::read_to_string(entry.path().join("component.say")).ok()?;
            // The recorded origin is the first comment; the rest is the
            // subtree. Both halves are prose-shaped, so the split is on the
            // first line that is not a comment.
            let (head, body) = text
                .lines()
                .partition::<Vec<_>, _>(|line| line.starts_with("--"));
            let from = head
                .iter()
                .find_map(|line| line.trim_start_matches("-- extracted from ").split(' ').next())
                .unwrap_or("a .say file")
                .to_owned();
            Some(Component {
                name,
                source: body.join("\n"),
                from,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The name a fresh extraction takes: the picked widget's kind, then
/// `Kind2`, `Kind3`, … — never a file that already exists.
///
/// The same auto-naming rule `new_from_template` uses for screens, because
/// the situation is the same: a name is needed *now*, a dialog for it buys
/// precision this command does not need, and a colliding name is the one
/// thing auto-naming must never produce.
#[must_use]
pub fn next_component_name(root: &Path, kind: &str) -> String {
    let stem = kind.to_ascii_lowercase();
    let mut name = stem.clone();
    for suffix in 2..1000_u32 {
        if !root.join("src/widgets").join(format!("{name}.rs")).exists() {
            return upper_camel(&name);
        }
        name = format!("{stem}_{suffix}");
    }
    upper_camel(&stem)
}

/// `photo_grid_2` → `PhotoGrid2`: the Rust type a module name stands for.
#[must_use]
fn upper_camel(name: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for ch in name.chars() {
        if ch == '_' {
            upper = true;
        } else if upper {
            out.push(ch.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// The picked subtree's source span, as line numbers into the buffer:
/// `(first, last)` inclusive, 1-based.
///
/// The span is the picked widget line plus every widget line deeper than it
/// that follows before the next line at the picked depth or shallower. Lines
/// the widget-line reader cannot see (an event block's actions, blanks,
/// comments) are *included* when they fall inside that range — they belong
/// to the subtree visually, and the component compiler will refuse them if
/// they are what they look like.
#[must_use]
pub fn subtree_span(lines: &[WidgetLine], picked: &WidgetLine) -> Option<(u32, u32)> {
    let start = lines
        .iter()
        .position(|widget| widget.line == picked.line)?;
    let mut last = picked.line;
    for widget in &lines[start + 1..] {
        if widget.depth <= picked.depth {
            break;
        }
        last = widget.line;
    }
    Some((picked.line, last))
}

/// The buffer text with the span `span` replaced by the single line
/// `replacement`, indented as the span's first line is.
///
/// Carriage returns are preserved line for line, exactly as
/// `canvas_edit`'s one-line rewrites preserve them: a CRLF buffer must not
/// silently become an LF one.
#[must_use]
pub fn replace_span(text: &str, span: (u32, u32), replacement: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let first = usize::try_from(span.0).unwrap_or(1).saturating_sub(1);
    let last = usize::try_from(span.1).unwrap_or(1).saturating_sub(1);
    let indent = lines
        .get(first)
        .map(|line| line.len() - line.trim_start_matches(' ').len())
        .unwrap_or(0);
    let written = format!("{}{}", " ".repeat(indent), replacement);
    let mut out = String::with_capacity(text.len());
    // A newline is emitted *between* written pieces, never before the first
    // and never for a skipped line — the span's interior newlines go with
    // the span, and a trailing empty element (a final `\n`) survives as one.
    let mut wrote = false;
    for (index, line) in lines.iter().enumerate() {
        if index == first {
            if wrote {
                out.push('\n');
            }
            out.push_str(&written);
            wrote = true;
        } else if index > first && index <= last {
            continue;
        } else {
            if wrote {
                out.push('\n');
            }
            out.push_str(line);
            wrote = true;
        }
    }
    out
}

/// Extract the picked subtree as a component (ST-05's first verb).
///
/// The whole refusal list is stated in the module doc; what this adds is the
/// studio's own: a Say buffer, a pick that maps to a say line, and a
/// workspace to write into. Everything else — what may be extracted, what
/// the component file contains — is [`vieww_say_codegen::compile_component`]'s
/// to say, because the same compiler has to accept the result on the next
/// Render.
pub fn extract(studio: &Studio, name: &str) {
    let Some(root) = studio.root.peek().clone() else {
        studio.notify("Open a folder first — a component is a file in a project.");
        return;
    };
    let Some(widget) = studio.picked_say_line() else {
        studio.notify("Pick a widget on the canvas or in the Inspector first.");
        return;
    };
    let Some(buffer) = studio.active() else {
        return;
    };
    if buffer.language != crate::language::Language::Say {
        studio.notify("Extract works on a `.say` screen — the subtree comes out of Say.");
        return;
    }
    let lines = studio.say_widget_lines().unwrap_or_default();
    let Some(span) = subtree_span(&lines, &widget) else {
        studio.notify("The picked line is no longer in the buffer.");
        return;
    };
    let text = &buffer.value.text;
    let raw: Vec<&str> = text.split('\n').collect();
    let start = usize::try_from(span.0).unwrap_or(1).saturating_sub(1);
    let end = usize::try_from(span.1).unwrap_or(1).saturating_sub(1);
    let subtree: String = raw
        .get(start..=end)
        .map(|slice| slice.join("\n"))
        .unwrap_or_default();

    // The component compiler is the authority: purity, the name, the body.
    let options = vieww_say_codegen::Options {
        asset_root: buffer
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(|dir| dir.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    let generated = match vieww_say_codegen::compile_component(
        &buffer.name,
        name,
        &subtree,
        &options,
    ) {
        Ok(generated) => generated,
        Err(diagnostics) => {
            let first = diagnostics
                .first()
                .map(|d| {
                    let line = u32::from(d.line.saturating_sub(1)) + span.0;
                    format!("{} (line {line}): {}", d.code, d.message)
                })
                .unwrap_or_else(|| "the subtree cannot become a component".to_owned());
            studio.notify(&first);
            return;
        }
    };

    // The component file, its module list, and the library's `pub mod`.
    let module = name.to_ascii_lowercase();
    let widget_path = root.join("src/widgets").join(format!("{module}.rs"));
    if let Some(parent) = widget_path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            studio.notify(&format!("Could not create {}: {error}", parent.display()));
            return;
        }
    }
    if let Err(error) = std::fs::write(&widget_path, &generated.rust) {
        studio.notify(&format!("Could not write {}: {error}", widget_path.display()));
        return;
    }
    let line = format!("pub mod {module};");
    let mod_rs = root.join("src/widgets/mod.rs");
    match std::fs::read_to_string(&mod_rs) {
        Ok(existing) if existing.contains(&line) => {}
        Ok(existing) => {
            let joined = format!("{}\n{line}\n", existing.trim_end());
            if let Err(error) = std::fs::write(&mod_rs, joined) {
                studio.notify(&format!("Wrote the file, but not mod.rs: {error}"));
            }
        }
        Err(_) => {
            let contents = format!(
                "//! The project's extracted components — see `extract_widget`.\n//!\n//! One module per component; the screens reference them as `<Name>`.\n\n{line}\n"
            );
            if let Err(error) = std::fs::write(&mod_rs, contents) {
                studio.notify(&format!("Wrote the file, but not mod.rs: {error}"));
            }
        }
    }
    let lib_rs = root.join("src/lib.rs");
    if let Ok(existing) = std::fs::read_to_string(&lib_rs) {
        if !existing.contains("pub mod widgets;") {
            let joined = format!("{}\npub mod widgets;\n", existing.trim_end());
            if let Err(error) = std::fs::write(&lib_rs, joined) {
                studio.notify(&format!("Could not add `pub mod widgets` to lib.rs: {error}"));
            }
        }
    }

    // The buffer: the span becomes the reference, through the one edit path
    // every keystroke takes.
    let replaced = replace_span(text, span, &format!("<{name}>"));
    let len = replaced.len();
    let clamp = |offset: usize| offset.min(len);
    let mut value = vieww_foundation::TextEditingValue::new(replaced);
    value.selection = vieww_foundation::TextSelection {
        base: clamp(buffer.value.selection.base),
        extent: clamp(buffer.value.selection.extent),
        affinity: buffer.value.selection.affinity,
    };
    studio.edit(value);

    // The registry entry: best-effort, and said so — a registry that cannot
    // be written must not unwind a successful extraction.
    let dedented = dedent(&subtree);
    if let Err(error) = write_registry_entry(&registry_dir(), name, &buffer.name, &dedented) {
        studio.notify(&format!(
            "Extracted {name}, but the component library would not take it: {error}"
        ));
        return;
    }

    studio.rescan_tree();
    studio.notify(&format!(
        "Extracted {name} into src/widgets/{module}.rs — Build once (⌘B) so the preview can link it."
    ));
}

/// Scaffold a registry component into the open project (ST-05's second
/// verb): write the widget file, wire the modules, and put `<Name>` where
/// the caret is.
pub fn scaffold(studio: &Studio, component: &Component) {
    let Some(root) = studio.root.peek().clone() else {
        studio.notify("Open a folder first — a component is a file in a project.");
        return;
    };
    let Some(buffer) = studio.active() else {
        studio.notify("Open a `.say` screen to place the component in.");
        return;
    };
    if buffer.language != crate::language::Language::Say {
        studio.notify("Components are placed in a `.say` screen, as `<Name>`.");
        return;
    }
    let module = component.name.to_ascii_lowercase();
    let widget_path = root.join("src/widgets").join(format!("{module}.rs"));
    if widget_path.exists() {
        // Not an error: the project already has this component, and the
        // reference is still wanted.
        studio.notify(&format!(
            "{name} is already in this project — placing a reference to it.",
            name = component.name
        ));
    } else {
        let options = vieww_say_codegen::Options {
            asset_root: buffer
                .path
                .as_deref()
                .and_then(Path::parent)
                .map(|dir| dir.to_string_lossy().into_owned())
                .unwrap_or_default(),
        };
        let generated = match vieww_say_codegen::compile_component(
            &buffer.name,
            &component.name,
            &component.source,
            &options,
        ) {
            Ok(generated) => generated,
            Err(diagnostics) => {
                let first = diagnostics
                    .first()
                    .map(|d| format!("{}: {}", d.code, d.message))
                    .unwrap_or_else(|| "the component no longer compiles".to_owned());
                studio.notify(&first);
                return;
            }
        };
        if let Some(parent) = widget_path.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                studio.notify(&format!("Could not create {}: {error}", parent.display()));
                return;
            }
        }
        if let Err(error) = std::fs::write(&widget_path, &generated.rust) {
            studio.notify(&format!("Could not write {}: {error}", widget_path.display()));
            return;
        }
        let line = format!("pub mod {module};");
        let mod_rs = root.join("src/widgets/mod.rs");
        match std::fs::read_to_string(&mod_rs) {
            Ok(existing) if existing.contains(&line) => {}
            Ok(existing) => {
                let joined = format!("{}\n{line}\n", existing.trim_end());
                std::fs::write(&mod_rs, joined).ok();
            }
            Err(_) => {
                std::fs::write(
                    &mod_rs,
                    format!(
                        "//! The project's extracted components — see `extract_widget`.\n\n{line}\n"
                    ),
                )
                .ok();
            }
        }
        let lib_rs = root.join("src/lib.rs");
        if let Ok(existing) = std::fs::read_to_string(&lib_rs) {
            if !existing.contains("pub mod widgets;") {
                let joined = format!("{}\npub mod widgets;\n", existing.trim_end());
                std::fs::write(&lib_rs, joined).ok();
            }
        }
    }

    // The reference, as its own line at the caret: on the caret's line when
    // that line is blank, otherwise under it, indented as that line is.
    //
    // Not `insert_text_snippet`, on purpose: that path is Rust-shaped — it
    // asks `brace_depth` whether the caret is inside a `build`, and a `.say`
    // file has no braces, so every placement in every Say screen would be
    // refused as "an expression cannot go at the top level". Say's structure
    // is indentation, and whether `<Name>` may stand where the caret is, is
    // the Say compiler's to say on the next Render — through this edit, the
    // one path every keystroke takes, so undo and the debounce both engage.
    let value = placed_reference(&buffer.value.text, buffer.value.selection.extent, &component.name);
    studio.edit(value);
    studio.rescan_tree();
    studio.notify(&format!(
        "Placed <{name}> — Build once (⌘B) so the preview can link it.",
        name = component.name
    ));
}

/// `subtree` with the leading indentation of its least-indented line
/// removed — the form the registry stores and the component compiler wants.
#[must_use]
fn dedent(subtree: &str) -> String {
    let indent = subtree
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start_matches(' ').len())
        .min()
        .unwrap_or(0);
    subtree
        .lines()
        .map(|line| line.get(indent..).unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The buffer value after `<Name>` is placed as its own line at the caret,
/// with the caret left after it.
///
/// A **child of the caret's line** when that line already has children —
/// the reference goes at their indent, so it lands inside the container and
/// never between a parent and the children that would then be orphaned
/// under it. Otherwise a **sibling under it**, at the caret's line's own
/// indent. On a blank line the reference *is* that line. The caret moves to
/// just after the reference, so the next keystroke edits the line the
/// placement made.
#[must_use]
fn placed_reference(text: &str, caret: usize, name: &str) -> vieww_foundation::TextEditingValue {
    let caret = caret.min(text.len());
    let line_start = text[..caret].rfind('\n').map_or(0, |index| index + 1);
    let line_end = text[line_start..]
        .find('\n')
        .map_or(text.len(), |index| line_start + index);
    let own = text[line_start..].len() - text[line_start..].trim_start_matches(' ').len();
    // The indent of the next non-blank line, when it is deeper: the caret's
    // line is a container (`a column:` with children at 8), and a reference
    // at the caret's own indent would sit *between* it and its children —
    // which the parser then refuses as "indented further than the block it
    // is in", because a `<Name>` takes no children of its own.
    let child = text[line_end..]
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start_matches(' ').len())
        .filter(|indent| *indent > own);
    let indent = child.unwrap_or(own);
    let written = format!("{}<{name}>", " ".repeat(indent));
    // A blank line is REPLACED, not inserted before: the reference takes
    // the line's place (its indent, then the newline that followed it),
    // so the line's own whitespace cannot survive past the reference.
    let (at, tail, body) = if text[line_start..line_end].trim().is_empty() {
        (line_start, line_end, written)
    } else {
        (line_end, line_end, format!("\n{written}"))
    };
    let mut out = String::with_capacity(text.len() + body.len());
    out.push_str(&text[..at]);
    out.push_str(&body);
    let after = out.len();
    out.push_str(&text[tail..]);
    let mut value = vieww_foundation::TextEditingValue::new(out);
    value.selection = vieww_foundation::TextSelection::collapsed(after);
    value
}

/// The component name for a fresh extraction off the picked line.
#[must_use]
pub fn picked_component_name(studio: &Studio) -> Option<String> {
    let root = studio.root.peek().clone()?;
    let widget = studio.picked_say_line()?;
    Some(next_component_name(&root, widget.kind.name()))
}

impl Studio {
    /// ST-05's first verb: the picked subtree becomes a component.
    ///
    /// The name is derived (the picked kind, then `Kind2`, `Kind3`) by the
    /// same no-collision rule `new_from_template` uses — a name dialog would
    /// buy precision this command does not need and cost the one thing it
    /// must have, which is immediacy. The folder is asked before the pick,
    /// because a studio with neither deserves the message that names the
    /// bigger missing piece: a component is a file in a project first and a
    /// subtree second.
    pub fn extract_widget(&self) {
        if self.root.peek().is_none() {
            self.notify("Open a folder first — a component is a file in a project.");
            return;
        }
        let Some(name) = picked_component_name(self) else {
            self.notify("Pick a widget on the canvas or in the Inspector first.");
            return;
        };
        extract(self, &name);
    }

    /// ST-05's second verb: a registry component into this project.
    pub fn scaffold_component(&self, component: &Component) {
        scaffold(self, component);
    }

    /// The registry's components, for the Snippets view.
    #[must_use]
    pub fn registry_components(&self) -> Vec<Component> {
        registry_components(&registry_dir())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The span is the picked line plus the deeper lines that follow, and it
    /// stops at the next sibling.
    #[test]
    fn the_span_covers_the_subtree_and_nothing_else() {
        let source = "-- say-language: 1\n\
                      screen \"Home\":\n\
                      \x20   a column:\n\
                      \x20       a card, padded 12:\n\
                      \x20           a heading \"A\"\n\
                      \x20           a text \"B\"\n\
                      \x20       a text \"sibling\"\n\
                      \x20       a card, padded 8:\n\
                      \x20           a text \"C\"\n";
        let lines = vieww_say_codegen::line::parse_widget_lines(source);
        let card = lines
            .iter()
            .find(|w| w.line == 4)
            .expect("the card on line 4");
        assert_eq!(subtree_span(&lines, card), Some((4, 6)));
        let second = lines
            .iter()
            .find(|w| w.line == 8)
            .expect("the card on line 8");
        assert_eq!(subtree_span(&lines, second), Some((8, 9)));
        // A leaf extracts as itself.
        let leaf = lines.iter().find(|w| w.line == 7).expect("the sibling");
        assert_eq!(subtree_span(&lines, leaf), Some((7, 7)));
    }

    /// The replacement keeps every other line byte-identical, including
    /// carriage returns.
    #[test]
    fn the_replacement_is_surgical() {
        let text = "a\n    b\n    c\nd\n";
        let out = replace_span(text, (2, 3), "<X>");
        assert_eq!(out, "a\n    <X>\nd\n");
        let crlf = "a\r\n    b\r\n    c\r\nd\r\n";
        let out = replace_span(crlf, (2, 3), "<X>");
        assert_eq!(out, "a\r\n    <X>\nd\r\n");
    }

    /// Auto-names never collide with a file that exists, and are Rust type
    /// names.
    #[test]
    fn names_are_unique_and_upper_camel() {
        let dir = std::env::temp_dir().join(format!("components-names-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src/widgets")).expect("a temp dir");
        std::fs::write(dir.join("src/widgets/card.rs"), "// taken\n").expect("a stub");
        assert_eq!(next_component_name(&dir, "Card"), "Card2");
        assert_eq!(next_component_name(&dir, "Row"), "Row");
        assert_eq!(upper_camel("photo_grid_2"), "PhotoGrid2");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The registry round-trips: what extraction writes, browsing reads.
    #[test]
    fn the_registry_round_trips() {
        let dir = std::env::temp_dir().join(format!("components-registry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_registry_entry(&dir, "Note", "home.say", "a card, padded 12:\n    a text \"hi\"\n")
            .expect("the entry writes");
        let components = registry_components(&dir);
        assert_eq!(components.len(), 1, "{components:?}");
        assert_eq!(components[0].name, "Note");
        assert_eq!(components[0].from, "home.say");
        assert!(
            components[0].source.starts_with("a card"),
            "the comments are not part of the component: {:?}",
            components[0].source
        );
        // A missing directory is an empty registry.
        assert!(registry_components(&dir.join("nope")).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A reference is placed as its own line — a child of the caret's line
    /// when that line has children (never *between* it and them, which the
    /// parser would refuse: a `<Name>` takes no children), a sibling under it
    /// when it does not, and *on* a blank line. This is the Say-aware half
    /// `insert_text_snippet` cannot do: a `.say` file has no braces, so the
    /// Rust-shaped depth check would refuse every placement.
    #[test]
    fn a_reference_is_placed_as_its_own_line() {
        let say = "-- say-language: 1\nscreen \"Home\":\n    a column:\n        a text \"x\"\n";
        // The caret at the column's line end: the column has children, so the
        // reference goes at their indent, inside it.
        let at = say.find("a column:").expect("the line") + "a column:".len();
        let under = placed_reference(say, at, "Card");
        assert_eq!(
            under.text,
            "-- say-language: 1\nscreen \"Home\":\n    a column:\n        <Card>\n        a text \"x\"\n"
        );
        assert_eq!(
            under.selection.extent,
            under.text.find("<Card>").expect("placed") + "<Card>".len()
        );

        // The caret on a leaf: the next line is no deeper, so the reference is
        // a sibling after it, at the leaf's own indent.
        let leaf = say.find("a text").expect("the leaf") + "a text".len();
        let sibling = placed_reference(say, leaf, "Card");
        assert_eq!(
            sibling.text,
            "-- say-language: 1\nscreen \"Home\":\n    a column:\n        a text \"x\"\n        <Card>\n"
        );

        // On a blank line: the reference is that line, not a new one under it.
        let blank_input = "screen \"Home\":\n    \n    a text \"x\"\n";
        let blank = placed_reference(
            blank_input,
            blank_input.len() - "\n    a text \"x\"\n".len(),
            "Card",
        );
        assert_eq!(
            blank.text,
            "screen \"Home\":\n    <Card>\n    a text \"x\"\n",
            "the blank line became the reference"
        );
    }
}
