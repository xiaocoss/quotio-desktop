# Floating Window Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create a complete, Image 2-generated design handoff for Quotio's compact floating window under `设计图/悬浮窗/` without changing application code.

**Architecture:** Treat the generated PNG as the visual target, `design-tokens.json` as the implementation contract, and `floating-window-reference.html` as a standalone executable reference that recreates the design with CSS and SVG. Shared SVG assets live under `assets/`; the HTML renders dynamic-looking quota states with semantic markup and CSS rather than embedding the PNG.

**Tech Stack:** Image 2 via the built-in image generation tool, HTML5, CSS custom properties, inline/local SVG, JSON, PowerShell validation, Chromium/Playwright screenshot verification when available.

---

## File Map

- Create: `设计图/悬浮窗/README.md` — delivery index, dimensions, implementation guidance, and visual direction.
- Create: `设计图/悬浮窗/imagegen-prompt.md` — final structured Image 2 prompt and source-image role.
- Create: `设计图/悬浮窗/design-tokens.json` — canvas, colors, typography, spacing, radii, shadows, and component metrics.
- Create: `设计图/悬浮窗/floating-window-redesign.png` — selected high-fidelity Image 2 output.
- Create: `设计图/悬浮窗/floating-window-reference.html` — standalone browser reference with fixed header/footer and a scrolling account list.
- Create: `设计图/悬浮窗/assets/floating-window-bg.svg` — warm paper field with restrained blue/mint glows.
- Create: `设计图/悬浮窗/assets/card-noise.svg` — subtle reusable card texture.
- Create: `设计图/悬浮窗/assets/floating-window-icons.svg` — SVG symbols for stop, refresh, open, exit, clock, calendar, and status.
- Create: `设计图/悬浮窗/assets/quota-indicators.svg` — reusable health-dot and quota-bar visual reference.

### Task 1: Establish the design contract and generation prompt

**Files:**
- Create: `设计图/悬浮窗/imagegen-prompt.md`
- Create: `设计图/悬浮窗/design-tokens.json`
- Create: `设计图/悬浮窗/README.md`

- [ ] **Step 1: Create the directory skeleton**

Run:

```powershell
New-Item -ItemType Directory -Force '设计图/悬浮窗/assets'
```

Expected: `设计图/悬浮窗/assets` exists and no existing design package is overwritten.

- [ ] **Step 2: Write the structured Image 2 prompt**

The prompt must use `ui-mockup`, label the supplied screenshot as `Image 1: structural and content reference`, preserve the narrow floating-window form, fixed header, wrapped provider tabs, account cards, health dots, Session/Weekly progress, scrollbar, and three footer actions. It must request warm ivory surfaces, crisp readable typography, implementation-friendly CSS/SVG styling, and prohibit phone hardware, desktop scenery, dark mode, marketing layout, 3D elements, watermarks, and extra product features.

- [ ] **Step 3: Write concrete design tokens**

Use this top-level JSON contract:

```json
{
  "meta": {"name": "quotio-floating-window-redesign", "target": "desktop-floating-window", "canvas": {}},
  "colors": {},
  "typography": {},
  "spacing": {},
  "radii": {},
  "shadows": {},
  "components": {"window": {}, "header": {}, "tabs": {}, "accountCard": {}, "progress": {}, "footer": {}}
}
```

Set the logical window to `360 × 960`, card width to the available inner width, account card height to `150–168px`, progress height to `6px`, footer row height to `48px`, and health-dot diameter to `7px`.

- [ ] **Step 4: Write the package README**

Include exactly these sections: `文件说明`, `开发落地建议`, `关键尺寸`, and `视觉方向`. State that the package does not modify business code and that dynamic quota bars/health dots should use live data rather than the decorative SVG.

- [ ] **Step 5: Validate JSON and required text**

Run:

```powershell
Get-Content -Raw '设计图/悬浮窗/design-tokens.json' | ConvertFrom-Json | Out-Null
rg -n "文件说明|开发落地建议|关键尺寸|视觉方向" '设计图/悬浮窗/README.md'
rg -n "Use case: ui-mockup|Image 1|Constraints:|Avoid:" '设计图/悬浮窗/imagegen-prompt.md'
```

Expected: JSON parsing succeeds; each `rg` command finds all required headings or prompt labels.

### Task 2: Generate and persist the Image 2 visual target

**Files:**
- Create: `设计图/悬浮窗/floating-window-redesign.png`
- Reference: `C:/Users/lilin/AppData/Local/Temp/codex-clipboard-c994f8cb-e870-4d0b-8048-a7f2e0f0c827.png`
- Reference: `设计图/悬浮窗/imagegen-prompt.md`

- [ ] **Step 1: Generate the first high-fidelity variant**

Call the built-in Image 2 generation tool with the screenshot as a structural reference. Request one complete centered narrow floating window on a clean neutral canvas, high-fidelity UI rendering, at least four complete account cards, and a visible footer.

- [ ] **Step 2: Inspect the generated image**

Check the following invariants visually: one window only; window border not cropped; header, tabs, account list, scrollbar, and footer all visible; no phone bezel; no dark mode; no unrelated controls; account rows clearly separate Session from Weekly.

