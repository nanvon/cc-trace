---
name: CC Trace
description: A restrained adaptive desktop system built around stable provider lanes and honest reset-state rails.
---

<!-- Updated 2026-07-25 from the shipped implementation. Token values below are mirrored from
     `src/styles/tokens.css`, which is the single source of truth; component names map to
     `src/components/`. The semantic-name ↔ CSS-variable table lives in
     `docs/设计方向与状态规范.md` §3.4 and must be changed alongside any token rename. -->

# Design System: CC Trace

## Overview

**Creative North Star: "Reset Rail"**

CC Trace should feel like a precise developer run sheet: stable provider lanes, legible quota windows and a clear relationship between remaining capacity, data freshness and reset time. The system refuses the category-default dashboard made from interchangeable rounded cards and decorative charts. Its memorable device is a horizontal reset rail that keeps the remaining value and reset endpoint in one reading path.

The interface operates frequently and briefly. Expression comes from the rhythm of rails, restrained use of the double-C geometry and excellent numeric typography, not from spectacle. macOS and Windows share the same information grammar while their shells, menu behavior and window materials remain platform-appropriate.

**Key Characteristics:**

- Stable Codex → Claude Code lanes.
- Risk-led hierarchy without reordering content.
- Cool mineral neutrals with one interaction blue and semantic status colors.
- System UI typography paired with platform-aware monospaced numerals.
- Flat, structured content with elevation reserved for transient windows and focus.
- Honest state combinations: activity, freshness and failure reason remain distinguishable.

## Colors

The palette is restrained and cool rather than warm or atmospheric. Large surfaces stay neutral; interaction blue and semantic status colors appear only where they carry meaning.

Values are light / dark pairs as shipped in `src/styles/tokens.css`.

### Primary

- **Trace Blue** `--action-primary` `#2F67D8` / `#7BA3FF`: interactive controls, keyboard focus and selected settings. It must never stand in for success.

### Secondary

- **Signal Green** `--status-success` `#247A55` / `#63C99A`: current successful data and completed checks.
- **Reset Amber** `--status-warning` `#B66A12` / `#F0AA52`: stale data, rate limits and approaching quota risk.
- **Fault Red** `--status-error` `#B93636` / `#FF7B75`: unrecoverable or credential/protocol errors. It is not used for ordinary absence.

### Neutral

- **Mineral Canvas** `--surface-primary` `#F2F5F3` / `#111513`: application background.
- **Instrument Surface** `--surface-raised` `#FFFFFF` / `#1A201D`: provider lanes, fields and quiet grouped content.
- **Carbon Ink** `--text-primary` `#161A18` / `#EEF3F0`: primary text and high-value numbers.
- **Graphite Muted** `--text-secondary` `#66706B` / `#9DA8A2`: labels, timestamps and supporting explanations.
- **Hairline** `--border-subtle` `#DCE2DE` / `#303934`: structural separators in dense areas.
- **Rail Track** `--track-background` `#E7EBE8` / `#2B332F`: the unfilled groove of the reset rail.

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

The core spatial grammar is a stack of stable provider lanes. Each lane reads from identity to remaining capacity to reset endpoint, then to freshness and recovery. Compact surfaces collapse details but preserve that order; larger windows add explanation below the same rail instead of changing the model.

Use a four-point spacing rhythm. Dense control groups prefer 8–16 units; major content transitions use 24–32 units. Provider order never changes after refresh. Main-window content remains a single page and becomes one column before text or controls collide.

Transient surfaces originate from their system trigger. The macOS compact panel is anchored to the Menu Bar icon; the Windows compact panel appears adjacent to the Tray. Main, settings and onboarding windows use the platform title bar and focus model.

**The Stable Lane Rule.** Risk changes emphasis, never spatial order.

**The One Reading Path Rule.** A quota percentage and its reset time must be visible in the same horizontal or immediate vertical sequence.

## Elevation & Depth

Depth is structural, not decorative. The compact panel may use the operating system's translucent or acrylic material at the outer shell, but its content surfaces remain solid enough for dependable contrast. Main and settings windows use tonal separation and hairlines before shadows.

Transient panels use one layered ambient shadow and a subtle neutral ring. Interactive cards do not float at rest; hover may strengthen the ring without moving the layout. Never stack translucent surfaces.

**The One Material Layer Rule.** Platform material belongs to the transient shell only; nested provider lanes remain solid.

## Shapes

The double-C brand geometry supplies controlled arcs and round endpoints, not a license to make every object circular. Provider lanes and fields use restrained corners; nested radii remain concentric. Buttons and inputs are rounded rectangles, not pills, except for true compact state labels whose text length is bounded.

The reset rail uses a straight track with softened endpoints and a clear terminal marker. It does not become a circular gauge, speedometer or decorative waveform.

## Implementation

| Element | Component | Notes |
|---|---|---|
| Reset rail | `src/components/ResetRail.vue` | 3px straight track (`--rail-height`), 1.5px softened ends, hairline ticks at 25/50/75%, a terminal marker at the right end followed immediately by the reset time |
| Provider lane | `src/components/ProviderLane.vue` | 2px status spine on the left, neutral when `ready` and coloured only on risk |
| Overall signal | `src/components/OverallSignal.vue` | Raises the weight of the highest-risk provider without reordering anything |
| Status explanation | `src/components/StatusExplanation.vue` | One line in the compact panel, title / impact / next step in the main window |
| Refresh icon | `src/components/RefreshIcon.vue` | Spins only during a real refresh; static under reduced motion |

Spacing follows a four-point rhythm (`--space-1` … `--space-8` = 4/8/12/16/20/24/32). Radii are `--radius-small` 7px, `--radius-medium` 11px, `--radius-shell` 16px. Motion uses `--motion-fast` 140ms, `--motion-base` 200ms, `--motion-panel` 320ms with a single `--ease-out` curve and no overshoot.

The mapping from the three status dimensions to copy keys, tone and rail treatment exists in exactly one place: `src/lib/status.ts`. Components never re-derive status from `availability` themselves.

## Do's and Don'ts

### Do:

- **Do** keep Codex and Claude Code in stable lanes with the same field order.
- **Do** show remaining capacity, reset time and freshness as one coherent reading path.
- **Do** use system typography, tabular numerals and precise optical alignment.
- **Do** reserve platform material for transient shell hierarchy.
- **Do** pair every semantic color with readable status language.
- **Do** preserve old snapshot values during refresh and visibly mark their freshness.

### Don't:

- **Don't** build a generic dashboard grid of interchangeable cards.
- **Don't** use gradients, glow, glass stacks or oversized shadows as brand substitutes.
- **Don't** dynamically reorder Providers by risk.
- **Don't** use circular gauges, charts or animated counters for static quota facts.
- **Don't** fill the interface with badges, pills, uppercase labels or icon tiles.
- **Don't** use success green for neutral live data everywhere; most healthy content remains neutral.
