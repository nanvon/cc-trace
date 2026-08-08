---
name: CC Trace
description: A HeroUI-styled adaptive desktop system built around stable provider lanes, shadow-driven elevation and honest reset-state progress.
---

<!-- Token values below are mirrored from the shipped implementation in
     `src/styles/tokens.css`, which is the single source of truth; component names map to
     `src/components/`. Window topology includes the implemented ADR-0011 structure; desktop
     validation is still pending. The semantic-name ↔ CSS-variable table lives in
     `docs/设计方向与状态规范.md` §3.4 and must be changed alongside any token rename. -->

# Design System: CC Trace

## Overview

**Creative North Star: "HeroUI"** (see [ADR-0012](docs/决策/ADR-0012-视觉方向改为HeroUI风格.md), fixed values per [ADR-0023](docs/决策/ADR-0023-视觉修复方向定稿.md))

CC Trace adopts the HeroUI visual language: shadow-driven elevation, larger corner radii, and semantic color that appears more often and more saturated than a purely restrained system. Provider lanes still read from identity to remaining capacity to reset endpoint in one path, and Codex/Claude Code keep a stable order — the information grammar is unchanged, only the shape, elevation and color language changed.

macOS and Windows share the same information grammar while their shells, menu behavior and window materials remain platform-appropriate.

**Key Characteristics:**

- Stable Codex → Claude Code lanes.
- Risk-led hierarchy without reordering content.
- Cards carry shadow at rest; hairline borders stay for definition, especially in dark mode.
- 12–14px corner radii, pill-shaped quota progress tracks.
- System UI typography paired with platform-aware monospaced numerals.
- Honest state combinations: activity, freshness and failure reason remain distinguishable.

## Colors

The palette follows HeroUI's default semantic scale. Canvas and card surfaces stay close in value, but not identical — the shipped values deepen the canvas (`#F4F4F5` light / `#0E0E11` dark) and strengthen the resting lane shadow so elevation reads through both shadow and a modest color difference (ADR-0023).

Values are light / dark pairs as shipped in `src/styles/tokens.css`.

### Primary

- **HeroUI Primary** `--action-primary` `#006FEE` / `#338EF7`: interactive controls, keyboard focus and selected settings. It must never stand in for success.

### Secondary

- **Success** `--status-success` `#17C964` / `#45D483`: current successful data, completed checks, and the healthy (>50%) remaining-quota band. The ok band is green permanently — "fresh and healthy" must not collide with the grey of stale data; see ADR-0023.
- **Warning** `--status-warning` `#F5A524` / `#FBBF24`: stale data, rate limits and quota between 20% and 50% remaining.
- **Low** `--status-low` `#F3730E` / `#FF8A3D`: under 20% of a quota window remaining. HeroUI has no official band between warning and danger; this value is adopted by ADR-0017.
- **Danger** `--status-error` `#F31260` / `#F5455C`: unrecoverable or credential/protocol errors, and a fully consumed quota window. It is not used for ordinary absence.

### Neutral

- **Canvas** `--surface-primary` `#F4F4F5` / `#0E0E11`: application background. One step darker than the white card so elevation reads from color difference as well as shadow.
- **Surface** `--surface-raised` `#FFFFFF` / `#1C1C1F`: provider lanes, fields and quiet grouped content — intentionally close to the canvas value; shadow and the modest canvas gap do the separating.
- **Ink** `--text-primary` `#11181C` / `#ECEDEE`: primary text and high-value numbers.
- **Muted** `--text-secondary` `#71717A` / `#A1A1AA`: labels, timestamps and supporting explanations.
- **Hairline** `--border-subtle` `#E4E4E7` / `#2E2E33`: card borders, list and dense-area structural separators.
- **Track** `--track-background` `#EDEDF0` / `#2A2A2E`: the unfilled groove of the quota progress bar.

Appearance is driven by `data-appearance` on the root element: absent or `system` follows `prefers-color-scheme`; `light` and `dark` override it.

