# Imagegen Prompt

```text
Use case: ui-mockup
Asset type: high-fidelity desktop 2FA management page redesign mockup for a Tauri/React app
Primary request: Redesign the provided 2FA 管理 page screenshot into a premium, developer-buildable local TOTP vault page. Preserve the product semantics: left navigation, page title 2FA 管理, local TOTP security notice, input for name/email, input for otpauth://totp or Base32 Secret, query/save/QR actions, current verification code area with countdown, saved/history tabs, import/export actions, saved secrets list empty state, local proxy status in sidebar. Make the page feel secure, useful, and less empty.
Input images: Image 1 is the structural reference only. Keep the same Chinese app context and rough sidebar/navigation, but redesign the information hierarchy and visual layout.
Scene/backdrop: macOS desktop app window, full-page wide canvas, light theme with soft depth.
Subject: local two-factor authentication vault and TOTP code generator.
Style/medium: crisp high-fidelity UI design mockup, modern security tool, refined glassmorphism, implementation-friendly React/CSS/SVG components, precise cards and inputs.
Composition/framing: left sidebar about 220px; main content with title and subtle security status; top security notice strip; main 2-column layout: left large generator card with secret inputs, QR button, save button, generated 6-digit code, copy action, circular countdown ring; right vault summary card showing local-only storage, encrypted vault, import/export, backup reminder; below a saved list panel with tabs 已保存 / 历史, searchable empty state, sample saved item rows or empty-state skeleton, import/export buttons. Reduce barren whitespace while keeping calm spacing.
Lighting/mood: calm, trustworthy, private, premium, focused.
Color palette: warm ivory background (#F7F3EA), white translucent cards, deep slate text, electric blue for primary actions, cyan/teal for security and local vault, amber for countdown, soft lavender for encrypted vault accents, red only for destructive/delete warnings.
Materials/textures: subtle paper grain background, frosted translucent panels, thin borders, soft shadows, gentle blue-cyan glow around the TOTP code panel.
Text: Use short Chinese labels where legible: 2FA 管理, 本地 TOTP, 本地保存, 加密保险箱, 名称 / 邮箱, otpauth://totp 或 Base32 Secret, 查询, 保存, QR, 当前验证码, 428 916, 28s, 复制, 已保存, 历史, 导入, 导出, 输入有效密钥后生成验证码, 清除应用数据前请先导出备份. Do not add unrelated menus.
Constraints: Make it realistic and buildable in React/CSS/SVG. No dark mode, no phone mockup, no giant illustration, no brand logos, no watermark. Text and controls should be readable. Keep navigation labels aligned with the reference.
Avoid: blurry text, excessive neon, cyberpunk, generic dashboard template, random unrelated charts, turning the page into a marketing hero, showing real secrets.
```
