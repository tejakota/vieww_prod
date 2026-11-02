//! Deciding whether a new library's state can be adopted or has to be dropped.
//!
//! # The hazard this exists for
//!
//! `docs/HOT-RELOAD.md` §3 names it as one of three costs, and it is the only
//! one that is *unsound* rather than merely wasteful:
//!
//! > If the application edits the fields of something held in a `Signal` or an
//! > `ElementState`, the old allocation has the old layout and the new code
//! > reads it with the new one. That is undefined behaviour, not a glitch.
//!
//! An element that survives a reload keeps its `Box<dyn ElementState>` — the
//! allocation the *old* library made. The new library's code then reads it. If
//! the struct changed shape in between, every field access is at the wrong
//! offset.
//!
//! # What is detected, and what is not
//!
//! A fingerprint of each state type's **name, size and alignment**. Adding,
//! removing or retyping a field almost always moves one of the three.
//!
//! **A same-size reorder does not.** Swapping two `u32` fields keeps the name,
//! the size and the alignment, and this will happily adopt the old allocation
//! into code that reads the fields the other way round. That is a real hole and
//! it is stated rather than papered over — the mitigation is that
//! [`Verdict::Restart`] is cheap and always available, so an application that
//! sees nonsense after a reload should restart before debugging it.
//!
//! The other direction has the same limit: editing a state class's
//! fields forces a hot *restart* there, full stop. This is more permissive and
//! correspondingly sharper.

use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};

/// A summary of every state type a guest library owns.
///
/// Compared across two builds to decide whether element state can be carried
/// over. Built by the `guest!` macro, which is what makes it hard to forget a
/// type — it is written once next to the root function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fingerprint(u64);

impl Fingerprint {
    /// A fingerprint of nothing, for a guest that holds no state at all.
    ///
    /// Distinct from any real one, so "declared no state" and "declared one
    /// type that happens to hash to zero" cannot be confused.
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Build one from `(name, size, align)` triples, in declaration order.
    ///
    /// **Order matters and that is deliberate.** Reordering the list is a
    /// change to what the guest declared, and treating it as equivalent would
    /// mean a fingerprint that ignores the one thing it is cheap to notice.
    #[must_use]
    pub fn of(types: &[(&str, usize, usize)]) -> Self {
        if types.is_empty() {
            return Self::empty();
        }
        let mut hasher = DefaultHasher::new();
        for (name, size, align) in types {
            name.hash(&mut hasher);
            size.hash(&mut hasher);
            align.hash(&mut hasher);
        }
        // Zero is reserved for `empty`, so a real fingerprint that hashes to it
        // is nudged rather than reported as "no state".
        Self(hasher.finish() | 1)
    }

    /// The raw value, for crossing the library boundary as a plain `u64`.
    #[must_use]
    pub const fn to_bits(self) -> u64 {
        self.0
    }

    /// Rebuild one from the other side of that boundary.
    #[must_use]
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }
}

/// What the host should do with a newly built library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Swap the root and keep every element's state.
    Reload,
    /// Tear the tree down and mount the new root fresh.
    ///
    /// Not a failure — the application is still running and the developer still
    /// did not restart the process. It is the honest answer when the shape of
    /// the state changed, and it is what stands between a reload and undefined
    /// behaviour.
    Restart,
}

impl Verdict {
    /// Compare what the running guest declared against what the new one does.
    #[must_use]
    pub fn between(running: Fingerprint, incoming: Fingerprint) -> Self {
        if running == incoming {
            Self::Reload
        } else {
            Self::Restart
        }
    }
}

/// One state type as the guest declared it: its type path, size and alignment.
///
/// The fingerprint is these, hashed — fast to compare, and opaque. When the
/// comparison says *different*, the developer's next question is **which
/// type**, and the answer has to have been kept rather than thrown into the
/// hasher. Guests export the whole list alongside the fingerprint (see
/// [`crate::Guest::state_types`]), and the host holds it so a restart can
/// say what moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateType {
    /// The type path, as `core::any::type_name` reports it.
    pub name: String,
    /// `size_of`, which is what almost every field edit moves.
    pub size: usize,
    /// `align_of`, which a realignment moves at a constant size.
    pub align: usize,
}

