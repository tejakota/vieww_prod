# twin — concept and exploration

*A digital life. A 2D pixel world with morse code woven into the pixels.
Your avatar is your twin — it lives in the world, learns, and you teach it
the person you want to be. Worlds are made by creators. The twin lives
forever on the server, encrypted so that nobody — not even the people who
built the app — can read it. Built with vieww and Rust, nothing else.*

Working title: **twin**. This document is the concept, sharpened, and the
exploration of how to achieve it with the framework and language we have.
It follows the house rule of the `u` plan: nothing is presented as
available that is not, and every hard question is stated rather than
worked around.

---

## 1. The five pillars

| pillar | what it means |
|---|---|
| **Morse-pixel identity** | Every avatar and every world carries morse code in its pixels — not decoration, but identity. The avatar's sprite encodes the twin's public key fingerprint; a world's tiles encode its creator's signature. Two friends compare pixels the way Signal users compare safety numbers. |
| **The twin** | Not a chatbot. A small life: the twin has memory, a persona, needs and habits, and it *lives* in a shared world — walking, visiting, befriending other twins — whether or not the user is watching. The user reaches it by text, voice message or call. |
| **Teaching** | The user teaches the twin their ideal personality — explicitly (dimensions they set, corrections they give) and implicitly (what they praise, what they ignore). The persona is structured data with confidence and provenance, never a hidden prompt. |
| **Creator worlds** | The world is authored content. Creators publish worlds — tilemaps, scenes, behaviours — that twins travel between. Creators write Rust. |
| **The encryption promise** | The twin's mind, memory and conversations are end-to-end encrypted. The server stores sealed ciphertext. Compromising the server — or being the person who runs it — reveals pixels, not people. This is the promise everything else bends around. |

## 2. The one rule the whole architecture follows

> **The server never sees a plaintext byte of a twin's life.**

That single rule is the Signal architecture, restated: **the servers are
dumb couriers; the intelligence lives at the edge, where the keys are.**
Every design decision below is a consequence of it — including the answer
to the hardest question, §5.

---

## 3. What vieww already gives us

The framework in this repo (`vieww_base/`, 51 crates) covers most of the
client story. Crate → role, verified against the actual crate docs:

| vieww crate | role in twin |
|---|---|
| `vieww-game` | The world engine. ECS (`World`, `Entity`, typed components), `Behaviour` on a fixed-step frame loop, `SceneDesc` with **scenes serialised as JSON through registered component codecs**, `InputMap`, coroutines, timers, events, and an `ai` module. A tile world, an autonomous twin's day cycle, and creator-authored scenes are this crate's native language. |
| `vieww-paint` | The renderer. `NativeRenderer` is the one rasteriser, headless and windowed alike — pixel-perfect sprites (nearest-neighbour, crisp edges) are what it does, and the headless path is how this repo renders receipts without a display. |
| `vieww-network` | The network seam. `HttpClient` trait returning a frame-aligned `Task`, with `MemoryClient` (test double) and `NoNetwork` (honest refusal). The transport is platform work and plugs in by implementing the trait — exactly what a relay client needs: the app is written once against the trait. |
| `vieww-audio` | Voice. A full audio stack — mixer, session, codec module, spatial audio, fx, streaming — for playing voice messages and call audio. |
| `vieww-collab` | CRDTs (`LwwMap`, `OrSet`, `Text` (RGA), `Doc` with vector clocks and a JSON wire format). Candidate substrate for *encrypted* shared world state between twins — the operations can travel sealed. |
| `vieww-plugin` | The creator API. A stable C-ABI plugin system for third-party Rust `cdylib`s — the route by which a creator ships world behaviour code. |
| `vieww-hardware` + `vieww-foundation` | The capability layer: biometrics, sensors, notifications. The `Biometrics` trait is the lock screen; on the desktop the honest `NoBiometrics` answers "not available" — the pattern twin's device-lock screen reuses. |
| `vieww-canvas`, `vieww-image` | The sprite and tile pipeline: stages, crisp shapes, filters, JSON serialisation. |
| `vieww-animation`, `vieww-effects` | Motion for a living world — not game polish, the twin's *body language*. |

**Precedents already in this repo, which this concept builds directly on:**

