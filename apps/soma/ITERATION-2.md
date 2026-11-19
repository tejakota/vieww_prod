# Iteration 2 — the search stack, measured

Written 2026-11-19, the session after v0.2 shipped (the APK, the wasm build
and the seven screenshots in `download/`). Read this beside
`../vieww_prod/recovered/README.md`, which explains the framework situation:
**the vieww workspace is not on this machine** — only `vieww-mesh` and the
recovered `vieww-3d/src/scene.rs` survived the sandbox recycle — so nothing
that links the app could be built this round. Everything below was done in
the two modules that never needed the framework, against a harness that
compiles them unmodified.

## The harness

`../soma-core/` is a std-only crate whose two modules are this project's own
`src/db.rs` and `src/search.rs`, included by `#[path]` — no copy, no fork.
`cargo test` in `soma-core` runs every app test on the app's own files;
`cargo run --bin queries` is `examples/graph_debug.rs`'s twin; and
`tests/eval.rs` is new: **the search-quality benchmark**.

## The benchmark

Thirty-one queries a person would actually type, each with the corpus page
that should answer it, scored top-1/3/8 by group (exact / synonym /
semantic / prefix / typo). The floors are the engine's current numbers —
`cargo test --test eval` is now a gate; a ranking change that costs recall
fails the build. `-- --nocapture` prints the table.

## What changed, with the receipts

| group | v0.2 top-1 | v0.3 | v0.2 top-3 | v0.3 | v0.2 top-8 | v0.3 |
|---|---|---|---|---|---|---|
| exact | 6/6 | 6/6 | 6/6 | 6/6 | 6/6 | 6/6 |
| synonym | 6/9 | 7/9 | 8/9 | **9/9** | 8/9 | 9/9 |
| semantic | 0/6 | 3/6 | 1/6 | **4/6** | 2/6 | **6/6** |
| prefix | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 |
| typo | 0/8 | **8/8** | 1/8 | **8/8** | 1/8 | 8/8 |
| **overall** | 14/31 | **26/31** | 18/31 | **29/31** | 19/31 | **31/31** |

Four changes produced that, in order of size:

1. **The typo layer** (`search.rs`, the fifth query engine). A token the
   index does not carry is offered its nearest index term — bounded
   Damerau-Levenshtein (one edit for tokens under eight chars, two above;
   a two-edit false positive barely exists at that length), gated by the
   candidate's evidence (≥ 5 postings, idf ≥ 3 — a term two topics carry
   is not a word someone was trying to spell), and joined at 0.6 weight
   under the `EXPANDED` badge. Cost, measured in release: **+98–230 µs**
   per misspelled token, against the ~5.5 ms the literal scan already
   costs. "migrane", "dyabetes", "pnemonia", "hipatitis", "aneamia" now
   find their pages; they used to find nothing.

2. **The saturation cap was the semantic layer's disease.** The old
   combine held the cosine at `min(sem × 2.4, 2.6)` — but a strong cosine
   runs 2.5–7, so every good answer tied at 2.6 and the ordering fell to
   tiebreakers (kind boost, title length, the graph). The raw cosine
   ranked *Insomnia* #2 for "cant sleep" and *Frequent Urination* #1 for
   "peeing a lot"; the cap buried both. The ceiling is now 12.0 — a guard
   against pathological density, not a blend control. An exact title match
   collects both layers and still cannot be outranked (the eval's exact
   group is unchanged, 6/6 everywhere).

3. **Title evidence in the vector layer.** The builder counts a title
   mention at tf 3 (descriptions at 1), so a posting at tf ≥ 3 means the
   term is *in the title*. Those contributions are multiplied by 1.5: the
   page that IS the word outranks a page that merely says it often —
   *Tranylcypromine*'s leaflet says "depression" more than the Depression
   page does, and the cosine alone could not tell them apart.

4. **The plural stemmer bug.** `stem("stones")` returned `"ston"` while
   `stem("stone")` returned `"stone"` — the `es` rule fired on regular
   plurals of `-e` words, so *every* singular query missed *every* plural
   title ("muscles"→"muscl" vs "muscle", "headaches"→"headach" vs
   "headache"). The rules are now sibilant-aware (`watches`, `dishes`,
   `boxes` keep the strip; `-aches` is `-ache`+`s`; everything else strips
   only `s`), mirrored in `build_db_semantic.py`, asserted by mirrored
   test vectors on both sides, and `soma_db.txt` was regenerated: **only
   section 4 changed** (5,962 → 5,850 terms; topics, organs, bodies and
   the graph are byte-identical). "kidney stone" (singular) now leads
   with *Kidney Stones*.

Plus one line: the synonym `(" razor burn", …)` had a leading space and
never fired at the start of a query — the one place it most likely sits.

## What did NOT change

The public API (`search()`, `Hit`, `found`), the regex engine, the graph
walk, `db.rs` (the corpus asset changed, the parser did not), and every
UI-facing module — `anatomy.rs`, `screens/home.rs`, `organs.rs` are
untouched, so when the framework comes back nothing here rewrites them.

## Rebuilding

The wasm and the APK in `download/` are v0.2 — they still run (verified
headless this session: boot, search, result tap, topic open with the
illustration; `soma-screens/8–10-runtime-verify-*.png`). They do not carry
these search changes. To ship v0.3: restore the vieww crates (see
`../vieww_prod/recovered/README.md` for the exact list), then the standard
builds — `cargo test` first; the app's own test run now includes everything
`soma-core` runs, against the same files.
