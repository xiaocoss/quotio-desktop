# Rose Floral Theme Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a persistent, selectable rose-floral Quotio theme with separately reusable original-character assets and page-by-page design references while preserving the current default UI.

**Architecture:** Keep the existing React screens and Rust settings pipeline, extend the shared theme enum with `rose`, and apply the new appearance through a root `data-theme="rose"` selector plus focused CSS overrides. Archive the current design package under `设计图/原版`, generate reusable raster assets separately from full-page references, and keep all functional text and controls code-rendered.

**Tech Stack:** React 19, TypeScript 5.8, CSS, Recharts, Tauri 2, Rust/Serde, Vitest, built-in Image 2 image generation.

---

## File map

- `设计图/原版/**`: unchanged original design deliveries moved from the current page folders.
- `设计图/粉色花卉主题/<页面>/**`: page reference PNG, prompt, tokens, README, and optional reference HTML.
- `apps/desktop/public/rose/character-avatar.png`: square reusable character avatar.
- `apps/desktop/public/rose/character-hero.png`: reusable character hero portrait.
- `apps/desktop/public/rose/rose-ornaments.png`: reusable floral decoration sheet.
- `apps/desktop/public/rose/rose-bg.webp`: lightweight theme background texture.
- `apps/desktop/src/lib/theme.ts`: pure theme resolution/application helpers.
- `apps/desktop/src/lib/theme.test.ts`: theme helper tests.
- `apps/desktop/src/types.ts`: frontend `ThemeMode` union.
- `crates/quotio-types/src/lib.rs`: persisted Rust theme enum.
- `apps/desktop/src/App.tsx`: main-window theme application.
- `apps/desktop/src/MenuBarPanel.tsx`: floating-window theme application.
- `apps/desktop/src/components/sections/SettingsScreen.tsx`: rose theme selection.
- `apps/desktop/src/i18n.tsx`: English and Chinese rose-theme labels.
- `apps/desktop/src/components/rose-theme.css`: cross-page rose tokens and overrides.
- Existing page CSS files: only page-specific rose rules that cannot remain clear in the shared theme file.

### Task 1: Archive the original design package

**Files:**
- Move: `设计图/{仪表盘,额度,服务商,关于,日志,设置,悬浮窗,智能体,2FA}` → `设计图/原版/`
- Preserve: `设计图/设计图.lnk` in place unless it resolves to one of the moved folders and needs its target updated.

- [ ] **Step 1: Record the current inventory**

Run:

```powershell
Get-ChildItem '设计图' -Directory | ForEach-Object { Get-ChildItem $_.FullName -Recurse -File } | Select-Object FullName
```

Expected: every existing PNG, HTML, JSON, Markdown, and SVG delivery is listed.

- [ ] **Step 2: Create the archive directory**

Use `apply_patch` to add `设计图/原版/README.md` explaining that the directory contains the pre-rose design package and must remain visually unchanged.

- [ ] **Step 3: Move each page directory with native PowerShell**

Run one `Move-Item -LiteralPath <absolute-source> -Destination <absolute-target>` per page after verifying both resolved paths remain inside the workspace `设计图` directory.

- [ ] **Step 4: Compare the inventory**

Run the same recursive listing against `设计图/原版`; expected file counts and filenames must match Step 1.

- [ ] **Step 5: Commit**

```powershell
git add -- '设计图'
git commit -m "chore: archive original design package"
```

### Task 2: Generate the reusable original-character assets

**Files:**
- Create: `apps/desktop/public/rose/character-avatar.png`
- Create: `apps/desktop/public/rose/character-hero.png`
- Create: `apps/desktop/public/rose/rose-ornaments.png`
- Create: `apps/desktop/public/rose/rose-bg.webp`
- Create: `设计图/粉色花卉主题/角色设定/README.md`
- Create: `设计图/粉色花卉主题/角色设定/imagegen-prompts.md`

- [ ] **Step 1: Generate the canonical avatar with built-in Image 2**

Use this identity prompt and treat the supplied screenshot only as a style/mood reference:

```text
Use case: photorealistic-natural
Asset type: reusable square desktop-app avatar
Primary request: create an original fictional young adult woman for a rose-floral software theme; she must not copy or identify the real person in the reference
Subject: consistent oval face, warm brown eyes, dark glossy twin ponytails with soft bangs, pearl hair accents, blush-pink elegant dress
Composition/framing: centered head-and-shoulders portrait, square crop, generous safe margin
Lighting/mood: soft diffused studio light, romantic and polished
Color palette: ivory, blush pink, dusty rose, tiny champagne-gold accents
Constraints: no text, no logo, no watermark, anatomically natural, clean background, reusable as a 256px avatar
```

