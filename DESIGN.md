---
name: CC Trace
description: A HeroUI-styled adaptive desktop system built around stable provider lanes, hairline-outlined flush cards and honest reset-state progress.
---

<!-- Token values below are mirrored from the shipped implementation in
     `src/styles/tokens.css`, which is the single source of truth; component names map to
     `src/components/`. Window topology includes the implemented ADR-0011 structure; desktop
     validation is still pending. The semantic-name ↔ CSS-variable table lives in
     `docs/设计方向与状态规范.md` §3.4 and must be changed alongside any token rename. -->

# Design System: CC Trace

## Overview

**Creative North Star: "HeroUI, flush-card layering"** (see [ADR-0012](docs/决策/ADR-0012-视觉方向改为HeroUI风格.md), fixed values per [ADR-0023](docs/决策/ADR-0023-视觉修复方向定稿.md), layering per [ADR-0028](docs/决策/ADR-0028-全部表面改为贴合式弱化卡片层级.md))

CC Trace adopts the HeroUI visual language — larger corner radii and semantic color that appears more often and more saturated than a purely restrained system — with the card hierarchy mechanism revised to cc-bar's flush-card approach: cards rest **without shadows**, their boundaries drawn by a hairline stroke (`--border-hairline`), and hierarchy is carried by type. The KPI row gives the usage page a single visual anchor: one large numeric scale, everything else recedes below 12.5px. Provider lanes still read from identity to remaining capacity to reset endpoint in one path, and Codex/Claude Code keep a stable order — the information grammar is unchanged, only the shape, stroke and color language changed.

macOS and Windows share the same information grammar while their shells, menu behavior and window materials remain platform-appropriate.

**Key Characteristics:**

- Stable Codex → Claude Code lanes.
- Risk-led hierarchy without reordering content.
- Cards rest flush: no resting shadow, hairline border only; shadow (`--shadow-panel`) is reserved for floating surfaces (compact panel shell, status-dot tooltip).
- 12–14px corner radii, pill-shaped quota progress tracks.
- System UI typography on a four-step scale — 22px numerals / 13px titles / 12.5px body / 11px labels — with tabular numerals.
- Honest state combinations: activity, freshness and failure reason remain distinguishable.

## Colors

The palette follows HeroUI's default semantic scale. Canvas and card surfaces stay close in value — the shipped values deepen the canvas (`#F4F4F5` light / `#0E0E11` dark) so a white card separates from the page by tone; card boundaries are drawn with a hairline stroke (`--border-hairline`) instead of a resting shadow (ADR-0028).

Values are light / dark pairs as shipped in `src/styles/tokens.css`.

### Primary

- **HeroUI Primary** `--action-primary` `#006FEE` / `#338EF7`: interactive controls, keyboard focus, selected settings and the **selected sidebar item** (blue fill with `--action-on-primary` white text, ADR-0028). It must never stand in for success.

### Secondary

- **Success** `--status-success` `#17C964` / `#45D483`: current successful data, completed checks, and the healthy (>50%) remaining-quota band. The ok band is green permanently — "fresh and healthy" must not collide with the grey of stale data; see ADR-0023.
- **Warning** `--status-warning` `#F5A524` / `#FBBF24`: stale data, rate limits and quota between 20% and 50% remaining.
- **Low** `--status-low` `#F3730E` / `#FF8A3D`: under 20% of a quota window remaining. HeroUI has no official band between warning and danger; this value is adopted by ADR-0017.
- **Danger** `--status-error` `#F31260` / `#F5455C`: unrecoverable or credential/protocol errors, and a fully consumed quota window. It is not used for ordinary absence.

### Neutral

