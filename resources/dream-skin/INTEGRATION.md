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
