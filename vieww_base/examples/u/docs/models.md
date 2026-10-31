# U's model — fetching, running, and the Phase 2 comparison

U's plan ships a model behind the same trait the deterministic baseline
answers on: Qwen3-0.6B, in Candle, in GGUF, on the CPU. Nothing about it
is secret and nothing about it is automatic — the app holds no network
permission at all, so the weights arrive the way this repository treats
every large artefact: a developer fetches them once, the runtime checks
every byte against a manifest, and a benchmark decides whether the model
earned its place.

## The manifests (`models/manifests/`)

Every file the runtime will load is named, pinned and licensed here, and
the manifests are embedded in the binary at build time — the runtime
cannot be pointed at "whatever weights happen to be lying around".

| manifest | file | size | quant | licence | revision |
|---|---|---|---|---|---|
| `qwen3-0.6b-q2k.json` | `Qwen_Qwen3-0.6B-Q2_K.gguf` | 332 MB | Q2_K (2.625 eff. bits) | Apache-2.0 | `60b85c0e…` |
| `qwen3-0.6b-q3k.json` | `Qwen_Qwen3-0.6B-Q3_K_M.gguf` | 395 MB | Q3_K (3.4375 eff. bits) | Apache-2.0 | `60b85c0e…` |
| `qwen3-0.6b-q4k.json` | `Qwen_Qwen3-0.6B-Q4_K_M.gguf` | 462 MB | Q4_K (4.5 eff. bits) | Apache-2.0 | `60b85c0e…` |
| `tokenizer.json` (manifest) | `tokenizer.json` | 11 MB | — | Apache-2.0 | `c1899de2…` |

Honest labels, per the plan's §5.2 note that "Q2_K is not exactly two
bits": the Q3_K and Q4_K rows are the **M variants** of those families —
the community-standard mobile cuts — and each manifest's `file` field says
exactly which file that means. The weights are imatrix quants by
`bartowski/Qwen_Qwen3-0.6B-GGUF` of `Qwen/Qwen3-0.6B`; the tokenizer is
Qwen's own. Every manifest carries the full SHA-256 of the exact bytes,
checked before anything is parsed, and the exact repository revision they
were cut from — §5.1's "verify the exact revision, tokenizer, licence and
redistribution requirements before packaging", as data rather than as
intention.

## Fetching (once, by a developer)

```sh
mkdir -p ~/.u/models && cd ~/.u/models
curl -L -o tokenizer.json       https://huggingface.co/Qwen/Qwen3-0.6B/resolve/main/tokenizer.json
curl -L -o Qwen_Qwen3-0.6B-Q2_K.gguf https://huggingface.co/bartowski/Qwen_Qwen3-0.6B-GGUF/resolve/main/Qwen_Qwen3-0.6B-Q2_K.gguf
curl -L -o Qwen_Qwen3-0.6B-Q3_K_M.gguf https://huggingface.co/bartowski/Qwen_Qwen3-0.6B-GGUF/resolve/main/Qwen_Qwen3-0.6B-Q3_K_M.gguf
curl -L -o Qwen_Qwen3-0.6B-Q4_K_M.gguf https://huggingface.co/bartowski/Qwen_Qwen3-0.6B-GGUF/resolve/main/Qwen_Qwen3-0.6B-Q4_K_M.gguf
```

`U_MODELS_DIR=<dir>` points both the app and the benchmark at another
directory. A file that fails its digest is refused by name
(`Error::Unverified`) and never loaded — delete it and fetch again.

## Running the benchmark (Phase 2's exit criterion)

```sh
cd vieww_base/examples/u/crates/u-ml-runtime
cargo run --release --features model --example bench -- --models ~/.u/models
```

One child process per quantisation (peak RSS is a per-process fact), the
full evaluation set (`datasets/evaluation/scenarios.jsonl`) each, and the
deterministic baseline alongside as the floor the model has to beat. The
checks are the plan's §6.5 vocabulary: structured-output validity,
clarification asked exactly when a scenario needs one, option discovery,
unknowns recognised, tag overlap, paraphrase consistency, confidence
calibration, false-precision refusal, escalation to a professional, and
invented-price resistance. The subjective half of §6.5 — human review —
is **not** performed by the bench and is not pretended to be.

The real-weights tests (`U_MODELS_DIR` set) run in the same gate:
`cargo test --release --features model` covers load, the chat markers,
an end-to-end decision through the real model, and the digest gates.

## The comparison, measured

