//! soma-core — the app's brain, benched without its body.
//!
//! Two `#[path]` includes and nothing else: `db.rs` and `search.rs` are the
//! soma application's own files, compiled here exactly as the app compiles
//! them. Nothing in this crate may patch, wrap or reimplement them — if the
//! search needs to change, it changes *there*, and this bench picks it up on
//! the next build. That is the whole point: iteration on the data and search
//! layers does not need the vieww workspace, and cannot drift from what the
//! app ships.
//!
//! What lives here that does not live in `soma`:
//!
//! * `bin/queries.rs` — the ranked-answer dumper (`graph_debug.rs`'s twin).
//! * `tests/eval.rs` — the search-quality benchmark: a fixed query set with
//!   expected answers, scored top-1 / top-3 / top-8, so a ranking change is
//!   a number, not a feeling. It prints a table and fails under a floor, so
//!   `cargo test` is the gate and `cargo test -- --nocapture` is the report.
//!
//! The corpus is embedded by `db.rs` itself (`include_str!` of
//! `soma/assets/db/soma_db.txt`, 13 MB), so the bench needs no environment.

#[path = "../../soma/src/db.rs"]
pub mod db;

#[path = "../../soma/src/search.rs"]
pub mod search;