- **Canvas** `--surface-primary` `#F4F4F5` / `#0E0E11`: application background. One step darker than the white card so elevation reads from color difference as well as stroke.
- **Surface** `--surface-raised` `#FFFFFF` / `#1C1C1F`: provider lanes, fields and quiet grouped content — intentionally close to the canvas value; the hairline stroke and the modest canvas gap do the separating.
- **Hairline** `--border-subtle` `#E4E4E7` / `#2E2E33`: list and dense-area structural separators.
- **Card hairline** `--border-hairline` `rgb(16 16 20 / 8%)` / `rgb(255 255 255 / 10%)`: the 1px card boundary stroke that replaces the resting shadow (ADR-0028). Cards use this token, never a hand-mixed border.
- **Ink** `--text-primary` `#11181C` / `#ECEDEE`: primary text and high-value numbers.
- **Muted** `--text-secondary` `#71717A` / `#A1A1AA`: labels, timestamps and supporting explanations.
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

A four-step type scale carries the hierarchy (ADR-0028): **22px numerals** are the only large type on a page (KPI row; the compact panel's 32px primary reading is the lone exception), **13px medium** titles, **12.5px regular** body, **11px muted** labels, with 10.5px for small notes. No intermediate odd sizes like 11.5px.

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

Use a four-point spacing rhythm. Dense control groups prefer 8–16 units; major content transitions use 20–32 units. Provider order never changes after refresh. The main window is driven by a 176px grouped sidebar (views group / data source group / settings pinned bottom, ADR-0024): usage, conversations and timeline share the same window and the data source selection is a global in-memory filter across views; settings hides the data source group. Settings content stays a narrow 640px reading column.

The usage page opens with a **KPI row** (total tokens / total spend / per-provider spend, one flush card each) that anchors the page's hierarchy — the single 22px numeric scale everything else recedes beneath (ADR-0028). Section headings are plain 13px medium text without decorative rules; sections sit 20px apart.

Transient surfaces originate from their system trigger. The macOS compact panel is anchored to the Menu Bar icon; the Windows compact panel appears adjacent to the Tray. The main window holds usage, timeline, conversations and settings under one platform title bar, navigated by the grouped sidebar; onboarding remains a separate window.

**The Stable Lane Rule.** Risk changes emphasis, never spatial order.

**The One Reading Path Rule.** A quota percentage and its reset time must be visible in the same horizontal or immediate vertical sequence.

## Elevation & Depth

Depth is structural, not decorative — and it is carried by **stroke, tone and type, not resting shadows** (ADR-0028). Cards sit flush on the canvas: their boundary is the 1px hairline stroke (`--border-hairline`), their separation from the page comes from the modest canvas gap, and their hierarchy comes from the type scale (the KPI numerals are the page's only large type). A resting shadow on every card makes all of them claim the same emphasis, which reads as no hierarchy at all; the hairline is a quiet boundary that lets information speak.

Shadows are reserved for floating surfaces: the compact panel shell and the status-dot tooltip use `--shadow-panel`, which reads as a higher layer than the cards inside the main window. The compact panel may use the operating system's translucent or acrylic material at the outer shell, but its content surfaces remain solid enough for dependable contrast. Dark mode keeps a faint card stroke as a definition backstop. Never stack translucent surfaces.

## Shapes

The double-C brand geometry supplies controlled arcs and round endpoints, not a license to make every object circular. Provider lanes and fields use a generous 12–14px radius; nested radii remain concentric. Buttons and inputs are rounded rectangles at 12px — 8px for the small icon buttons in the compact panel header, so a 32px square does not read as over-rounded — not pills, except for true compact state labels whose text length is bounded, such as the plan chip on a provider lane.

Quota progress uses a pill-shaped track, not a straight rail. It does not become a circular gauge, speedometer or decorative waveform.

## Implementation

| Element | Component | Notes |
|---|---|---|
| Quota progress | `src/components/QuotaProgress.vue` | Rounded pill track. Primary windows stack a large reading above a full-width bar; secondary windows are a single row. Fill and reading are coloured by remaining quota, not by availability |
| Usage cost readout | `src/components/UsageCostReadout.vue` | Compact-only today / this-week API-equivalent cost beside the primary reset reading, with a muted `花费 / Cost` label below. Scanning uses a small muted loading indicator after the amounts. A priced subtotal is shown without a lower-bound suffix or unpriced notice; never-indexed, wholly unpriced and unavailable values use `—`, never a false `$0` |
| Provider lane | `src/components/ProviderLane.vue` | Flush hairline card, no left status spine, no resting shadow. Header is name + plan chip + masked account; secondary windows sit under a dashed divider |
| Overall signal | `src/components/OverallSignal.vue` | Raises the weight of the highest-risk provider without reordering anything. Both surfaces use a stable surface name as the title — never a status sentence; a status dot with an accessible name carries the overall state. All status detail lives in a dot tooltip (hover or keyboard focus): one entry per affected provider with status, next step and backoff countdown; cards carry quota only |
| Refresh icon | `src/components/RefreshIcon.vue` | Spins only during a real refresh; static under reduced motion |
| Main window sidebar | `src/components/MainSidebar.vue` | 176px grouped sidebar: views group, data source group (all / Codex / Claude Code / Pi / OpenCode) and settings pinned bottom. Selected item is a blue fill with white text (ADR-0028). Data source selection is a global in-memory filter; settings hides the data source group (ADR-0024) |
| KPI row | `src/views/MainView.vue` | The page's visual anchor: one flush card each for total tokens, total spend and per-provider spend. 22px semibold tabular numerals are the page's only large type; 11px muted labels; provider cards carry a 7px squircle mark (ADR-0028) |
| Menu bar badge | `src-tauri/src/platform/menubar_badge.rs` | macOS only: provider marks and five-hour percentages rendered into a single-colour template bitmap (ADR-0017) |

Spacing follows a four-point rhythm (`--space-1` … `--space-8` = 4/8/12/16/20/24/32). Radii are `--radius-control` 8px, `--radius-small` 12px, `--radius-medium` 14px, `--radius-shell` 16px. Motion uses `--motion-fast` 140ms, `--motion-base` 200ms, `--motion-panel` 320ms with a single `--ease-out` curve and no overshoot.

Each tone dimension has exactly one implementation and components never re-derive either: the three status dimensions map to copy keys, availability tone and progress treatment in `src/lib/status.ts`; remaining percentage maps to a quota band in `src/lib/quotaTone.ts`.

Desktop controls keep a 40 × 40 minimum target, with one exception: the four icon buttons in the compact panel header are 32 × 32, which still meets WCAG 2.5.8 AA. At 380px wide the panel cannot hold four 40px buttons. The usage loading indicator is status, not a control.

## Do's and Don'ts

### Do:

- **Do** keep Codex and Claude Code in stable lanes with the same field order.
- **Do** show remaining capacity, reset time and freshness as one coherent reading path.
- **Do** use system typography, tabular numerals and precise optical alignment.
- **Do** let the hairline stroke — plus a modest canvas/white gap — carry card boundaries; reserve shadows for floating surfaces (`--shadow-panel`).
- **Do** keep the KPI numerals as the page's only large type; let everything else recede below 12.5px.
- **Do** pair every semantic color with readable status language.
- **Do** preserve old snapshot values during refresh and visibly mark their freshness.

### Don't:

- **Don't** use gradients, glass stacks, colored icon tiles or badge/pill spam.
- **Don't** add resting shadows to cards (`--shadow-lane` was removed by ADR-0028); a box of six floating cards is the Dashboard-wall look the direction rejects.
- **Don't** dynamically reorder Providers by risk.
- **Don't** use circular gauges, charts or animated counters for static quota facts.
- **Don't** revert to a straight-rail progress shape or reintroduce tick marks — that direction was explicitly superseded by [ADR-0012](docs/决策/ADR-0012-视觉方向改为HeroUI风格.md).