**The Semantic Color Rule.** Color only appears when it identifies interaction, freshness, warning or failure; it never decorates headings or fills arbitrary tiles.

**The Dual Evidence Rule.** Every status color is paired with a word, symbol or readable explanation.

**The Two Tones Rule.** Two independent tone dimensions coexist and neither substitutes for the other. *Availability* tone (`src/lib/status.ts`) drives the explanation banner, the status dot and the status word. *Remaining-quota* tone (`src/lib/quotaTone.ts`) drives the percentage reading and the progress fill, purely from the remaining percentage. "Credentials expired" and "only 3% left" are different facts and must both be readable at once. A snapshot that is not current downgrades the remaining-quota tone to neutral — stale data never claims in vivid color that quota is tight.

## Typography

**Display Font:** platform system UI stack.

**Body Font:** platform system UI stack.

**Label/Mono Font:** `SFMono-Regular` on macOS, `Cascadia Mono` on Windows, then the shared `ui-monospace` fallback.

**Character:** The system face keeps controls native and readable; the utility face makes percentages, reset times and status metadata feel measured without turning the product into a terminal imitation.

### Hierarchy

- **Window title:** compact system-weight heading; never an oversized marketing headline.
- **Provider title:** medium weight with enough contrast to anchor a stable lane.
- **Quota value:** strong but not theatrical; tabular numerals prevent refresh-time movement.
- **Body:** plain-language explanations with comfortable leading and short line lengths.
- **Utility label:** small, slightly tracked metadata for window names, refresh times and state vocabulary.
- **Compact cost:** follow ccbar’s restrained inline hierarchy: 10px period label, 11px medium
  amount, 4px internal gap and 10px between periods. A 9.5px muted `花费 / Cost` label sits below;
  scanning adds only a 10px muted loading indicator after the week amount.

**The Numeric Stability Rule.** Every changing percentage, countdown, reset time and compact cost uses tabular numerals and a width that does not shift nearby controls.

**The Platform Voice Rule.** Use the platform system face for UI; do not force macOS typography onto Windows or add a web font dependency only for personality.

## Layout

The core spatial grammar is a stack of stable provider lanes. Each lane reads from identity to remaining capacity to reset endpoint, then to freshness and recovery. Compact surfaces collapse details but preserve that order; larger windows add explanation below the same progress bar instead of changing the model.

Use a four-point spacing rhythm. Dense control groups prefer 8–16 units; major content transitions use 24–32 units. Provider order never changes after refresh. The main window is driven by a 176px grouped sidebar (views group / data source group / settings pinned bottom, ADR-0024): usage, conversations and timeline share the same window and the data source selection is a global in-memory filter across views; settings hides the data source group. Settings content stays a narrow 640px reading column.

Transient surfaces originate from their system trigger. The macOS compact panel is anchored to the Menu Bar icon; the Windows compact panel appears adjacent to the Tray. The main window holds usage, timeline, conversations and settings under one platform title bar, navigated by the grouped sidebar; onboarding remains a separate window.

**The Stable Lane Rule.** Risk changes emphasis, never spatial order.

**The One Reading Path Rule.** A quota percentage and its reset time must be visible in the same horizontal or immediate vertical sequence.

## Elevation & Depth

Depth is expressive, not just structural. Provider lanes carry a soft shadow (`--shadow-lane`) at rest — elevation does not wait for hover or interaction. The lane shadow was strengthened in ADR-0023 so it reads at rest, and the canvas was deepened one step so a white card separates from the page even where shadow is subtle. A hairline border stays on every card for definition, especially where shadow alone is too faint on a dark background. The compact panel may use the operating system's translucent or acrylic material at the outer shell, but its content surfaces remain solid enough for dependable contrast.

Transient panels use a stronger layered ambient shadow (`--shadow-panel`) than resting lanes, so the floating compact panel reads as a higher layer than the cards inside the main window. Never stack translucent surfaces.

## Shapes

