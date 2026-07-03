# Athanor Design Language — "Black Glass" (v2)

**Binding spec for all UI work. Supersedes v1 entirely.**
Thesis: Athanor is a cockpit, not a dashboard — one sheet of black glass on which warm
light surfaces where the machine is alive. One dominant instrument per view. Light does
the layout; lines never do. The room feels like a parked S-Class breathing on at night.

Derived from a 4-concept judged design study (MBUX cockpit direction, with grafts from
B&O, Porsche, and Arc concepts) plus a forensic audit of v1's "AI-dashboard" tells.

---

## 1. Non-negotiable rules

1. **Zero borders, zero hairline boxes.** Zones separate by luminance (alpha surfaces),
   depth shadow, and space. The only permitted "line" is light (see §6).
2. **No gradient text. Anywhere.** Solid warm ink only.
3. **No chips/pills.** Metadata is quiet text. Verdicts are jewels (§6). One radius
   language from tokens; `border-radius: 99px` is banned.
4. **One heartbeat.** Exactly one pulsing dot in the app (status bar). Liveness elsewhere
   is proven by data visibly moving.
5. **Uppercase micro-labels: ≤ one per group,** 10px, +0.14em, 40% ink. Never stacked.
6. **Accent = light, never paint.** The violet family appears only as glow, light-lines,
   and lit values — never as flat fills or borders. Semantic amber/red appear only on
   threshold breach ("redline discipline"); healthy is silent ink.
7. **Effects budget ≈ 0** beyond the three signatures (§6). No glow creep.
8. **Motion:** nothing snaps, nothing bounces. `--glide: cubic-bezier(.22,.9,.24,1)`.
   Hover 200ms, layout 480ms, choreography 700–900ms. Values tween, never step.
9. **Copy is instrument-grade.** Labels name values, not concepts. No marketing voice,
   no roadmap teasers inside the tool.
10. **Spacing/type from tokens only.** Space scale 8/16/28/48/72 (prefer larger). Six
    type steps, no ad-hoc sizes.

## 2. Type

- **Outfit** (variable) — display + all numerals. `tnum` everywhere numeric.
- **Instrument Sans** (variable) — UI/body.
- **JetBrains Mono** — quant codes / machine identifiers only, ≤ 11px.

| Step | Spec | Use |
|---|---|---|
| hero | Outfit 300, 88px, −0.03em, tnum | ring numeral, workspace monogram |
| display | Outfit 500, 28px, −0.02em | best-model name; one view title per view |
| readout | Outfit 400, 26px, tnum | telemetry band numerals |
| body | Instrument Sans 450, 13.5px | default |
| quiet | Instrument Sans 400, 12px, 55% ink | metadata, alternates |
| label | Instrument Sans 500, 10px, +0.14em, uppercase, 40% ink | sparing |

## 3. Palette & lighting model

One light source: the **ambient spine** (left). Surfaces brighten toward it; every glow
is warm violet→orchid (never neon/cyan). Ink is warm off-white.

```css
--field: #0A070E;                          /* the cabin */
--srf-1: rgba(244,240,234,.025);           /* resting zone (alpha = calibration-proof) */
--srf-2: rgba(244,240,234,.05);            /* raised / hover */
--srf-3: rgba(244,240,234,.085);           /* active / sheet */
--ink-hi: #F4F0EA;  --ink-mid: rgba(244,240,234,.62);
--ink-low: rgba(244,240,234,.38); --ink-ghost: rgba(244,240,234,.18);
--lume: #A06BFF; --lume-warm: #C99AF5; --lume-hot: #EFD9FF; --lume-deep: #4C2B85;
--ok: #9BD8A8; --warn: #E7C088; --bad: #E29A94;   /* desaturated; breach-only */
```

Surface recipe: alpha background + `inset 0 1px 0 rgba(244,240,234,.04)` glass
catch-light + `0 24px 60px -24px rgba(0,0,0,.7)` depth. Radius 20 (28 hero).

