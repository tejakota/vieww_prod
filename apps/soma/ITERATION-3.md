# Iteration 3: the body answers, the card reads

Written 2026-11-19, on top of `cf1f61e` (iteration 2's search stack). Plan
IDs (S2, S3…) are from the analysis doc; DESIGN_SPEC.md is the UI contract.

## What changed, with the receipts

| Plan | Change | Receipt |
|---|---|---|
| S2 | **The right organ lights up.** `placements[i]` is now `db.organs[i]` (it was build order, so *Heart* lit a lung and *Kidneys* the stomach); paired organs light both halves; a deselected organ gets its own colour back (it stayed teal before). The camera flies to the meshes' centre, not a table anchor. | Probe: 16/16 meshes aligned (was 3/16). "heart attack" → heart, "kidny stones" → both kidneys. Test `every_organ_lights_its_own_meshes`. |
| S7 | **Organ pictures become 3D organs.** `tools/cut_organs.py` cuts each organ out of its labelled illustration (crop + colour, or "not the paper", largest shape, holes filled) into `assets/organs_cut/*.png`. At startup `vieww_mesh::relief::silhouette_solid` grows each into a textured solid at the organ's real size. 10 organs grown; lungs (the source is a photo of a clinic visit) and thyroid (too small in its drawing) keep their procedural shapes. The flat-image path (`organs.rs`, the figure in the card) is gone. | Test `pictured_organs_grow_from_their_cutouts`. Embedded art 1.7 MB of JPEG → 0.75 MB of cut PNG. |
| S17 | **Pan, zoom, rotate.** Drag orbits (pitch clamped ±63°), pinch zooms (0.35–6 units), the pinch's moving midpoint pans (clamped to the figure), wheel zooms. Round buttons for zoom, quarter turns, and recenter once the camera has been moved. A gesture stops a flight where it is; a later answer keeps the side the person chose. | Shot `8-side-view`. |
| S11 | **The card never covers the organ.** The card snaps to one end (head and neck → low, chest down → high) and the camera frames the organ in the other half (`bias`). The organ is framed by its own size (`frame_distance(extent)`), not the old per-region zoom. Callout and buttons sit on the side away from the card. | Shots `5`, `7`. |
| S6 | **The callout cites this query:** organ, the answer's title, the words that matched, the source host. Visible while typing too. | Shots `2`, `5`, `7`. |
| S18 | **The answer is text:** back, kind and organ, title, the line that matched the question (terms in bold, skipped when it is the opening line), the article (headings set as bold lines), nearby chips, source, disclaimer. Its own scroll controller. | Test `the_passage_is_the_paragraph_that_answers`. |
| S3 | **Safety layer:** red-flag phrases (chest pain, stroke signs, breathing, overdose, self-harm…) put an emergency card above results and answer; every list and answer carries the disclaimer. | `safety.rs`, 3 tests over every phrase. Shot `9-red-flag`. |
| S12 | **Readability:** 11 px minimum (was 8), 44 px targets, all 24 results (was 5), distinct kind letters (P was T), back stack, suggestions visible at rest, semantics labels. | No layout overflows in the reel. |
| S8 | **A resting body costs zero frames:** the sway runs 8 s after the last input, fades over 1 s, then the rig stops asking for frames. Blur scratch reused across frames. | Reel: "rest after 12 s: rig still animating = false". |
| S9 | **Search 3× faster, same answers:** titles and descriptions lowercased once at load and searched as bytes. | Eval 8.6 → 2.8 ms/query (release, 2-core VM), 26/29/31 unchanged; top-24 lists byte-identical on 10 probe queries. |

The close-up cost: x-ray close-ups put the see-through shell on every pixel,
so the 3D buffer drops a fifth in resolution as x-ray rises. Camera-flight
frame 14.1–14.9 ms (was 20.6 before that change), under 16.7.

## Not in this round

- **Licence (S1).** The cutouts are derivatives of the Cleveland Clinic
  illustrations, and the corpus is still theirs. Swap `assets/organs/` for
  licensed art and re-run `tools/cut_organs.py`; no code changes.
- **Side views of the body shell (S13).** Turning shows the shell's flat
  sides; a multi-view relief builder is the fix.
- **Rename (S16), responsive layouts (S15), size diet (S14), Android Back.**
- Pre-existing clippy notes (`region_at`, `render_frame` arity, search.rs
  `as_ref`) are untouched.

## Check it

```console
cargo test --lib                                     # 30 tests
cargo run --release --example shots -- /tmp/shots    # 9 shots + frame cost
cd ../soma-core && cargo test --release              # search + eval gate
python3 -I tools/cut_organs.py --sheet /tmp/cut.png  # regenerate the cutouts
```
