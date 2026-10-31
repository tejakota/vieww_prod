# The plan, closed — phase by phase

*U — Complete Product and Engineering Plan*, implemented state, phase by
phase, with the file that owns each promise. The plan's own §14 rule
applies throughout: nothing is presented as available that is not, and
the open items say what would close them.

## Phase 0 — product and architecture freeze

**Delivered.** The data model and output contract are types, versioned
and validated where they live: `u-decision`'s `Interpretation` (§4.2) and
`Assessment` (§4.4, `contract_violations()`), `SCHEMA_VERSION: 1`. Privacy
is structural: no network permission in the APK manifest at all, checked
by the `U APK` workflow. The device matrix is `cohort.rs`'s table. Baseline
metrics: the deterministic row of every benchmark run.

## Phase 1 — core decision experience

**Delivered, shipped in the APK.** Home is one primary action
(`src/home.rs`), the workspace takes free-form input with optional
structure and no categories (`src/workspace.rs`), the orchestration is
`src/engine.rs` behind the `Runtime` trait, and history is the store's
`decisions`/`assessments` records. Zero-, one- and many-option decisions
are all one code path; clarification is one question at most
(`fold_answer`).

## Phase 2 — Candle proof of concept

**Delivered.** The runtime is `crates/u-ml-runtime` with the `model`
feature: GGUF loading with digest verification (`quantization.rs`,
`model.rs`), Qwen's own tokenizer validated for the chat markers
(`tokenizer.rs`), prompts in Qwen3's exact chat template with thinking
off and injection-neutralised user text (`prompt.rs`), greedy-by-default
sampling and §5.6 stopping (`generation.rs`), a worker thread with a
cancellation token and a timeout behind the same trait (`qwen.rs`), and
the Q2_K/Q3_K/Q4_K benchmark CLI over the evaluation set
(`examples/bench.rs`). Manifests with real digests, revisions and
licences: `models/manifests/`. Results: `docs/models.md`.

**The comparison's headline finding:** Q2_K cannot produce the contract;
Q4_K does. See `docs/models.md` for the measured rows.

## Phase 3 — Android CPU deployment

**Open, by the plan's own gates.** The code paths a phone needs exist and
are tested — cohort caps from `/proc/meminfo`, cancellation from the
workspace's back gesture, timeout, error recovery, the tested fallback —
but §10 forbids shipping the model in the APK until it has been measured
on physical devices (load/cold-start, time to first token, tokens/s, peak
memory, thermal, battery, cancellation responsiveness). The APK therefore
ships the deterministic baseline, `model` is a desktop-only cargo feature,
and the numbers that would open this gate are a bench run on real
hardware. What is also open from this phase's own list: lifecycle-aware
*UI* for a long decision (the current `decide` is synchronous on the UI
thread; the model's arithmetic is already on a worker, but the wait is
the UI's to own).

## Phase 4 — local memory and persona

**Delivered.** `u-memory` (the store, versioned, forward-compatible),
`u-persona` (dimensions with value/confidence/context/source/time-stamp,
explicit-vs-inferred, current/uncertain/superseded, §7.4's bounded
update), relevant-only retrieval by the situation's own tags (§7.5), and
the settings screen where a person inspects and corrects every preference
(§7.7, `src/settings.rs`).

## Phase 5 — feedback learning

**Delivered.** The ask sheet's one-question-at-a-time training
(`src/ask.rs`, `src/sheet.rs`), the event log (`src/events.rs`),
assessment verdicts and corrections that teach the persona nothing
(§7.3, `u-feedback`), and a rejected recommendation never read as its
opposite (§7.3's own rule, tested in `u-persona`).

## Phase 6 — general model improvement

**Open, and correctly so.** The plan's own order: fine-tune "only when
evaluation reveals a specific capability gap". The evaluation set
(`datasets/evaluation/scenarios.jsonl`) and the automated half of §6.5's
metrics now exist to reveal one; the curated training data, SFT/LoRA
work and held-out human review are the response, and they have not been
pretended at. The one gap the bench already names: Q2_K and Q3_K's
structured-output reliability, which is a quantisation-quality finding
first and a fine-tuning question only after Q4_K's ceiling is measured.

## Phase 7 — advanced personalisation experiment

**Deliberately not built.** §7.6 and §13.12 both gate it on "only if the
structured persona baseline is insufficient", and §15 forbids treating
the head as committed architecture. No `u-personalization` crate exists
for that reason, and this line is the decision record.

## Phase 8 — production hardening

**Partially delivered; the rest is named, not implied.** Delivered:
offline-first is total (no network permission), the §5.6 safeguards are
code, honest runtime naming, reduced-motion respected by the animation
layer, and no unimplemented feature is presented as available — the
tests and screens enforce the wording. Open, because they must be
*implemented and verified* before claim: encryption at rest with key
creation/rotation/recovery and Android Keystore integration (§8's own
list), biometric lock, export, and privacy-reviewed telemetry. The §12
risk table's mitigations are all in place structurally.

## The §13 backlog, item by item

1. **Freeze the data model and output contract** — done: versioned types,
   validation with the types, forward-compatibility refusals.
2. **New Decision without category selection** — done: the workspace has
   no categories anywhere; tags are inferred and optional.
3. **Deterministic orchestration and validation** — done: `src/engine.rs`
   + `u-decision`, contract-enforcing.
4. **A diverse evaluation set before model tuning** — done:
   `datasets/evaluation/scenarios.jsonl`, 22 scenarios over §6.4's
   dimensions, with paraphrase pairs and per-check expectations.
5. **Integrate Candle, load the first Qwen3-0.6B** — done: the `model`
   feature, digest-gated loading, the real model deciding in tests.
6. **Benchmark Q2_K/Q3_K/Q4_K on the same scenarios** — done:
   `examples/bench.rs`, results in `docs/models.md`.
7. **Android ARM64 CPU inference** — the runtime is CPU-only and
   architecture-neutral; the APK gate is Phase 3's device report.
8. **Measure on physical devices** — open (see Phase 3).
9. **Local persistence and contextual persona** — done (Phase 4).
10. **Explicit feedback and optional preference questions** — done
    (Phase 5).
11. **Fine-tune only when evaluation reveals a gap** — the evaluation
    exists; the gap it names is quantisation quality, not yet a
    fine-tuning gap.
12. **Personalisation head only if the baseline is insufficient** — the
    decision not to build it is recorded in Phase 7.

## §9's structure vs. the repository

The plan's preamble governs: "adapt this structure to the existing
repository rather than duplicating or moving working code". Six of the
nine crates are the plan's own names (`u-core`, `u-decision`,
`u-ml-runtime`, `u-persona`, `u-memory`, `u-feedback`). `u-app` is
`src/engine.rs` — the orchestration the screens call, which is the
responsibility §9 gives that crate. `u-platform-android` is the app's own
`src/lib.rs` (activity lifecycle, storage locations) plus `cohort.rs`
(device capability, the one thing the plan names that lived nowhere) —
splitting a third crate out of working code is what the preamble forbids
until there is a second consumer. `u-personalization` does not exist, on
Phase 7's recorded decision. `models/manifests/`, `datasets/evaluation/`
and `docs/` exist exactly as drawn.
