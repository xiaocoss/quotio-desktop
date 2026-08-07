# Codex Dream Skin integration

This directory vendors the Windows runtime from the sibling Codex Dream Skin
project under its MIT license. Quotio starts it only as part of a Codex app
launch profile and tears it down with that profile.

Local integration changes:

- no installer or shortcut creation is invoked;
- Quotio keeps sole ownership of temporary `~/.codex` backup and restore;
- the Windows celebrity/reference artwork is replaced with the upstream
  project's MIT-covered abstract `portal-hero.png` asset;
- renderer branding is neutralized to `Quotio Dream Skin`;
- Windows theme packs are loaded from `windows/themes/<id>`; the bundled
  presets include `dream`, `aurora`, `midnight`, plus the eight README
  gallery effects requested for the personal portable build;
- user-imported PNG, JPEG, and WebP images are converted into isolated theme
  packs under `%APPDATA%/Quotio/dream-skin/themes/<id>`; the original image
  stays outside the packaged application and is limited to 16 MB;
- Quotio dynamically merges bundled and user themes in the launch-profile
  selector, while Rust resolves and validates the final theme directory before
  passing it to PowerShell;
- atomic UTF-8 replacement uses a real same-directory backup path for
  PowerShell 5.1 and modern pwsh compatibility;
- the skin is packaged as a Quotio resource and requires Node.js 22 or newer
  on `PATH` at runtime.

The original project README is preserved as `README.upstream.md`. See
`UPSTREAM-SNAPSHOT.md` for source hashes and `LICENSE` / `NOTICE.md` for license
and trademark boundaries.

## Codex shell anchors (why `data-*`, not class names)

The injector has to find Codex's shell in a renderer it does not control. Codex
`26.730.8199.0` (Store auto-update, 2026-08-05) replaced the semantic class names
with hashed CSS-module classes — `main.main-surface` became
`main._MainContentSurface_<hash>` — which silently broke target discovery:
`connectCodexTargets` found no page matching the expected markers and every
Dream Skin launch failed at `--verify`, regardless of the selected theme.

That build also exposes stable semantic attributes, so anchors are matched on
those first and fall back to the old class names (both generations work):

| purpose        | preferred anchor                          | legacy fallback              |
| -------------- | ----------------------------------------- | ---------------------------- |
| shell main     | `main[data-app-shell-main-surface]`       | `main.main-surface`          |
| shell header   | `header[data-app-shell-application-menu-bar]` | `header.app-header-tint` |
| composer probe | `[data-codex-composer]`                   | `.composer-surface-chrome`   |
| composer chrome| `[data-composer-surface-variant]`         | `.composer-surface-chrome`   |
| sidebar        | `aside.app-shell-left-panel` (unchanged across both) |                   |

Never match a hashed CSS-module class (`_MainContentSurface_zbk1f_63`): the hash
changes on every Codex build. Also never fall back to a bare `document
.querySelector("main")` — on the new shell the first `<main>` is the outer window
frame, not the content surface, so layout math anchored to it is offset by the
sidebar width.

Still outstanding after the anchor repair: the `--verify` geometry assertions
(`hero`, `cardsInsideHero`, `pinkLayoutAligned`, `pinkUniformScale`,
`pinkCompositionGeometry`, `composerContentGeometry`, `polaroidPositionAligned`,
`chromeAligned`, `brandAligned`) are still calibrated against the pre-update
layout and need re-deriving against the current shell. `hero` in particular is
read positionally via `home.firstElementChild.firstElementChild
.firstElementChild`, which now lands on a collapsed `div.home-banners`; it should
measure a node the skin itself marks instead of a fixed descent path.
