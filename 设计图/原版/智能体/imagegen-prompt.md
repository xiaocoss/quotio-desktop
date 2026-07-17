# Imagegen Prompt

生成方式：内置 `image_gen`，原始截图作为信息结构参考图，不作为像素级编辑目标。

```text
Use case: ui-mockup
Asset type: high-fidelity desktop smart-agent management page redesign mockup for a Tauri/React macOS app
Primary request: Redesign the provided 智能体 page screenshot into a premium, developer-buildable agent operations console. Preserve the real product semantics: left navigation, installed CLI tools, Claude Code installed but needing configuration, Codex configured, executable paths, local CLIProxyAPI, launch schemes, one running scheme named Got-5.5, masked account, model name, local endpoint, API key hint, stop/edit/delete actions, add scheme action, and discoverable but uninstalled tools Amp, Factory Droid, Gemini, OpenCode. Improve hierarchy and usefulness without inventing unrelated features.
Input images: Image 1 is the structural and content reference only. Generate a newly designed screen, not a pixel-for-pixel edit. Keep the same Chinese desktop app context and aligned navigation labels.
Scene/backdrop: full macOS desktop app window, very wide landscape canvas, light theme with subtle warm ivory to cool blue atmospheric background.
Subject: AI CLI agent setup and local proxy launch-plan management page.
Style/medium: crisp shippable product UI mockup, refined modern operations console, realistic React/CSS/SVG implementation, translucent white cards, precise lists, restrained soft depth, not concept art.
Composition/framing: left sidebar around 220px with macOS traffic lights and navigation; main content with title “智能体” and subtitle “管理 CLI 智能体与本地代理启动方案”; compact refresh action. Top overview strip with four metrics: “已检测 6”, “已安装 2”, “已配置 1”, “运行中 1”. Below, a two-column area: a larger “已接入智能体” card listing Claude Code and Codex with icon tile, installation/configuration badge, executable path, and clear actions; a narrower “运行概览” card showing local proxy healthy, address 127.0.0.1:28317, current agent Codex, active scheme Got-5.5, and route-ready status. Next, a prominent “当前启动方案” panel with Codex application path, a selected running Got-5.5 scheme card, green live indicator, masked account, model gpt-5.5-sol, endpoint http://127.0.0.1:28317, API key hint, and Stop/Edit/Delete controls plus “新建方案”. Bottom section “发现更多智能体” uses four compact cards for Amp, Factory Droid, Gemini, OpenCode with auto-detect or install-guide actions. Keep content dense enough to avoid the original barren empty area while maintaining generous spacing.
Lighting/mood: calm, dependable, technically capable, premium, clean.
Color palette: warm ivory #F7F3EA, cool mist #F5F8FF, translucent white surfaces, deep slate #172033, electric blue #2E7BFF for primary actions, mint/teal #14B8A6 and green #20B86E for healthy/running, amber #FF9F1C for needs configuration, red only for stop/delete, lavender used sparingly for discovery.
Materials/textures: subtle paper-grain background, frosted panels, thin cool-gray borders, soft shadows, gentle blue and mint glows around active cards.
Text (verbatim where legible): “智能体”, “管理 CLI 智能体与本地代理启动方案”, “已检测 6”, “已安装 2”, “已配置 1”, “运行中 1”, “已接入智能体”, “Claude Code”, “已安装”, “需要配置”, “Codex”, “已配置”, “运行概览”, “本地代理”, “运行正常”, “127.0.0.1:28317”, “当前启动方案”, “Got-5.5”, “运行中”, “停止”, “编辑”, “删除”, “新建方案”, “发现更多智能体”, “Amp”, “Factory Droid”, “Gemini”, “OpenCode”, “自动检测”, “安装指南”. Do not add unrelated navigation.
Constraints: wide desktop page; realistic and buildable in React/CSS/SVG; readable information hierarchy; preserve sidebar labels from the reference: 仪表盘, 额度, 服务商, 2FA, 智能体, API 密钥, 日志, 设置, 关于; active item 智能体. No dark mode, no phone mockup, no giant illustration, no marketing hero, no provider brand logos, no watermark.
Avoid: blurry text, excessive neon, cyberpunk, generic analytics charts, oversized empty regions, hiding executable paths, removing launch-plan actions, replacing the product page with a marketing landing page.
```
