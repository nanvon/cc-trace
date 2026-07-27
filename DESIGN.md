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

**Creative North Star: "HeroUI"** (see [ADR-0012](docs/决策/ADR-0012-视觉方向改为HeroUI风格.md))

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

The palette follows HeroUI's default semantic scale. Background and card surfaces are deliberately close in value; elevation is expressed through shadow rather than color contrast.

Values are light / dark pairs as shipped in `src/styles/tokens.css`.

### Primary

- **HeroUI Primary** `--action-primary` `#006FEE` / `#338EF7`: interactive controls, keyboard focus and selected settings. It must never stand in for success.

### Secondary

- **Success** `--status-success` `#17C964` / `#45D483`: current successful data and completed checks.
- **Warning** `--status-warning` `#F5A524` / `#FBBF24`: stale data, rate limits and approaching quota risk.
- **Danger** `--status-error` `#F31260` / `#F5455C`: unrecoverable or credential/protocol errors. It is not used for ordinary absence.

### Neutral

- **Canvas** `--surface-primary` `#FAFAFA` / `#000000`: application background.
- **Surface** `--surface-raised` `#FFFFFF` / `#18181B`: provider lanes, fields and quiet grouped content — intentionally close to the canvas value; shadow does the separating.
- **Ink** `--text-primary` `#11181C` / `#ECEDEE`: primary text and high-value numbers.
- **Muted** `--text-secondary` `#71717A` / `#A1A1AA`: labels, timestamps and supporting explanations.
- **Hairline** `--border-subtle` `#E4E4E7` / `#27272A`: card borders, list and dense-area structural separators.
- **Track** `--track-background` `#F4F4F5` / `#27272A`: the unfilled groove of the quota progress bar.

Appearance is driven by `data-appearance` on the root element: absent or `system` follows `prefers-color-scheme`; `light` and `dark` override it.

**The Semantic Color Rule.** Color only appears when it identifies interaction, freshness, warning or failure; it never decorates headings or fills arbitrary tiles.

**The Dual Evidence Rule.** Every status color is paired with a word, symbol or readable explanation.

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

**The Numeric Stability Rule.** Every changing percentage, countdown and reset time uses tabular numerals and a width that does not shift nearby controls.

**The Platform Voice Rule.** Use the platform system face for UI; do not force macOS typography onto Windows or add a web font dependency only for personality.

## Layout

The core spatial grammar is a stack of stable provider lanes. Each lane reads from identity to remaining capacity to reset endpoint, then to freshness and recovery. Compact surfaces collapse details but preserve that order; larger windows add explanation below the same progress bar instead of changing the model.

Use a four-point spacing rhythm. Dense control groups prefer 8–16 units; major content transitions use 24–32 units. Provider order never changes after refresh. The main window's quota view remains a single page and becomes one column before text or controls collide; settings replaces it as a narrow secondary view rather than appearing beside it.

Transient surfaces originate from their system trigger. The macOS compact panel is anchored to the Menu Bar icon; the Windows compact panel appears adjacent to the Tray. The main window contains quota and settings views under one platform title bar and focus model; onboarding remains a separate window.

**The Stable Lane Rule.** Risk changes emphasis, never spatial order.

**The One Reading Path Rule.** A quota percentage and its reset time must be visible in the same horizontal or immediate vertical sequence.

## Elevation & Depth

Depth is expressive, not just structural. Provider lanes carry a soft shadow (`--shadow-lane`) at rest — elevation does not wait for hover or interaction. A hairline border stays on every card for definition, especially where shadow alone is too faint on a dark background. The compact panel may use the operating system's translucent or acrylic material at the outer shell, but its content surfaces remain solid enough for dependable contrast.

Transient panels use a stronger layered ambient shadow (`--shadow-panel`) than resting lanes, so the floating compact panel reads as a higher layer than the cards inside the main window. Never stack translucent surfaces.

## Shapes

The double-C brand geometry supplies controlled arcs and round endpoints, not a license to make every object circular. Provider lanes and fields use a generous 12–14px radius; nested radii remain concentric. Buttons and inputs are rounded rectangles at 12px, not pills, except for true compact state labels whose text length is bounded.

Quota progress uses a pill-shaped track, not a straight rail. It does not become a circular gauge, speedometer or decorative waveform.

## Implementation

| Element | Component | Notes |
|---|---|---|
| Quota progress | `src/components/QuotaProgress.vue` | Rounded pill track, neutral fill at `ready`, coloured only on `warning`/`critical` |
| Provider lane | `src/components/ProviderLane.vue` | Shadow-driven card, no left status spine; risk changes text color only |
| Overall signal | `src/components/OverallSignal.vue` | Raises the weight of the highest-risk provider without reordering anything |
| Status explanation | `src/components/StatusExplanation.vue` | Tint-background alert in both the compact panel (one line) and the main window (title / impact / next step) |
| Refresh icon | `src/components/RefreshIcon.vue` | Spins only during a real refresh; static under reduced motion |

Spacing follows a four-point rhythm (`--space-1` … `--space-8` = 4/8/12/16/20/24/32). Radii are `--radius-small` 12px, `--radius-medium` 14px, `--radius-shell` 16px. Motion uses `--motion-fast` 140ms, `--motion-base` 200ms, `--motion-panel` 320ms with a single `--ease-out` curve and no overshoot.

The mapping from the three status dimensions to copy keys, tone and progress treatment exists in exactly one place: `src/lib/status.ts`. Components never re-derive status from `availability` themselves.

## Do's and Don'ts

### Do:

- **Do** keep Codex and Claude Code in stable lanes with the same field order.
- **Do** show remaining capacity, reset time and freshness as one coherent reading path.
- **Do** use system typography, tabular numerals and precise optical alignment.
- **Do** let shadow — not just color or borders — carry elevation for lanes and panels.
- **Do** pair every semantic color with readable status language.
- **Do** preserve old snapshot values during refresh and visibly mark their freshness.

### Don't:

- **Don't** use gradients, glass stacks, colored icon tiles or badge/pill spam.
- **Don't** dynamically reorder Providers by risk.
- **Don't** use circular gauges, charts or animated counters for static quota facts.
- **Don't** revert to a straight-rail progress shape or reintroduce tick marks — that direction was explicitly superseded by [ADR-0012](docs/决策/ADR-0012-视觉方向改为HeroUI风格.md).