- [ ] **Step 2: Inspect the avatar**

Check facial originality, hair silhouette, eyes, earrings, edge quality, crop, watermark, and absence of text. Regenerate once with one targeted correction if any check fails.

- [ ] **Step 3: Generate the hero portrait from the accepted avatar identity**

Use the accepted avatar as the identity reference. Request a landscape-friendly seated half-body pose, hands posed naturally near the face, pink dress, white feather-soft shoulder detail, roses confined to the outer edges, and transparent-looking negative space on the left. Require no text, logo, watermark, or UI.

- [ ] **Step 4: Generate floral assets separately**

Generate a rose ornament sheet on a uniform removable chroma-key background and remove the key with the imagegen skill helper. Generate a subtle ivory/blush paper-and-bokeh background separately, then convert the selected background to WebP with existing workspace image libraries.

- [ ] **Step 5: Save project-bound files**

Copy final selected outputs from the built-in generation directory into `apps/desktop/public/rose/`; copy reference versions and exact prompts into `设计图/粉色花卉主题/角色设定/`.

- [ ] **Step 6: Verify raster assets**

Check dimensions, file size, alpha channel for ornaments, transparent corners, no green fringe, and visual consistency between avatar and hero.

- [ ] **Step 7: Commit**

```powershell
git add -- 'apps/desktop/public/rose' '设计图/粉色花卉主题/角色设定'
git commit -m "design: add rose theme character assets"
```

### Task 3: Generate page designs one by one

**Files:**
- Create: `设计图/粉色花卉主题/{仪表盘,额度,服务商,智能体,日志,设置,2FA,悬浮窗,关于}/`
- Create in each directory: `*-rose.png`, `imagegen-prompt.md`, `design-tokens.json`, `README.md`

- [ ] **Step 1: Establish a shared page prompt header**

Write the exact shared constraints into every prompt: use the accepted character identity when a person is present; preserve Quotio page information architecture; render no final UI text inside reusable assets; use ivory/blush/rose/champagne palette; keep interactive regions unobscured; target a feasible React/CSS/Recharts implementation.

- [ ] **Step 2: Generate the dashboard design**

Use the existing archived dashboard PNG as layout reference and the accepted hero portrait as supporting input. Include the major KPI cards, charts, account summary, pale sidebar, floral hero, and realistic Chinese UI labels. Inspect before continuing.

- [ ] **Step 3: Generate the quota design**

Keep account cards, progress bars, health states, filters, and refresh action. Reduce the character to a small corner portrait or omit it if it competes with quota data.

- [ ] **Step 4: Generate the providers design**

Keep provider logos, connection states, integration controls, and action hierarchy. Use floral borders and small portrait decoration only.

- [ ] **Step 5: Generate the agents design**

Keep routing flow, agent states, configuration actions, and backup controls. Reserve roses for the page perimeter so the flow remains readable.

- [ ] **Step 6: Generate logs and proxy-logs designs**

Produce two references in the `日志` directory. Preserve dense tables, filters, latency/state colors, pagination, request details, and proxy-log tabs.

- [ ] **Step 7: Generate the settings design**

Show the existing settings groups and a four-option theme selector containing system, light, dark, and rose floral. Make selection state unambiguous.

- [ ] **Step 8: Generate the 2FA design**

Preserve vault list, QR/import flow, OTP countdown, copy controls, and security messaging. Keep decorative flowers away from codes and QR regions.

- [ ] **Step 9: Generate the floating-window design**

Use the existing compact dimensions and preserve provider tabs, account quota cards, proxy status, and window controls. Use the avatar rather than the full hero portrait.

- [ ] **Step 10: Generate the about design**

Use the strongest brand treatment: avatar/hero, Quotio identity, version and links, while preserving existing about-page actions.

- [ ] **Step 11: Validate and document every page**

For each output, check character consistency, control visibility, Chinese text artifacts, impossible layouts, watermark, and correspondence with the archived page. Record asset references and implementation notes in its README.

- [ ] **Step 12: Commit**

```powershell
git add -- '设计图/粉色花卉主题'
git commit -m "design: add rose theme page references"
```

### Task 4: Add a tested rose theme model

**Files:**
- Create: `apps/desktop/src/lib/theme.ts`
- Create: `apps/desktop/src/lib/theme.test.ts`
- Modify: `apps/desktop/src/types.ts`
- Modify: `crates/quotio-types/src/lib.rs`