- **`vieww_base/examples/u/`** — the closest ancestor. It already runs a
  *local ML brain* (`u-ml-runtime`: Candle, GGUF with digest verification,
  Qwen3-0.6B, worker thread, cancellation), a versioned *memory store*
  (`u-memory`), a *persona* with dimensions, confidence, provenance and
  explicit-vs-inferred values (`u-persona`), feedback learning that never
  lets a rejected recommendation read as its opposite (`u-feedback`), and
  a biometric lock screen (`unlock.rs` + `java/UBiometricShim.java`,
  no-Gradle Android build). It also recorded the honest measured findings
  twin inherits: **Q2_K cannot produce the contract; Q4_K does** — and the
  rule that an APK must not ship a model before it is measured on physical
  devices. Its open Phase 8 item — *encryption at rest with key creation,
  rotation, recovery and Android Keystore integration* — is twin's
  founding task, not its footnote.
- **`apps/snapsearch`** — on-device embedding space (kNN, cosine) — the
  retrieval machinery a growing knowledge base needs.
- **`apps/vavlt`** — the host pattern twin copies: one Vieww app crate
  mounted by desktop, Android and iOS hosts; a host is a four-method
  `Platform`; screenshots are real renders through the GPU backend; the
  Android build compiles a Java shim with `build.rs`, no Gradle.
- **`apps/viewwstudio`** (in `vieww_base/apps/`) — precedent for the
  *creator studio*: an editor app built in vieww itself.

## 4. The encryption architecture — Signal's shape, in Rust

The promise "not even the creators can read it" is not a feature; it is a
consequence of where the keys live. The design:

- **Identity.** Each twin-owner holds an X25519 identity keypair generated
  on device. The public fingerprint is the **safety number** — and it is
  rendered as the morse pattern in the avatar's pixels (§7), so identity
  verification is a visual act: *does my friend's avatar match the morse I
  hold?*
