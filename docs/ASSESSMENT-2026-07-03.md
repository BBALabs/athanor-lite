# Condere — Critical Product Assessment

**Date:** July 3, 2026 · **State assessed:** M1 (commit `9a6ac80`)
**Method:** five independent analyses — full-code fragility audit, July-2026 competitive
research (sourced), first-timer walkthrough of the live UI, power-user (Ollama veteran)
audit, and defensibility analysis — synthesized with full knowledge of the codebase.
**Tone:** as requested — brutal. The praise is at the end because it's the smaller list.

> **Naming note:** the brief for this assessment referred to the product as **"Athanor"**
> (the alchemist's furnace — a strong name for a machine that turns raw hardware into
> intelligence). Everything in repo, UI, and identifiers currently says **Condere**.
> If a rename is happening, do it before M2 ships an updater, a data dir, and a manifest
> format that all embed the identifier `com.bba.condere`. Decision needed; cheap now,
> expensive after GA.

---

## 0. The one-paragraph truth

Condere today is a beautiful cockpit bolted to no engine. It detects hardware better than
anything in the category, recommends models it cannot download, and manages workspaces
that cannot chat. Nothing about M1 is a moat — a competent two-person team who saw every
screen could rebuild it in 2–4 weeks. The competitive window is real but narrow: nobody
owns hardware-aware recommendations, nobody owns the Windows novice, and the incumbents
are busy alienating their own users (Ollama's sign-in creep, Open WebUI's license retreat,
GPT4All's death). The product becomes real the day a first-timer chats within ten minutes
of install — and becomes defensible only if that same release starts **measuring** real
performance on real machines. Every strategic decision below flows from those two facts.

---

## 1. Weak points — what breaks under real usage

### 1a. Existential: it isn't an AI app yet
No inference, no chat, no downloads. In July 2026 the *free floor* of this category is
llama.cpp's own bundled WebUI — "a chat UI over llama-server" is worth $0. M1 is not in
the category; it's a hardware dashboard with a shopping list. Everything else in this
document is secondary to closing M2.

### 1b. Data safety — the "my workspaces disappeared" class (fix in hours, before any user)
From the code audit, all confirmed against source:

- **Every on-disk write is non-atomic** (`fs::write` truncates then writes —
  `workspaces/mod.rs:70,93`). Power loss or disk-full mid-write destroys the existing
  file. Disk-full is a *routine* state for an app whose job is downloading 40 GB models.
- **A torn `state.json` bricks the app permanently.** Boot step 4 propagates the parse
  error; the fault screen's only affordance is a reload that re-reads the same corrupt
  file forever. The file is a trivially reconstructible cache and should never fail boot.
- **No schema version, no `#[serde(default)]` on any persisted struct.** The first
  additive field change in M2 strands every existing user's manifests — silently
  ("workspace skipped: unreadable manifest").
- **No single-instance guard.** Two launches race read-modify-write on `state.json`
  with an in-process-only mutex; second telemetry thread; second NVML session.
- **Workspace delete is instant recursive `remove_dir_all`** behind a two-click confirm.
  Fine today (one JSON file); catastrophic at M3 when that directory holds a client's
  document corpus and fine-tunes. Needs move-to-trash semantics before contents matter.
- **Mutex poisoning:** any panic during a workspace op poisons `WsLock` and every
  subsequent workspace command panics for the session (`lib.rs:32,45,55,65`).
- **Boot is a fail-fast serial chain** — a broken WMI service (common on unhealthy
  Windows installs) blocks the user from even browsing the catalog, which needs no
  hardware at all. Each boot step should degrade independently.

### 1c. Distribution — the product can't reach machines
No code signing (SmartScreen shows "Windows protected your PC" to exactly the novice
audience we target), no updater (`tauri-plugin-updater` absent — every fix below requires
a manual reinstall), no crash reporting, log rotation keeps ~40 KB (diagnostics gone by
the time a user reports a bug), and `targets: "all"` will happily produce untested
macOS/Linux artifacts that misclassify every Mac as CPU-only garbage-tier. Signing +
updater are M2 blockers, not M5 polish.

### 1d. The core promise has honesty gaps
The whole thesis is "it knows your machine; you never hit an OOM wall." Today:

- **Memory floors are valid only at exactly 8K context** while the UI advertises 128K
  context windows on the same row. KV cache scales linearly with context; the flagship
  70B pick OOMs the moment a user raises context. The fit math must take context as an
  input (`kv_gb_per_1k` per entry) or clamp the advertised context to the headroom.
- **The "tight" verdict includes models up to 15% *over* budget** (`Models.tsx`) —
  "tight" currently means "will OOM or spill to RAM at 5–10× slowdown." Over budget must
  read "exceeds"; "tight" belongs *inside* the budget (85–100%).
- **Budget ignores VRAM already in use.** DWM + browser + a game can hold 2–8 GB; the
  recommendation OOMs on contact. We already collect `vram_used_bytes` — use it.
- **Multi-GPU machines are budgeted as their single largest card.** A 2×24 GB rig
  (BBA's own blackbox server) is told 70B "exceeds this machine." It doesn't
  (`--tensor-split`).
- **No partial-offload model of the world.** The defining technique of local inference —
  25 of 80 layers on GPU, rest on CPU — doesn't exist in our math. A 12 GB + 64 GB RAM
  machine *can* run 70B at reading speed; we say impossible. Same for KV-cache
  quantization (halves KV memory) and MoE expert offload (the trick that makes
  30B-A3B fly on an 8 GB card). The power-user verdict was fair: *"every number it
  shows a power user is somewhere between conservative and wrong."*
- **Speed is never modeled — only fit.** A P40 and a 4090 with equal VRAM get identical
  cheery recommendations. We detect architecture beautifully, then ignore it.
- **Docs/code drift:** ARCHITECTURE.md says `×0.90 − 1.0 GB`; code says `×0.95 − 0.5 GB`.
  The product's core equation shouldn't have two values.
- **Catalog is frozen in the binary.** No update path except app releases (and there's
  no updater). Models age in weeks in this space. The catalog must become a remotely
  fetched, signed, cached document — with the embedded copy as fallback.

### 1e. Platform reality
GPU detection is NVML + Windows-only WMI/registry. On macOS (a huge share of local-AI
users — unified memory changes the budget math entirely) and Linux+AMD, we'd silently
classify capable machines as CPU-only. Either model unified memory/ROCm properly or
don't ship those builds. Also: `minWidth: 1100` doesn't fit a 1366×768 laptop at 125%
scaling — the single most common budget display, i.e., exactly our CpuOnly customers.

### 1f. Harness leakage (root of the "wrong GPU" incident)
The design harness ships in the production bundle and activates on one falsy global.
It's now labeled, but it should be dev-gated behind `import.meta.env.DEV` + dynamic
import so tree-shaking removes it from shipped JS entirely, and its hand-mirrored fit
math (already drifting from `recommend.rs`) should die with it.

---

## 2. Areas of improvement — exists, but should be much better

1. **Recommendation engine → runtime configurator.** The pure-function architecture is
   right; the math is naive (see 1d). Phase it: (1) context-aware KV + used-VRAM budget,
   (2) multi-GPU sum with tensor-split caveat, (3) offload slider with live VRAM
   projection, KV-quant options, MoE offload. The UI for (3) — a slider where the
   projected VRAM line moves against the machine's real ceiling — is also a wow moment.
2. **Onboarding.** The novice tester finished the entire tour without finding anything
   to *do*: "a beautiful dashboard with ZERO buttons." The dashboard needs one primary
   action, jargon needs progressive disclosure (Q4_K_M, VRAM, headroom, ctx behind a
   "technical details" toggle), the jewels need a legend, the rail needs tooltips, and
   "telemetry 1 Hz" needs renaming — it made a privacy-minded user think we phone home.
   (We don't. The status line should say so: "local monitor · nothing leaves this machine.")
3. **Copy system.** Two registers everywhere: plain-English first ("Quality level:
   Excellent — the best this machine can hold"), instrument detail on demand. The model
   blurbs are the only place the novice felt spoken to; that voice should cover fit
   verdicts, classes, and the ring.
4. **Boot resilience** (1b) — degrade per-subsystem instead of the all-or-nothing chain.
5. **Error surfaces.** `CondereError` codes exist but the UI shows raw strings in a
   toast. Map codes to designed states with actions (retry / open folder / diagnostics).
6. **Workspaces are folders with a color.** The killer concept has no killer content
   yet — a workspace must *hold* things visibly (model, prompt, docs, params) by M3 or
   the differentiator reads as a label-maker.
7. **Performance hygiene at scale:** O(n) full re-read on every workspace op, manifest
   rewrite on every switch, unvirtualized shelf — fine at 10, audit at 200.

---

## 3. What would make it MORE useful and easy to use

### For the first-timer (never ran a local model)
The novice tester wrote our onboarding spec verbatim:

1. First launch, one sentence: *"Condere runs AI entirely on your computer. Nothing you
   type ever leaves this machine."* One button: **Set me up.**
2. Auto-check: *"Your machine is exceptional. It can run Llama 3.3 — ChatGPT-class,
   fully private."* (Q3_K_M/VRAM/headroom behind "technical details.")
3. One button: **Download (34 GB, ~20 min)** with honest progress and a plain note.
4. Auto-create a "My Chat" workspace — skip the form entirely on first run.
5. Land in a chat box: *"Ask anything — this never leaves your computer."*

Everything currently on screen belongs in the "day 3" experience, not day 1. Both are
worth keeping — the instrument cluster is why they'll *stay* — but day 1 has one job.

### For the power user (a year of Ollama, 300 GB of models)
Their three dealbreakers, verbatim from the audit, in priority order:

1. **Import-in-place.** Scan `~/.ollama/models/blobs` (already sha256-addressed — it
   hash-matches straight into our content-addressed store design) and arbitrary GGUF
   folders. Zero re-download. This is the entire power-user acquisition funnel; without
   it, "re-download 300 GB I own" = instant uninstall.
2. **Exposed OpenAI-compatible endpoint + headless.** llama-server *is* the endpoint —
   surface it (port + token toggle) so their scripts, n8n flows, and Open WebUI keep
   working with a one-line base-URL change. Every incumbent has this; absence is
   disqualifying.
3. **Honest, visible resource control.** Context length actually enforced (Ollama's
   silent truncation is their #1 resentment), explicit offload/KV/split settings, live
   VRAM occupancy — which our telemetry already draws beautifully.

Plus: custom models (paste an HF repo), and vision models in the catalog (the thing
novices actually ask for first).

---

## 4. What makes this IMPRESSIVE — the wow inventory

**Already real:**
- The boot ignition → Power Ring reveal. Every tester, in every round, called out the
  first ten seconds. This is the demo moment; protect it.
- "It knows my machine" — even the confused novice said the fit line was *"the one
  moment the app felt smart on my behalf."*
- Live telemetry as instrument cluster — replaces the power user's permanent
  `watch nvidia-smi` tmux pane, and makes novices feel their computer is a beast.
- Detection craft: correct VRAM where WMI lies, Blackwell identified by compute
  capability. Invisible to most, but it's why the numbers are *right*, and the power
  crowd noticed ("more careful than anything in Ollama's Windows path").

**Buildable, high-wow-per-effort:**
1. **The capability reveal** (onboarding step 2): "This machine can run a 70-billion-
   parameter model — the class behind ChatGPT — entirely offline." Plain words + the
   ring lighting up = the screenshot people post.
2. **First-load benchmark scene:** after the first model loads, run a 30-second measured
   benchmark with the gauges live — "Your machine: 42 tokens/sec." Doubles as the moat
   seed (§5). Nobody else turns first-load into theater *and* data.
3. **The offload slider** with a live VRAM projection line — the first time the
   projected line crosses the real ceiling and the verdict flips, a power user screams.
4. **Workspace switch as ignition:** switching workspaces swaps model + prompt + docs
   in one glide (spine flare, model hot-swap). "Switching AIs like switching cars."
5. **Ollama import montage:** point it at the blob store, watch 40 models fly into the
   library with fit verdicts attached, zero bytes downloaded. The Reddit demo.
6. **Before/after speed receipt** on imported models (llama-server vs Ollama's 30–70%
   documented deficit): "Same model. Same machine. 1.6× faster here."

---

## 5. What would make this HARD TO REPRODUCE — the moat, honestly

**Verdict from the defensibility analysis: M1 has zero moat.** ~1,400 lines of Rust and
a very good coat of paint; every visible component is a 2–5 day rebuild for a competent
team, the UI 2–3 weeks at the 80% fidelity their users will accept. LM Studio could ship
a matching "fit advisor" panel in one sprint. Do not confuse "nobody has done it this
beautifully" with "nobody can."

**The one real, available moat: the measured performance database.**
Every install that runs inference (M2+) measures ground truth — prefill/decode tok/s,
VRAM at load and at full context, time-to-first-token, thermal throttle onset, and
(most valuable) OOM/failure events with the exact config that caused them — keyed to
(GPU, driver, CPU, RAM, model, quant, ctx, offload, llama.cpp build). Every competitor
ships static heuristics; this becomes the only *empirical* answer to "what will THIS
machine do with THIS model" in the category. It compounds classically: rows → better
recommendations → users → rows. A cloner gets zero of the corpus; replication cost is
their install base × time — 12–24 months for a fast follower, never if we keep growing.
The long tail (the 3060-laptop-slow-DDR4 configs, the P40s we literally own) is where
static heuristics fail and fleet data wins.

**Non-negotiables for the telemetry design** (the sovereign audience will read the payload):
local-first SQLite the user can open (sold as *your* performance history — a feature),
consent screen at first inference showing the literal JSON, default **OFF**, reciprocity
(contributors get fleet-measured numbers: "measured across 214 machines like yours"),
schema + aggregate dataset published openly, records keyed to llama.cpp build and
decay-weighted. Never prompts, never content, never fine-grained timestamps. One privacy
incident kills the play and the BBA brand with it.

**The moat that's live today: the BBA loop.** No competitor is a sovereign-AI
consultancy. Condere as the client-side artifact of every engagement: Gary's 96 GB
Blackwell machine is install #1, rare-hardware telemetry source #1, case study #1.
Engagements produce battle-tested workspace templates, weird-hardware data, revenue;
the app is the pre-sales sizing tool and demo for the next $5K–30K deployment; fleet
features (M4+) convert deployments into support retainers. The moat isn't in the
software — it's that the founder's day job is the distribution channel and the
requirements engine.

**Deferred-but-design-now: workspace templates.** A portable `.condere` manifest
("Legal reviewer for 12 GB cards") is a Docker-Hub-shaped play — but the format is
copyable in a week; the moat is the *library with hardware-verified fit claims*, which
inherits the performance DB's corpus. Design manifests for portability now (stable ids,
hash refs, no absolute paths — one week of schema care); build sharing only after M3
retention proves people love workspaces.

**Fake moats — do not budget as defense:** UI polish (conversion asset, zero retention
defense), "community" (a cost center a solo founder loses to Ollama's Discord; the only
community asset that works solo is *passive* telemetry contribution), open-sourcing the
app (hands over the only crafted code; goodwill doesn't convert for solo maintainers —
open the telemetry *schema and dataset* instead), the curated catalog (a JSON file,
copyable in an afternoon).

**The sequencing rule:** the moat clock starts when the first real token is measured.
If M2 ships chat without instrumentation, the corpus head start burns while we polish.

---

## 6. What makes this WORTH SOMETHING — the value proposition

**The honest competitive frame (July 2026):** the real head-to-head is **LM Studio**,
not Ollama — free for commercial use, shipping monthly, headless daemon, MCP, mobile
via its Locally AI acquisition, and the category's only fit-*checking* badges. Its seams:
closed source, developer-dense UX, weak RAG, no workspace concept, Electron. Ollama is
the volume default we must beat in the first ten minutes; Msty ($149/yr, proving
individuals pay for polished local AI) is the pricing comp; AnythingLLM has workspaces
without polish or hardware intelligence; GPT4All is dead and its ~250K stranded users
are the easiest acquisition pool in the category.

**Why someone chooses Condere over Ollama + Open WebUI:**

1. **It knows your machine — measured, not guessed.** Every incumbent checks fit at
   best; nobody *recommends*, and (post-M2) nobody else's numbers come from real fleet
   measurements. "You have a 4070 Ti. Here are your three best models, here's why,
   here's what you're giving up" — that sentence doesn't exist anywhere else.
2. **Workspaces: per-job AI, switched like projects.** Model + prompt + documents +
   memory as one portable, deletable directory. Ollama has models; Open WebUI has
   chats; AnythingLLM has document buckets; nobody has the whole per-job stack as a
   first-class switchable object. This is the retention hook and the only genuinely
   original product idea we own.
3. **The sovereign trust posture.** No account, no phone-home (default), telemetry
   opt-in with the payload on screen, verifiable local-first everything — a position
   every major incumbent has vacated (sign-in creep, closed source, license retreats)
   and the one that matches BBA's brand exactly.
4. **Windows-first, native-fast.** The gaming-GPU install base *is* the local-AI
   hardware base, and every incumbent is culturally mac-first Electron. A Rust/Tauri
   app that idles near zero and proves it on its own gauges is a felt difference.
5. **It's beautiful.** Not a moat — but it's why the first five minutes convert, why
   screenshots spread, and why "local AI" stops looking like a nerd tool. Novices said
   it out loud: *"I trusted it instantly on aesthetics alone."*

One line: **"The local AI app that actually knows your machine — private by default,
gorgeous by design, with a workspace for every job."**

---

## 7. Prioritized action plan

### P0 — Existential (M2, in this order)
| # | Item | Why | Cost |
|---|---|---|---|
| 1 | Data-safety quartet: atomic writes (+ tolerate corrupt state.json), schema versions + serde defaults, single-instance guard, poison-proof locks | Eliminates the entire "workspaces disappeared / won't boot" class while user count is zero | ~2 days |
| 2 | **M2 core: resumable verified downloads + llama-server lifecycle + streaming chat** | Not in the category without it | the milestone |
| 3 | **Telemetry/measurement pipeline inside M2** — local-first, opt-in, schema published | The only compounding asset; the clock starts at first token | +2–4 wks |
| 4 | **Import-in-place** (Ollama blobs + GGUF folders) | The power-user funnel; near-free given content-addressed store design | ~1 wk |
| 5 | **Exposed OpenAI-compat endpoint** (port + token) | Table stakes; one config surface over the sidecar we're building anyway | days |
| 6 | Code signing + updater + remote signed catalog | Can't reach or stay on machines otherwise; catalog ages in weeks | ~1 wk + cert cost |
| 7 | First-run onboarding (the 5-step novice path) + jargon disclosure + legends/tooltips | Converts the audience the category abandoned | ~1 wk |

### P1 — Trust in the core promise (M2.x)
8. Context-aware fit math (KV per-1K in catalog), used-VRAM-aware budgets, "tight" never over budget, docs/code equation reconciled.
9. Multi-GPU budgets (sum + tensor-split caveat) — BBA's own server is the test rig.
10. Dev-gate the design harness out of production builds; move catalog.json to a shared location.
11. Boot degrades per-subsystem; error codes → designed states; log rotation + webview log capture.
12. Ship 3–5 first-party workspace templates; design the portable manifest schema now.
13. Platform honesty: Windows-only artifacts until unified-memory (Mac) and ROCm math exist; fix minWidth for 1366×768@125%.

### P2 — Differentiation (M3 window)
14. Offload slider / runtime configurator with live VRAM projection (+ KV quant, MoE offload).
15. First-load benchmark scene + local performance history view ("your machine over time").
16. RAG (M3) with sources-shown honesty + **MCP client** (market says M3, not someday).
17. Speed modeling in recommendations (architecture-aware tok/s estimates from fleet data).
18. Vision models + custom HF-repo add in catalog.
19. Import speed receipt: measured before/after vs Ollama.

### P3 — Flywheel (M4+)
20. Publish the data: "will it run" web widget, per-GPU pages, State of Local Inference report — the acquisition engine for the corpus.
21. Template sharing with hardware-verified fit claims.
22. Fleet/org features tying into BBA retainers; Condere ships with every engagement starting with Onyx.
23. Fine-tuning (M4) — genuinely open territory; nobody credible ships it in-app.

---

## 8. What's genuinely good (keep, protect)

The Black Glass design system and boot sequence (the conversion weapon — just never
call it a moat). The detection stack's correctness culture: registry-over-WMI VRAM,
compute-capability architecture ID, per-GPU diagnostics logging, the `probe_real_hardware`
support tool. The pure, unit-tested recommender architecture (the math is naive; the
*shape* is exactly right for the configurator it must become). Workspaces as portable
directories referencing a content-addressed store — the one original idea, and the
right one. Typed IPC end-to-end. The instinct to verify on real hardware before
believing a bug report. And the honesty norms this assessment is part of: the app's
entire value proposition is *trustworthy numbers* — the culture that fixes a "tight"
verdict because it's a lie is the product.

*Assessment prepared for Tony Winslow / Black Box Analytics. Full analyst reports
(fragility audit with file:line refs, market research with sources, novice walkthrough,
power-user audit, moat analysis) available in the session transcript.*