impl StateType {
    /// The `(name, size, align)` triple [`Fingerprint::of`] hashes — the
    /// same facts, in the shape the hasher takes.
    #[must_use]
    pub fn as_triple(&self) -> (&str, usize, usize) {
        (self.name.as_str(), self.size, self.align)
    }
}

/// Which state types moved between two builds, for the restart a
/// [`Verdict::Restart`] owes an explanation to.
///
/// `Reloaded::Restart` used to say only "the shape changed", which put the
/// developer's next fifteen minutes into bisecting their own edit. The
/// information was always there — the fingerprint hashed it — and this is it
/// unhashed: the type paths that were added, removed, or reshaped.
///
/// A same-size **reorder** of a type's fields is still invisible here, for
/// the same reason it is invisible to [`Fingerprint`]: name, size and
/// alignment genuinely did not move. That sharp edge stays documented in this
/// module's header rather than papered over by a diff that cannot see it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StateDiff {
    /// Same type path, different size or alignment: the layout hazard.
    ///
    /// Each entry is the path plus the old and new `(size, align)`, so the
    /// message can say what moved rather than only that something did.
    pub reshaped: Vec<(String, (usize, usize), (usize, usize))>,
    /// Type paths the new build declares that the old one did not.
    pub added: Vec<String>,
    /// Type paths the old build declared that the new one dropped.
    pub removed: Vec<String>,
    /// The same types in a different declaration order — a fingerprint
    /// change with no single type to name.
    pub reordered: bool,
}

impl StateDiff {
    /// Diff the two declarations. Either side may be `None` — a guest built
    /// before the state-type table existed exports no list, and a diff that
    /// pretended to know would be worse than one that says so.
    #[must_use]
    pub fn between(running: Option<&[StateType]>, incoming: Option<&[StateType]>) -> Self {
        let (Some(running), Some(incoming)) = (running, incoming) else {
            // A restart whose cause cannot be named, honestly labelled as
            // such rather than padded with a guess.
            return Self {
                reordered: false,
                reshaped: Vec::new(),
                added: vec!["<type list unavailable: one build predates it>".to_owned()],
                removed: Vec::new(),
            };
        };

        let mut diff = Self::default();
        // Looked up by name rather than paired by position: a list whose
        // order changed between builds would otherwise compare a type
        // against its unrelated neighbour and report nonsense.
        for old in running {
            if let Some(new) = incoming.iter().find(|new| new.name == old.name) {
                if (old.size, old.align) != (new.size, new.align) {
                    diff.reshaped.push((
                        old.name.clone(),
                        (old.size, old.align),
                        (new.size, new.align),
                    ));
                }
            }
        }
        let old_names: Vec<&str> = running.iter().map(|t| t.name.as_str()).collect();
        let new_names: Vec<&str> = incoming.iter().map(|t| t.name.as_str()).collect();
        for name in &new_names {
            if !old_names.contains(name) {
                diff.added.push((*name).to_owned());
            }
        }
        for name in &old_names {
            if !new_names.contains(name) {
                diff.removed.push((*name).to_owned());
            }
        }
        // Same set of names, none of them reshaped, but the order differs:
        // worth a line, because it is the one fingerprint change a diff of
        // the members cannot otherwise explain.
        diff.reordered = old_names != new_names
            && diff.reshaped.is_empty()
            && diff.added.is_empty()
            && diff.removed.is_empty();
        diff
    }

    /// Whether there is anything to say — the empty diff a same-fingerprint
    /// pair produces, and the reason a host prints nothing on a `Reload`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.reshaped.is_empty()
            && self.added.is_empty()
            && self.removed.is_empty()
            && !self.reordered
    }
}