- [ ] **Step 1: Write failing frontend tests**

```ts
import { describe, expect, it } from "vitest";
import { resolveEffectiveTheme } from "./theme";

describe("resolveEffectiveTheme", () => {
  it("keeps the rose theme independent of system color scheme", () => {
    expect(resolveEffectiveTheme("rose", true)).toBe("rose");
    expect(resolveEffectiveTheme("rose", false)).toBe("rose");
  });

  it("resolves system to light or dark", () => {
    expect(resolveEffectiveTheme("system", true)).toBe("dark");
    expect(resolveEffectiveTheme("system", false)).toBe("light");
  });
});
```

- [ ] **Step 2: Run the failing test**

Run: `npm test -- --run src/lib/theme.test.ts` from `apps/desktop`.

Expected: FAIL because `./theme` does not exist.

- [ ] **Step 3: Extend frontend and backend types**

Change the frontend union to:

```ts
export type ThemeMode = "system" | "light" | "dark" | "rose";
```

Add a Serde-compatible `Rose` variant to the Rust `ThemeMode` enum using the enum's existing rename convention. Do not change the default, which remains `System`.

- [ ] **Step 4: Implement pure resolution and DOM application**

```ts
import type { ThemeMode } from "../types";

export type EffectiveTheme = Exclude<ThemeMode, "system">;

export function resolveEffectiveTheme(theme: ThemeMode, prefersDark: boolean): EffectiveTheme {
  return theme === "system" ? (prefersDark ? "dark" : "light") : theme;
}

export function applyTheme(root: HTMLElement, theme: EffectiveTheme): void {
  root.setAttribute("data-theme", theme);
  root.style.colorScheme = theme === "dark" ? "dark" : "light";
}
```

- [ ] **Step 5: Run focused tests and Rust tests**

Run: `npm test -- --run src/lib/theme.test.ts` from `apps/desktop`; expected PASS.

Run: `cargo test -p quotio-types`; expected PASS.

- [ ] **Step 6: Commit**

```powershell
git add -- 'apps/desktop/src/lib/theme.ts' 'apps/desktop/src/lib/theme.test.ts' 'apps/desktop/src/types.ts' 'crates/quotio-types/src/lib.rs'
git commit -m "feat: add rose theme model"
```

### Task 5: Wire theme selection into both windows

**Files:**
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/MenuBarPanel.tsx`
- Modify: `apps/desktop/src/components/sections/SettingsScreen.tsx`
- Modify: `apps/desktop/src/i18n.tsx`

- [ ] **Step 1: Replace duplicate theme resolution**

Import `applyTheme` and `resolveEffectiveTheme` in both window entry components. Each effect must resolve from `window.matchMedia("(prefers-color-scheme: dark)").matches`, apply the result, and subscribe to media changes only when the stored value is `system`.

- [ ] **Step 2: Add the settings option**

Insert this option after light and before dark:

```tsx
{ value: "rose", label: t("theme.rose") },
```

- [ ] **Step 3: Add translations**

Add English `"theme.rose": "Rose Floral"` and Chinese `"theme.rose": "粉色花卉"` alongside the existing theme keys.

- [ ] **Step 4: Run build and tests**

Run from `apps/desktop`: `npm test` and `npm run build`.

Expected: all tests pass and TypeScript/Vite build completes.

- [ ] **Step 5: Commit**

```powershell
git add -- 'apps/desktop/src/App.tsx' 'apps/desktop/src/MenuBarPanel.tsx' 'apps/desktop/src/components/sections/SettingsScreen.tsx' 'apps/desktop/src/i18n.tsx'
git commit -m "feat: expose rose theme selection"
```

### Task 6: Implement the shared rose shell and component language

**Files:**
- Create: `apps/desktop/src/components/rose-theme.css`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/components/AppShell.tsx`

- [ ] **Step 1: Import the theme stylesheet**

Add `import "./components/rose-theme.css";` after the existing shell/about CSS imports.

- [ ] **Step 2: Add semantic shell hooks**

Add an avatar `<img src="/rose/character-avatar.png" ...>` and non-interactive floral decoration elements only where the shell lacks stable selectors. Hide these elements outside `data-theme="rose"` using CSS; do not conditionally duplicate navigation.

- [ ] **Step 3: Define rose tokens**

In `rose-theme.css`, define scoped variables under `:root[data-theme="rose"]`, including page, surface, elevated surface, text, muted text, primary, primary hover, border, success, warning, danger, card shadow, large radius, and background image URLs.