## 4. Layout

**Shell:** one continuous pane. Transparent 44px titlebar (small letterspaced wordmark,
window controls ghosted until hover). 76px rail: icons floating, no divider — the spine
carries the edge. 32px status line. 48px content gutters.

**Dashboard (instrument cluster):**
- Zone A (~55%): machine identity left (stacked quiet type + small class line) · **Power
  Ring** center (VRAM pressure; RAM when no GPU) · **recommendation** right (label,
  28px model name, quant/fit quiet line, alternates dim → brighten on hover).
- Zone B: one horizontal telemetry band, shared baseline — label above, 26px numeral,
  2px light-line beneath (length+warmth = load) with faint mirrored reflection. Disks
  as smaller same-baseline entries. CPU cell may carry a whisper sparkline (no dot).
- Zone C: per-role picks as one dim inline row.
- Eye flow: ring → best model → band. Nothing competes.

**Models (showroom):** vertical ledger of light rows (72px): name 20px / family+params
dim center / **fit jewels** right. Recommended model pinned first as a 120px hero row
with under-glow + light sweep. Text filters with sliding 2px light-bar underline.
Row click expands in place (height glide) to quant table (mono) + license. No install
affordance in M1.

**Workspaces (garage):** active workspace as hero panel of light (~40%): 96px monogram
(first letter, Outfit 300) at 12% opacity behind the name. Others on a horizontal
CSS-snap shelf of wide low tiles, under-glow in own hue (light only). Create = bottom
sheet over `blur(24px)` glass: boxless inputs (luminous baseline on focus), curated
6-swatch accent row (no hue slider). Monogram auto-derived; no glyph picker.

**Boot (~1.9s):** field → spine sweep (600ms) → wordmark tracking eases +0.5em→+0.18em
→ ring draws (dashoffset) while numerals count → zones rise 12px staggered 90ms. Boot
steps render as one quiet progress line, no dingbats.

## 5. Signature elements

1. **Ambient Spine** — 3px vertical light band at the rail edge + 28px-blurred duplicate,
   8s breathing loop; flares only on genuinely significant events; drifts toward warn
   hue under sustained heavy load.
2. **Power Ring** — 300px SVG, 270° arc, 6px round-cap gradient stroke (deep→hot), ghost
   track, single hot dot riding the tip, **no tick marks**; last 10% is the redline zone
   (desaturated `--bad`) lit only when entered. Center: hero numeral + one label.
3. **Light Sweep** — a slow specular pass (masked gradient translate) on the hero surface
   and the recommended row ONLY. Nowhere else, ever.

Grafts in force: fit jewels (B&O), alpha surfaces (Arc), redline discipline (Porsche),
mirrored reflection under light-lines (Arc), boxless luminous inputs (Arc), ghosted
window controls (B&O), hover-as-light row sheen (B&O).

## 6. Kill list (v1 patterns, banned forever)

Gradient text · bordered card grids · dot-grid wallpaper · chips/pills of any species ·
rainbow role-tag colors · full-spectrum hue slider · LED segment cells with glow cascade ·
tick-ring gauge · three heartbeat dots · dingbat workspace glyphs (◆✦⬢…) · bolt/stack/
pulse cliché icons · gradient-filled active pill tabs · center-stacked no-decision
layouts · dashed "add new" ghost tile · marketing microcopy & roadmap chips · glow
drop-shadows as default · uppercase label stacking · ad-hoc spacing.

## 7. Keep (v1 assets that survive)

Token plumbing (rebuilt values) · in-house icon craft & keystone mark (re-conceived
motifs) · data-first instrumentation mechanics (ring dashoffset tween, sparkline ring
buffer, real fit verdicts) · tabular-nums discipline · interaction correctness (two-step
delete, boot min-hold, selectable error text, quiet scrollbars, aria meters).