The double-C brand geometry supplies controlled arcs and round endpoints, not a license to make every object circular. Provider lanes and fields use a generous 12–14px radius; nested radii remain concentric. Buttons and inputs are rounded rectangles at 12px — 8px for the small icon buttons in the compact panel header, so a 32px square does not read as over-rounded — not pills, except for true compact state labels whose text length is bounded, such as the plan chip on a provider lane.

Quota progress uses a pill-shaped track, not a straight rail. It does not become a circular gauge, speedometer or decorative waveform.

## Implementation

| Element | Component | Notes |
|---|---|---|
| Quota progress | `src/components/QuotaProgress.vue` | Rounded pill track. Primary windows stack a large reading above a full-width bar; secondary windows are a single row. Fill and reading are coloured by remaining quota, not by availability |
| Usage cost readout | `src/components/UsageCostReadout.vue` | Compact-only today / this-week API-equivalent cost beside the primary reset reading, with a muted `花费 / Cost` label below. Scanning uses a small muted loading indicator after the amounts. A priced subtotal is shown without a lower-bound suffix or unpriced notice; never-indexed, wholly unpriced and unavailable values use `—`, never a false `$0` |
| Provider lane | `src/components/ProviderLane.vue` | Shadow-driven card, no left status spine. Header is name + plan chip + masked account; secondary windows sit under a dashed divider |
| Overall signal | `src/components/OverallSignal.vue` | Raises the weight of the highest-risk provider without reordering anything. Both surfaces use a stable surface name as the title — never a status sentence; a status dot with an accessible name carries the overall state. All status detail lives in a dot tooltip (hover or keyboard focus): one entry per affected provider with status, next step and backoff countdown; cards carry quota only |
| Refresh icon | `src/components/RefreshIcon.vue` | Spins only during a real refresh; static under reduced motion |
| Main window sidebar | `src/components/MainSidebar.vue` | 176px grouped sidebar: views group, data source group (all / Codex / Claude Code / Pi / OpenCode) and settings pinned bottom. Data source selection is a global in-memory filter; settings hides the data source group (ADR-0024) |
| Menu bar badge | `src-tauri/src/platform/menubar_badge.rs` | macOS only: provider marks and five-hour percentages rendered into a single-colour template bitmap (ADR-0017) |

Spacing follows a four-point rhythm (`--space-1` … `--space-8` = 4/8/12/16/20/24/32). Radii are `--radius-control` 8px, `--radius-small` 12px, `--radius-medium` 14px, `--radius-shell` 16px. Motion uses `--motion-fast` 140ms, `--motion-base` 200ms, `--motion-panel` 320ms with a single `--ease-out` curve and no overshoot.

Each tone dimension has exactly one implementation and components never re-derive either: the three status dimensions map to copy keys, availability tone and progress treatment in `src/lib/status.ts`; remaining percentage maps to a quota band in `src/lib/quotaTone.ts`.

Desktop controls keep a 40 × 40 minimum target, with one exception: the four icon buttons in the compact panel header are 32 × 32, which still meets WCAG 2.5.8 AA. At 380px wide the panel cannot hold four 40px buttons. The usage loading indicator is status, not a control.

## Do's and Don'ts

### Do:

- **Do** keep Codex and Claude Code in stable lanes with the same field order.
- **Do** show remaining capacity, reset time and freshness as one coherent reading path.
- **Do** use system typography, tabular numerals and precise optical alignment.
- **Do** let shadow — plus a modest canvas/white gap — carry elevation for lanes and panels.
- **Do** pair every semantic color with readable status language.
- **Do** preserve old snapshot values during refresh and visibly mark their freshness.

### Don't:

- **Don't** use gradients, glass stacks, colored icon tiles or badge/pill spam.
- **Don't** dynamically reorder Providers by risk.
- **Don't** use circular gauges, charts or animated counters for static quota facts.
- **Don't** revert to a straight-rail progress shape or reintroduce tick marks — that direction was explicitly superseded by [ADR-0012](docs/决策/ADR-0012-视觉方向改为HeroUI风格.md).
