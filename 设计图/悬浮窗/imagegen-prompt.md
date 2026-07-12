# Image 2 Prompt — Quotio Floating Window UI Mockup

Use case: ui-mockup

Image 1: structural and content reference

Reference handling:
Use Image 1 as the structural and content reference, not as a pixel-perfect style reference. Preserve its information architecture, data semantics, narrow floating-window proportions, provider order, account-list density, and footer actions while upgrading the visual hierarchy and polish.

Objective:
Create a high-fidelity redesign of Quotio as one compact desktop floating utility panel. Keep the existing product functions and information only; improve spacing, grouping, typography, state clarity, card separation, progress visualization, and scroll affordance.

Canvas and composition:

- Produce a vertical high-resolution result, ideally `720 × 1920px` or higher.
- The complete window body should remain approximately `3:8`, corresponding to a `360 × 960px` logical desktop window.
- Show exactly one narrow portrait desktop floating window, centered and occupying most of the image.
- Keep the full outer border, all four rounded corners, the complete header, the scrolling account region, the scrollbar, and the complete bottom action area visible with no cropping.
- A small clean neutral margin around the window is acceptable, but the result is an interface mockup rather than a device or environmental scene.

Content and hierarchy to preserve:

1. Fixed top control area:
   - Brand text `Quotio` on the left.
   - Local address `http://127.0.0.1:28317` in small crisp mono or near-mono text in the middle; visual truncation is acceptable if needed.
   - Compact red danger button labeled `停止` on the right.
2. Wrapped provider tabs in this exact order and spelling:
   - `Codex`, `Claude`, `Copilot`, `Antigravity`, `Kiro`, `GLM`, `Trae`, `cursor`.
   - `Codex` is active with a soft blue fill and blue text; the other labels are subdued text buttons. Tabs wrap naturally instead of horizontal scrolling.
3. Independently scrolling account-card list:
   - Show at least four complete compact account cards and, when space allows, a partial next card to make scrolling obvious.
   - Use a consistent `156px` logical height for each complete account card.
   - Each card includes a single-line truncated email, a visible health percentage, and a `PLUS` plan badge.
   - Preserve expiry/reset metadata and remaining-time text, using small clock/calendar/status icons and quieter secondary text.
   - Include a horizontal health-dot trail with light-gray neutral dots plus green success, orange warning, or red danger dots where appropriate.
   - Include both `Session` and `Weekly` rows. Each row shows its label, remaining time, a clearly readable percentage, and a separate `6px` rounded progress bar.
   - Use a realistic mix of healthy, warning, and danger examples without changing the card structure.
4. Visible slim scrollbar:
   - Use a narrow, low-contrast track and rounded thumb that clearly communicates the current position.
5. Fixed bottom action area:
   - Three full-width vertical rows labeled `刷新`, `打开 Quotio`, and `退出 Quotio`.
   - `刷新` is neutral, `打开 Quotio` receives restrained blue emphasis, and `退出 Quotio` uses a restrained red danger treatment.
   - Align each simple line icon and label to the left; each row should read as one clickable target.

Visual direction:

- Warm ivory page/background color `#F6F3EC` with clean white `#FFFFFF` cards and control surfaces.
- Deep slate primary text `#1F2937`, muted cool-gray secondary text, and subtle cool-gray borders.
- Blue `#3978F6` for the active provider and primary/open action.
- Green `#35B96B` for healthy status, orange `#F59E0B` for warning, and red `#EF4444` for danger and stop/exit semantics.
- Compact desktop-tool density with clearly separated white cards, roughly `14px` card radius, `18px` outer-window radius, delicate borders, and short soft shadows.
- Crisp, readable Chinese and English typography. Prefer the visual character of HarmonyOS Sans SC, MiSans, PingFang SC, Inter, or a clean system sans-serif.
- Quiet premium utility-panel appearance: warm, practical, calm, and highly scannable rather than decorative.

Implementation realism:

- Every visual element must be feasible with ordinary HTML, CSS, and simple SVG.
- Use flat or very restrained gradients only when they can be reproduced with CSS.
- Keep icons as simple rounded-stroke SVG shapes.
- Make emails, labels, percentages, plan badges, remaining times, health dots, and progress states look credible and aligned.
- Do not invent new navigation, actions, settings, charts, account fields, or product features.

Constraints:

- One desktop floating window only.
- Preserve the complete border and bottom footer; do not crop any window edge.
- Preserve all named providers and all three footer actions.
- Preserve email, health percentage, PLUS, expiry/reset date, remaining time, health dots, Session, Weekly, percentages, and progress bars.
- Keep the header and footer visually fixed while the center account list clearly reads as scrollable.
- Favor clean typography and production-realistic spacing over illustration or visual spectacle.

Avoid:

- Phone shell, mobile device bezel, tablet frame, browser chrome, laptop frame, or hardware mockup.
- Desktop room, desk surface, hands, keyboard, environmental scene, or lifestyle backdrop.
- Multiple windows, split screens, dashboards, marketing landing pages, posters, or presentation boards.
- Dark mode, black background, neon glow, heavy glassmorphism, excessive blur, glossy plastic, or dramatic lighting.
- 3D elements, isometric views, perspective distortion, illustrations, mascots, decorative logos, or photographic textures.
- Watermarks, signatures, generator labels, placeholder gibberish, illegible text, clipped corners, or cropped footer controls.
- Any new feature, extra button, added navigation, chart, search box, settings control, or data field not present in the requested content.
