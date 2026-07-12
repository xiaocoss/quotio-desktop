# Proxy Logs Imagegen Prompt

生成方式：内置 image_gen。

参考图用途：

- Image 1：已经完成的请求日志设计稿，作为布局和视觉体系主参考。
- Image 2：原始日志页面截图，作为导航和产品语义参考。

~~~text
Use case: ui-mockup
Asset type: high-fidelity desktop proxy-log viewer state for the same Tauri/React macOS app
Primary request: Create the missing “代理日志” state of the existing redesigned 日志 page. Preserve the exact product shell, sidebar, title, spacing, colors, card treatment, search, refresh, and clear actions from Image 1, but switch the active tab from 请求日志 to 代理日志 and replace request metrics/table content with a practical proxy runtime log viewer based on the real app data model: raw log lines, total line count, and latest timestamp. Keep this clearly the sibling state of the same page, not a separate settings or marketing page.
Input images: Image 1 is the primary style and layout reference and should remain highly consistent. Image 2 is the original app screenshot for navigation and page semantics.
Scene/backdrop: full macOS desktop app window, very wide landscape canvas, light operational theme with warm ivory and cool blue atmosphere.
Subject: local AI proxy runtime log stream inspection page.
Style/medium: crisp shippable product UI mockup, realistic React/CSS/SVG implementation, refined light operations console, translucent white cards, precise monospaced log stream, restrained depth, not dark terminal concept art.
Composition/framing: keep the same left sidebar around 220px, active item 日志, title “日志”, subtitle “追踪请求、代理事件与性能异常”, top tabs with “代理日志” active and “请求日志” inactive. Keep search field with placeholder “搜索代理日志”, refresh, and clear. Replace the request summary with four compact runtime metrics: “日志行数 1,248”, “代理状态 运行正常”, “错误 3”, “最新日志 08:12:04”. Below add one compact proxy-status strip showing “本地代理 127.0.0.1:28317”, “文件日志 已开启”, “调试模式 已关闭”, and an “自动滚动” switch enabled. Under it place a toolbar with level chips “全部”, “INFO”, “WARN”, “ERROR”, a source filter “全部来源”, and a reset filter action. Main focus is a large full-width light log-stream panel, not a dark terminal. Use a sticky-looking header with “实时代理日志” and a green live indicator. Show 14-16 readable monospaced lines with line number gutter, timestamp, colored level pill, source, and message. Include plausible raw lines compatible with a local proxy: proxy started on 127.0.0.1:28317, management snapshot refreshed, route selected provider=codex model=gpt-5.6-sol, request completed status=200 latency=49.80s, credential switched, WARN upstream latency 77.16s, WARN rate limit status=429, ERROR upstream request failed status=500, retry scheduled, request completed. Highlight warning lines with subtle amber background and errors with restrained red background. Bottom status bar shows “显示最近 200 / 1,248 行”, “最新 08:12:04”, and “自动滚动中”.
Lighting/mood: calm, precise, dependable, operational, premium, clean.
Color palette: same as Image 1: warm ivory #F7F3EA, cool mist #F5F8FF, translucent white, deep slate #172033, electric blue #2E7BFF for active tab and filters, mint/green #20B86E for live/INFO, amber #FF9F1C for WARN, red #EF4444 for ERROR and clear, cyan used sparingly.
Materials/textures: subtle paper grain, frosted panels, thin cool-gray borders, soft shadows, readable light monospaced log surface with gentle row separators.
Text (verbatim where legible): “日志”, “追踪请求、代理事件与性能异常”, “请求日志”, “代理日志”, “搜索代理日志”, “刷新”, “清空”, “日志行数”, “1,248”, “代理状态”, “运行正常”, “错误”, “3”, “最新日志”, “08:12:04”, “本地代理”, “127.0.0.1:28317”, “文件日志”, “已开启”, “调试模式”, “已关闭”, “自动滚动”, “全部”, “INFO”, “WARN”, “ERROR”, “全部来源”, “重置筛选”, “实时代理日志”, “显示最近 200 / 1,248 行”, “自动滚动中”. Keep the same navigation labels as Image 1.
Constraints: buildable in React/CSS/SVG; preserve the same layout system as the request logs state; raw logs remain the main focus; no dark mode, no cyberpunk terminal, no unrelated settings form, no charts, no giant illustration, no brand logos, no watermark. Do not expose full credentials or API keys.
Avoid: black terminal background, tiny unreadable log text, replacing logs with analytics charts, excessive neon, unrelated proxy configuration controls, hiding timestamps or severity levels, oversized empty regions.
~~~