- [ ] **Step 3: Iterate once only if a concrete invariant fails**

Repeat the original constraints and request only the failed change, such as `keep the full bottom action area visible` or `remove the phone-like outer bezel`. Do not redesign unrelated regions during iteration.

- [ ] **Step 4: Copy the selected output into the package**

Copy the generated file from the tool output location to:

```text
设计图/悬浮窗/floating-window-redesign.png
```

Do not overwrite a pre-existing user asset; if the path already exists unexpectedly, save `floating-window-redesign-v2.png` and update the README.

- [ ] **Step 5: Validate the final PNG**

Run a local image metadata check and require a portrait image with both dimensions greater than `700px` and a valid PNG signature.

### Task 3: Build the standalone implementation reference and SVG assets

**Files:**
- Create: `设计图/悬浮窗/floating-window-reference.html`
- Create: `设计图/悬浮窗/assets/floating-window-bg.svg`
- Create: `设计图/悬浮窗/assets/card-noise.svg`
- Create: `设计图/悬浮窗/assets/floating-window-icons.svg`
- Create: `设计图/悬浮窗/assets/quota-indicators.svg`

- [ ] **Step 1: Create the background and texture assets**

`floating-window-bg.svg` must use an ivory base plus restrained blue and mint radial glows. `card-noise.svg` must use `feTurbulence` at very low opacity so text contrast remains unaffected.

- [ ] **Step 2: Create the symbol sprite**

Define `<symbol>` entries with these IDs: `icon-stop`, `icon-refresh`, `icon-open`, `icon-exit`, `icon-clock`, `icon-calendar`, and `icon-status`. Use `currentColor`, rounded strokes, and no external fonts or scripts.

- [ ] **Step 3: Create quota indicator references**

Include one health-dot trail and three progress examples: healthy green, warning amber, and critical red. Keep them decorative; the HTML must generate actual dots and bars with CSS.

- [ ] **Step 4: Implement the standalone HTML**

The document must contain semantic regions with these class contracts:

```html
<main class="floating-window">
  <header class="window-header">...</header>
  <nav class="provider-tabs" aria-label="智能体筛选">...</nav>
  <section class="account-scroll" aria-label="账号额度列表">...</section>
  <footer class="window-actions">...</footer>
</main>
```

Each account must use `.account-card`, `.account-title`, `.account-meta`, `.health-dots`, and two `.quota-row` elements containing `.progress-track > .progress-fill`. Include healthy, warning, and critical examples. Use `position: sticky` or a three-row CSS grid so the middle section scrolls while header and footer remain visible.

- [ ] **Step 5: Verify file references and overflow rules**

Run:

```powershell
rg -n "floating-window-bg.svg|card-noise.svg|floating-window-icons.svg" '设计图/悬浮窗/floating-window-reference.html'
rg -n "overflow-y:\s*auto|grid-template-rows|position:\s*sticky" '设计图/悬浮窗/floating-window-reference.html'
rg -n "Session|Weekly|刷新|打开 Quotio|退出 Quotio" '设计图/悬浮窗/floating-window-reference.html'
```

Expected: all local assets, scrolling layout rules, quota labels, and footer actions are present.

### Task 4: Render, inspect, and finalize the delivery package

**Files:**
- Verify: `设计图/悬浮窗/floating-window-redesign.png`
- Verify: `设计图/悬浮窗/floating-window-reference.html`
- Verify: `设计图/悬浮窗/design-tokens.json`
- Verify: `设计图/悬浮窗/assets/*.svg`

- [ ] **Step 1: Render the HTML at the logical window size**

Open the local HTML in Chromium at approximately `420 × 1040` viewport and capture a screenshot. Expected: the `360 × 960` window fits with a small outer margin, the middle account list scrolls, and no horizontal scrollbar appears.

- [ ] **Step 2: Inspect the rendered screenshot**

Verify text is readable, the stop button is not clipped, tabs wrap cleanly, four account cards are visible or partially visible, percentage labels do not overlap PLUS badges, and all three footer actions fit.

- [ ] **Step 3: Run package integrity checks**

Run:

```powershell
$required = @(
  '设计图/悬浮窗/README.md',
  '设计图/悬浮窗/floating-window-redesign.png',
  '设计图/悬浮窗/floating-window-reference.html',
  '设计图/悬浮窗/design-tokens.json',
  '设计图/悬浮窗/imagegen-prompt.md',
  '设计图/悬浮窗/assets/floating-window-bg.svg',
  '设计图/悬浮窗/assets/card-noise.svg',
  '设计图/悬浮窗/assets/floating-window-icons.svg',
  '设计图/悬浮窗/assets/quota-indicators.svg'
)
$missing = $required | Where-Object { -not (Test-Path -LiteralPath $_) }
if ($missing) { throw "Missing: $($missing -join ', ')" }
```

Expected: command exits successfully with no missing files.

- [ ] **Step 4: Confirm scope isolation**

Run:

```powershell
git status --short
```

Expected: this task adds only the plan/spec documents and the new `设计图/悬浮窗/` package; pre-existing user changes elsewhere remain untouched.