impl fmt::Display for StateDiff {
    /// The line a host prints when the tree is torn down, in the
    /// `name → change` shape that lets a developer jump straight to the type:
    ///
    /// ```text
    /// app::Counter 8→12 bytes · +app::Theme · −app::Scroll
    /// ```
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<String> = self
            .reshaped
            .iter()
            .map(|(name, (size, align), (new_size, new_align))| {
                if align == new_align {
                    format!("{name} {size}→{new_size} bytes")
                } else {
                    format!("{name} {size}→{new_size} bytes, align {align}→{new_align}")
                }
            })
            .collect();
        parts.extend(self.added.iter().map(|name| format!("+{name}")));
        parts.extend(self.removed.iter().map(|name| format!("−{name}")));
        if self.reordered {
            parts.push("declaration order changed".to_owned());
        }
        write!(f, "{}", parts.join(" · "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COUNTER: (&str, usize, usize) = ("app::Counter", 8, 4);

    #[test]
    fn an_unchanged_guest_keeps_its_state() {
        let before = Fingerprint::of(&[COUNTER]);
        let after = Fingerprint::of(&[COUNTER]);
        assert_eq!(Verdict::between(before, after), Verdict::Reload);
    }

    #[test]
    fn a_state_type_that_grew_forces_a_restart() {
        // The case this exists for: the old allocation is 8 bytes and the new
        // code reads 12. Adopting it is undefined behaviour rather than a
        // glitch, so the tree goes instead.
        let before = Fingerprint::of(&[COUNTER]);
        let after = Fingerprint::of(&[("app::Counter", 12, 4)]);
        assert_eq!(Verdict::between(before, after), Verdict::Restart);
    }

    #[test]
    fn a_realignment_forces_a_restart_even_at_the_same_size() {
        let before = Fingerprint::of(&[COUNTER]);
        let after = Fingerprint::of(&[("app::Counter", 8, 8)]);
        assert_eq!(Verdict::between(before, after), Verdict::Restart);
    }

    #[test]
    fn adding_a_state_type_forces_a_restart() {
        let before = Fingerprint::of(&[COUNTER]);
        let after = Fingerprint::of(&[COUNTER, ("app::Scroll", 4, 4)]);
        assert_eq!(Verdict::between(before, after), Verdict::Restart);
    }

    #[test]
    fn reordering_the_declaration_is_a_change() {
        // Cheap to notice, so noticed. A fingerprint that ignored order would be
        // silently weaker for no saving.
        let scroll = ("app::Scroll", 4, 4);
        assert_ne!(
            Fingerprint::of(&[COUNTER, scroll]),
            Fingerprint::of(&[scroll, COUNTER])
        );
    }

    #[test]
    fn no_state_at_all_is_its_own_answer() {
        assert_eq!(Fingerprint::of(&[]), Fingerprint::empty());
        assert_eq!(
            Verdict::between(Fingerprint::empty(), Fingerprint::empty()),
            Verdict::Reload,
            "a guest with no state can always be reloaded"
        );
        assert_ne!(
            Fingerprint::of(&[COUNTER]),
            Fingerprint::empty(),
            "declaring a type must never collide with declaring none"
        );
    }

    #[test]
    fn a_fingerprint_survives_the_trip_across_the_library_boundary() {
        // It crosses as a plain `u64` because that is the only thing about this
        // ABI that is guaranteed to mean the same on both sides.
        let original = Fingerprint::of(&[COUNTER]);
        assert_eq!(Fingerprint::from_bits(original.to_bits()), original);
    }

    #[test]
    fn a_same_size_reorder_is_the_documented_hole() {
        // Not a bug in this type — a limit of what size and alignment can see.
        // Asserted so that nobody later reads the docs, disbelieves them, and
        // has to rediscover it on a corrupted tree.
        let before = Fingerprint::of(&[("app::Counter", 8, 4)]);
        let after = Fingerprint::of(&[("app::Counter", 8, 4)]);
        assert_eq!(
            Verdict::between(before, after),
            Verdict::Reload,
            "two different field orders at one size are indistinguishable here"
        );
    }

    // ----- StateDiff: naming the type paths that moved a restart ------------

    fn types(list: &[(&str, usize, usize)]) -> Vec<StateType> {
        list.iter()
            .map(|(name, size, align)| StateType {
                name: (*name).to_owned(),
                size: *size,
                align: *align,
            })
            .collect()
    }

    #[test]
    fn a_grown_type_is_named_with_its_old_and_new_size() {
        let running = types(&[("app::Counter", 8, 4)]);
        let incoming = types(&[("app::Counter", 12, 4)]);
        let diff = StateDiff::between(Some(&running), Some(&incoming));
        let line = diff.to_string();
        assert!(line.contains("app::Counter"), "{line}");
        assert!(line.contains("8→12 bytes"), "{line}");
        assert!(!diff.is_empty(), "one reshaped entry only");
    }

    #[test]
    fn a_realignment_is_named_even_at_a_unchanged_size() {
        let running = types(&[("app::Counter", 8, 4)]);
        let incoming = types(&[("app::Counter", 8, 8)]);
        let line = StateDiff::between(Some(&running), Some(&incoming)).to_string();
        assert!(line.contains("align 4→8"), "{line}");
    }

    #[test]
    fn added_and_removed_types_are_marked_with_their_signs() {
        let running = types(&[("app::Counter", 8, 4), ("app::Scroll", 4, 4)]);
        let incoming = types(&[("app::Counter", 8, 4), ("app::Theme", 16, 8)]);
        let diff = StateDiff::between(Some(&running), Some(&incoming));
        let line = diff.to_string();
        assert!(line.contains("+app::Theme"), "{line}");
        assert!(line.contains("−app::Scroll"), "{line}");
        assert!(
            !line.contains("app::Counter"),
            "{line}: untouched types are not named"
        );
    }

    #[test]
    fn a_reordered_declaration_is_said_rather_than_left_as_a_mystery() {
        // The same two types, swapped: the fingerprint changes (order is
        // hashed) but no type moved — the diff says exactly that.
        let running = types(&[("app::Counter", 8, 4), ("app::Scroll", 4, 4)]);
        let incoming = types(&[("app::Scroll", 4, 4), ("app::Counter", 8, 4)]);
        let diff = StateDiff::between(Some(&running), Some(&incoming));
        assert!(diff.reordered);
        assert!(diff.to_string().contains("declaration order changed"));
        // And the fingerprint genuinely did move, so this diff is attached to
        // a real restart rather than to a no-op.
        assert_eq!(
            Verdict::between(
                Fingerprint::of(&[("app::Counter", 8, 4), ("app::Scroll", 4, 4)]),
                Fingerprint::of(&[("app::Scroll", 4, 4), ("app::Counter", 8, 4)]),
            ),
            Verdict::Restart
        );
    }

    #[test]
    fn an_unchanged_pair_diffs_to_nothing() {
        let running = types(&[("app::Counter", 8, 4)]);
        let incoming = types(&[("app::Counter", 8, 4)]);
        assert!(StateDiff::between(Some(&running), Some(&incoming)).is_empty());
    }

    #[test]
    fn a_side_with_no_list_says_so_rather_than_guessing() {
        // A guest predating the table exports no list; a diff that invented
        // one would send the developer hunting for an edit they did not make.
        let incoming = types(&[("app::Counter", 8, 4)]);
        let diff = StateDiff::between(None, Some(&incoming));
        assert!(
            diff.to_string().contains("predates it"),
            "{}",
            diff.to_string()
        );
    }

    #[test]
    fn the_diff_sees_through_a_reordered_list_rather_than_by_position() {
        // `Scroll` moved to the front and grew; `Counter` is untouched. A
        // positional diff would report Counter vs Scroll and miss the growth.
        let running = types(&[("app::Counter", 8, 4), ("app::Scroll", 4, 4)]);
        let incoming = types(&[("app::Scroll", 8, 4), ("app::Counter", 8, 4)]);
        let diff = StateDiff::between(Some(&running), Some(&incoming));
        assert_eq!(diff.reshaped.len(), 1, "{diff:?}");
        assert_eq!(diff.reshaped[0].0, "app::Scroll");
        assert!(diff.to_string().contains("4→8 bytes"), "{}", diff);
    }

    #[test]
    fn the_state_type_list_hashes_to_the_same_fingerprint() {
        // The table and the fingerprint are the same facts in two shapes;
        // if they ever disagree, a restart's explanation would describe an
        // edit that did not cause it.
        let list = types(&[("app::Counter", 8, 4), ("app::Scroll", 4, 4)]);
        let triples: Vec<(&str, usize, usize)> = list.iter().map(StateType::as_triple).collect();
        assert_eq!(
            Fingerprint::of(&triples),
            Fingerprint::of(&[("app::Counter", 8, 4), ("app::Scroll", 4, 4)])
        );
    }
}