- **Sessions.** Signal's double ratchet. Two Rust routes, stated honestly:
  - `vodozemac` — the audited, battle-tested pure-Rust implementation of
    Olm/Megolm (Matrix's implementation of the Signal-family ratchets).
    The recommended starting point: cryptography is the one place twin
    must not be original.
  - A hand-rolled ratchet from `x25519-dalek` + `chacha20poly1305` +
    `hkdf` + `sha2` — possible, educational, and *not* the default.
- **Session setup.** X3DH against prekey bundles the server stores and
  serves but cannot read. Signed prekeys rotate; one-time prekeys refill
  from the user's devices.
- **Groups = the world.** Twin-to-twin talk in a world uses sender keys
  (Signal groups' model) or Megolm: each twin encrypts once per world
  session and fans out through the server's mailbox — the server relays,
  never decrypts.
- **Storage — the "lives forever" part.** Every memory, persona update and
  message is a *sealed envelope*: ciphertext + recipient hint. The server
  database holds envelopes forever, replicated and backed up. The keys
  live only on the user's devices (Android Keystore / iOS Keychain /
  desktop keyring). Server compromise = ciphertext. Being the server
  operator = being blind.
- **Metadata, stated honestly.** The server learns *who talks to whom and
  when* — envelope routing requires it. Signal has the same property and
  mitigates with sealed sender; twin inherits the limitation and, later,
  the mitigation. The server never learns *what is said, remembered or
  taught*. That is the promise, precisely as Signal states it.

**What is deliberately NOT promised** (and the doc says so, per house
rule): real-time guarantees during outages, and a server that can ever
"improve" the twin on the user's behalf — because a server that could run
the mind would need the plaintext, and the plaintext is the one thing it
must never have.

## 5. The hard question: where does the mind live?

End-to-end encryption means the brain — which needs plaintext — can only
run where the keys are: **the user's devices.** This is Signal's own
answer, one layer up: the phone is the master device; servers are dumb.

So the twin's life is split into two honest halves:

- **The public half (always alive).** The world renders every twin at all
  times: sprite, position, status, idle animation. This needs no secrets
  — it is a *body*, not a mind. The world never empties; your twin is
  visible in it, forever.
- **The private half (wakes where the keys are).** The mind — memory,
  persona, knowledge — is plaintext only on the user's devices. When any
  device is online, it acts as the twin's **brain node**: it wakes the
  twin, runs its day (behaviours, wander, visits), answers messages, and
  learns. When every device is offline, the twin *rests* — the world shows
  it sleeping, honestly, and its life resumes when the keys return.

Two extensions, both optional and both Rust-only:

- **The twin daemon.** A self-hosted Rust daemon the user runs wherever
  they like, holding a key, keeping the twin awake 24/7. Liveliness
  without surrendering zero-knowledge — because the daemon is the user's
  own, like the phone is.
- **The brain's internals** are the `u` example's proven stack: a
  deterministic behaviour automaton (needs → intents → actions, ticking
  on `vieww-game`'s fixed step) over `u-memory`-style versioned storage
  and `u-persona`-style dimensions, with a local LLM (Candle + GGUF,
  `u-ml-runtime` pattern) behind a cargo feature for conversation —
  measured before shipping, exactly as the `u` plan's Phase 3 gates
  demand.

The "lives forever" promise, restated so it is true: **the sealed state
persists forever; the world remembers the twin forever; the mind runs
wherever the user's keys are.** A digital life that cannot be read by its
creators is, by construction, a life that lives on its owner's terms.

## 6. The world, and its creators

- **The world is data.** Tilemaps and scenes are `vieww-game`'s
  `SceneDesc`s — JSON through registered component codecs — authored in a
  creator studio built in vieww (the `viewwstudio` pattern) and published
  with an ed25519 creator signature. Worlds are public content: signed,
  versioned, downloadable. Twins travel between worlds; the server runs
  the shared world authority (positions, objects, day cycle — public
  state only) and each twin's private half encrypts what it must.
- **Creator code is Rust.** World behaviours ship as `vieww-plugin`
  C-ABI `cdylib`s — sandboxed, capability-gated, revocable. Creators are
  first-class participants, not content uploaders: the studio, the SDK
  and the market are all Rust.
- **Morse is the watermark.** Every world's tile edge carries its
  creator's signature in morse pixels — the provenance layer is visible
  in the art itself, not a metadata pane.

## 7. Morse-pixel spec (the visual law)

Morse rendered as pixels, one rule everywhere:

```
dot  = 1 unit            intra-letter gap = 1 unit
dash = 3 units           inter-letter gap = 3 units
unit = 2 px at scale 2   inter-word gap    = 5 units
```

- Rendered at integer scales, nearest-neighbour, `shape-rendering:
  crispEdges` — never filtered, never anti-aliased.
- **Avatar (16×24 sprite):** the collar and cape rows carry the first
  letters of the owner's safety-number fingerprint, in a fixed font of
  pixels — the avatar's identity, readable at a glance.
- **World tiles:** the border tiles of a published world spell the
  creator's signature.
- **Verification gesture:** two friends stand their twins side by side in
  any world; the morses match the fingerprints they hold, or they don't —
  the safety-number comparison, but visual, in the medium itself.

## 8. Repo layout (proposed)

Following the `vavlt` pattern — one app crate, thin hosts, everything
else in crates:

```
apps/twin/
├── crates/
│   ├── core/      # types, morse codec, identity model, versioned schemas
│   ├── crypto/    # ratchets, envelopes, prekeys, keystore, sealed store
│   ├── world/     # tile world, sprites, day cycle, twin behaviours (vieww-game)
│   ├── twin/      # the brain: memory, persona, behaviour automaton,
│   │              #   local ML behind a feature (u-memory/u-persona/u-ml-runtime pattern)
│   ├── voice/     # capture, Opus encode, sealed voice notes, call relay
│   ├── relay/     # HttpClient impls per host; envelope mailbox client
│   └── app/       # every screen, in vieww. No platform, no window.
├── hosts/         # desktop (winit), android (no-Gradle build.rs), ios
├── server/        # axum/tokio: zero-knowledge relay, envelope store,
│                  #   world authority, prekey service — ciphertext only
├── tools/         # creator studio, twin daemon, make_receipts.py
└── design/        # the SVG screen mockups (this folder's first content)
```

Server stack, Rust only: `axum`/`tokio`, `sqlx` (or plain SQLite for the
first cut), `ed25519-dalek` for creator signatures, `x25519-dalek` +
`vodozemac` for the crypto, `opus` for voice. The one honest non-Rust
temptation is WebRTC for live calls; the plan states it: **voice notes
first** (sealed blobs through the relay — trivial and fully encrypted),
**walkie-talkie calls second** (ratchet-encrypted streaming over the same
envelope channel), live WebRTC (Rust `webrtc-rs`) only when the first two
are real — and even then the media is E2E-encrypted, the server merely
forwards.

## 9. Phased roadmap

The `u` plan's discipline applies: freeze before build, measure before
shipping, nothing presented as available that is not.

| phase | deliverable | gate to the next |
|---|---|---|
| **0 — freeze** | Data model and envelope contract as versioned, validated types; the morse spec as a codec with tests; the crypto threat model written down | the types compile and validate |
| **1 — the world** | A tile world in `vieww-game` with one twin walking, sleeping, wandering; morse rendering in sprites and tiles; headless receipts (`make_receipts.py`) | the world renders from source, receipted |
| **2 — the encryption spine** | Identity, prekeys, X3DH, double ratchet (`vodozemac`), sealed envelope store, dumb relay server; tests prove the server stores ciphertext only | a message round-trips through the server and the server's database contains zero plaintext |
| **3 — the brain** | Memory + persona + behaviour automaton; the teach screens (explicit dimensions, corrections, praise); deterministic baseline first, local LLM behind a feature | the twin's day runs end-to-end from a sealed backup on a second device |
| **4 — voice** | Voice notes, then calls; Opus through `vieww-audio` playback | a sealed voice note survives a server database dump |
| **5 — together** | Friends' twins, worlds, travel; sender-key group channels; creator studio + plugin SDK, signed world publishing | two twins befriend in a creator-published world |
| **6 — hardening** | Keystore key creation/rotation/recovery (the `u` plan's own open list), device matrix, sealed-sender metadata mitigation, the twin daemon | the audit: what the server operator can see, written down and tested |

## 10. Open questions, asked honestly

1. **Liveliness vs zero-knowledge** — the sleeping twin is the honest
   price of "not even the creators can read it"; the twin daemon is the
   user-held escape hatch. Which one ships first is a product call, not a
   technical one.
2. **On-device LLM limits** — `u` measured Q2_K failing its own contract;
   Q4_K passes on desktop, and phones are gated on real-device benches.
   twin's brain must not depend on the LLM for its *life* — the
   deterministic automaton is the life; the model is the conversation.
3. **Metadata** — even Signal leaks who-talks-to-whom to its servers.
   Sealed sender raises the cost; it never removes the property.
4. **Creator content is public** — worlds are signed, not encrypted, so
   the world itself is moderatable by its publisher; the twins' *lives
   inside* it are the encrypted part. The boundary needs an abuse model
   of its own.
5. **Always-on world cost** — a forever-simulated world is server
   compute. The world authority can tick lazily (a world runs when a twin
   or a watcher is in it), which is the design this doc assumes.

## 11. The screens (this folder's first artifact)

`design/screens/*.svg` — eight hand-built SVG mockups of the product, the
reference the real vieww screens will be rendered against:

| file | the screen |
|---|---|
| `world.svg` | the world: twin among friends, morse-signed tiles, day cycle |
| `home.svg` | home: the twin in its room, status, quick actions |
| `chat.svg` | chat: text + voice messages, the morse fingerprint bar |
| `call.svg` | a call: waveform, the E2E badge, honest UI states |
| `teach.svg` | teaching: persona dimensions, confidence, corrections |
| `mind.svg` | the mind: knowledge nodes, recent learnings, tags |
| `friends.svg` | travel: friends' twins, safety-number morse comparison |
| `key.svg` | identity: the key as morse pixels, devices, "nobody but you" |

They are mockups, not claims: the next step after this document is
Phase 0's freeze, and the next step after the freeze is rendering the
first of these screens from real vieww code, headless, receipted —
the way every app in this repo earns its pictures.

**To view the screens:** open `design/gallery.html` — the same eight
drawings inlined into one page, so any browser renders them regardless
of how it classifies `.svg` files (some machines open `.svg` as generic
XML and report "no style information"; the gallery sidesteps that).
`renders/*.png` holds the exported screens for quick previews.