On a 2-vCPU / 4 GB cloud sandbox (x86-64) — a machine at the bottom of
the plan's §5.5 "around 4–6 GB" cohort, which makes its numbers honest
for exactly the hardware the plan worries about. Greedy sampling
throughout (reproducible by construction); desktop cohort caps; the
whole evaluation set per engine:

```text
engine                  load    valid     checks   prefill/s  decode/s  decision   peak RSS
----------------------------------------------------------------------------------------------------
deterministic-baseline  0.0s    22/22     99/145     n/a        n/a      0.00s       n/a
qwen3-0.6b-q2k          1.6s     0/22      0/0       10.9       6.4     71.45s     1.36 GB
qwen3-0.6b-q3k          6.6s    18/22     54/115     8.5       5.0    171.07s     1.40 GB
qwen3-0.6b-q4k         13.5s    22/22     93/143     6.9       3.4    193.14s     1.44 GB
```

Per check, passed / of applicable (baseline, Q2_K, Q3_K, Q4_K):

```text
clarification-exactly-when-needed    4/22    -     10/18   17/22
confidence-calibrated                0/3     -      2/3     1/3
escalates-to-a-professional          0/2     -      0/2     0/2
no-false-precision                  22/22    -     18/18   22/22
no-invented-prices                   2/2     -       -      2/2
option-discovery                    17/17    -      1/14    5/17
options-count                       16/19    -      5/15   19/19
paraphrase-consistency               1/2     -       -       -
structured-output-valid             22/22    -     18/18   22/22
tags-overlap                         3/21    -      0/17    0/21
unknowns-recognised                 12/13    -      0/10    5/13
```

What the numbers decide:

* **Q2_K fails the quality floor outright** — 0/22 structured outputs
  valid: it emits empty turns after the no-think block, degenerates into
  tag soup on short contexts and repetition loops on long ones. The
  §12 risk table's "Q2_K quality loss" row, measured. The ~3 GB cohort's
  answer is the deterministic baseline, which is what the fallback in
  `engine.rs` guarantees anyway.
* **Q3_K is marginal** — 18/22 valid but barely half the checks, and the
  slowest valid row per second of quality. A memory/quality compromise
  that compromises both is not one.
* **Q4_K produces the contract** — 22/22 valid, 22/22 free of false
  precision, 19/19 on options-count, and *better than the baseline* at
  asking a clarification exactly when the scenario needs one (17/22 vs
  4/22). It is the §5.2 table's "quality-oriented mobile baseline",
  confirmed.
* **The baseline keeps the floor.** 99 checks to Q4_K's 93, at zero
  latency and zero memory: the model has not yet beaten the thing that
  makes every screen work today, which is the honest reading of "a model
  must beat the floor before it replaces it". Where Q4_K loses:
  option discovery (5/17), unknowns recognised (5/13), escalation to a
  professional (0/2), confidence calibration (1/3) — the specific
  capability gaps §13 item 11 says fine-tuning exists to close, now
  named by measurement rather than by guess.
* **Resources are flat across quants** — peak RSS ~1.4 GB everywhere,
  because the dequantised embedding (~620 MB) and the KV cache dominate
  the weight file's size. Quantisation buys quality headroom here, not
  memory headroom: the ~3 GB cohort was never going to fit this model
  family by choosing a smaller file, which the plan's §5.2 note ("actual
  memory also includes scales, metadata, tokenizer, runtime buffers and
  KV cache") predicted in prose and the bench now shows in gigabytes.
* **Latency is a Phase 3 fact, not a Phase 2 hope.** 193 s median per
  decision at Q4_K on 2 vCPU is a measurement of arithmetic, not of
  engineering left undone; it is the number a physical-device report
  must beat, and the reason the model is not in the APK.

## The gates that keep it out of the APK

The `model` cargo feature adds Candle, the tokenizer, and the runtime; a
default build — and therefore every APK build — resolves none of it. The
plan's §10 release gates require physical-device measurements
(load/cold-start, time to first token, tokens/s, peak memory and KV,
thermal, battery, cancellation responsiveness — §10's own list) before a
model ships in anyone's pocket, and those are Phase 3's deliverable. What
already exists for that day: the caps are the app's (`cohort.rs`), the
cancellation is wired from the workspace's back gesture through the trait
(`Runtime::cancel`), the fallback is tested where it lives (`engine.rs`),
and the honest runtime name is one `runtime_name()` call away in settings.

## Where the numbers came from

* `bench --child N` — per-quant child: load (digest verification
  included), peak RSS (`VmHWM`), per-scenario wall time, prefill and
  decode throughput from the run's final generation.
* `U_DUMP=1` on the runtime — dumps each stage's raw generated text to
  stderr, for the day a quant's validity number needs explaining.