- [ ] **Step 4: Style the shell**

Add scoped overrides for `.app-shell--v2`, `.sidebar`, navigation states, proxy card, content background, buttons, inputs, selects, dialogs, toast/loading overlays, and scrollbars. Ensure every selector begins with `:root[data-theme="rose"]` or targets an element hidden by default.

- [ ] **Step 5: Validate default themes**

Run `npm run build`; then inspect light, dark, and rose roots in the dev app. Expected: light/dark remain unchanged and rose displays the pale sidebar, avatar, floral background, and rose controls.

- [ ] **Step 6: Commit**

```powershell
git add -- 'apps/desktop/src/components/rose-theme.css' 'apps/desktop/src/App.tsx' 'apps/desktop/src/components/AppShell.tsx'
git commit -m "feat: style rose theme shell"
```

### Task 7: Apply the page-specific rose treatment

**Files:**
- Modify: `apps/desktop/src/components/dashboard/dashboard.css`
- Modify: `apps/desktop/src/components/sections/quota.css`
- Modify: `apps/desktop/src/components/sections/providers.css`
- Modify: `apps/desktop/src/components/sections/agents.css`
- Modify: `apps/desktop/src/components/sections/logs.css`
- Modify: `apps/desktop/src/components/sections/settings.css`
- Modify: `apps/desktop/src/components/sections/twofa.css`
- Modify: `apps/desktop/src/components/about.css`
- Modify: `apps/desktop/src/menubar.css`

- [ ] **Step 1: Implement dashboard reference**

Add scoped overrides for the hero, KPI cards, charts, tables, filters, and account summary. Use `/rose/character-hero.png` only in the dashboard hero and preserve chart semantics.

- [ ] **Step 2: Implement quota and providers references**

Map progress bars, health states, provider cards, logos, integrations, and action buttons to rose tokens without reducing status contrast.

- [ ] **Step 3: Implement agents and logs references**

Keep route-flow legibility and dense log-table readability. Use floral textures only on panel edges and empty space.

- [ ] **Step 4: Implement settings and 2FA references**

Make the theme selector visibly selected, keep QR/OTP areas plain and high contrast, and avoid image backgrounds behind secrets.

- [ ] **Step 5: Implement about and floating-window references**

Use character artwork prominently on About and the square avatar in the compact window. Preserve window controls, provider tabs, scrolling, quota warnings, and proxy actions.

- [ ] **Step 6: Check the minimum viewport**

At `960 × 640`, visit every main section. Expected: no horizontal scrollbar, no hidden primary action, no decoration over content, and no clipped dialog.

- [ ] **Step 7: Commit**

```powershell
git add -- 'apps/desktop/src/components/dashboard/dashboard.css' 'apps/desktop/src/components/sections' 'apps/desktop/src/components/about.css' 'apps/desktop/src/menubar.css'
git commit -m "feat: style rose theme pages"
```

### Task 8: Final regression and visual verification

**Files:**
- Modify if needed: only files already listed in Tasks 4–7.
- Create: `设计图/粉色花卉主题/验证记录.md`

- [ ] **Step 1: Run automated checks**

Run:

```powershell
npm test
npm run build
```

from `apps/desktop`, then run `cargo test -p quotio-types` from the repository root. Expected: all pass.

- [ ] **Step 2: Test persistence**

Select rose, restart the application, and verify rose returns. Repeat with system, light, and dark. An invalid stored/backend value must fall back through existing settings deserialization behavior without preventing startup.

- [ ] **Step 3: Compare every page with its approved design**

Capture runtime screenshots for dashboard, quota, providers, agents, logs, proxy logs, settings, 2FA, floating window, and about. Record pass/fail notes for layout, colors, character asset, overflow, empty/loading/error states, and action visibility.

- [ ] **Step 4: Verify asset boundaries**

Confirm the program references the independent avatar/hero/ornament/background files, not a full-page generated screenshot. Confirm no generated asset contains UI copy or watermarks.

- [ ] **Step 5: Inspect Git scope**

Run `git status --short` and `git diff --check`. Ensure the pre-existing `crates/quotio-core/src/codex_launch.rs` edit and `设计图/设计图.lnk` state are not accidentally included unless explicitly required by Task 1.

- [ ] **Step 6: Commit verification record**

```powershell
git add -- '设计图/粉色花卉主题/验证记录.md'
git commit -m "test: verify rose floral theme"
```
