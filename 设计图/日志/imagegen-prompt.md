# Imagegen Prompt

生成方式：内置 image_gen。

参考图用途：

- Image 1：日志页面的主要结构与数据参考。
- Image 2：同一应用的窗口及侧栏比例参考。
- Image 3：已有智能体重设计稿，作为设计套系的视觉风格参考。

~~~text
Use case: ui-mockup
Asset type: high-fidelity desktop request-log observability page redesign for a Tauri/React macOS app
Primary request: Redesign the provided 日志 page into a premium, developer-buildable request observability console. Preserve the real product semantics: request logs and proxy logs tabs, search, refresh, clear logs, today/all time filter, provider filter, request totals, success rate, token totals, average latency, dense request rows, timestamps, HTTP status, provider, masked account, model, mode, duration, input/output tokens, pagination, and the local proxy status in the sidebar. Improve scanability and anomaly discovery without inventing unrelated monitoring features.
Input images: Image 1 is the primary structural and data reference for the logs page. Image 2 is only a secondary reference for the same app window and sidebar proportions. Image 3 is the established redesign style reference for this product suite; match its light operational visual language, card treatment, spacing, and navigation.
Scene/backdrop: full macOS desktop app window, very wide landscape canvas, light theme with a subtle warm ivory to cool blue atmospheric background.
Subject: request and proxy log inspection page for an AI account/proxy management desktop application.
Style/medium: crisp shippable product UI mockup, realistic React/CSS/SVG implementation, refined operations console, translucent white cards, precise dense table, restrained depth, not concept art.
Composition/framing: left sidebar around 220px with macOS traffic lights and navigation, active item 日志. Main header with title “日志”, subtitle “追踪请求、代理事件与性能异常”, segmented tabs “请求日志” active and “代理日志”, a search field, refresh button, and a restrained clear action. A top row of four metric cards: “总请求 299”, “成功率 69.0%”, “平均耗时 28.5s”, “总 Tokens 22.9M”; use green for successful traffic, amber to call attention to the 69% success rate and slow latency, blue/cyan for token volume. Add one compact horizontal “请求健康” strip with a small status distribution bar and concise derived labels such as “2xx 206”, “4xx 31”, “5xx 62”, “最慢 107.03s”; keep it small and operational, not a full analytics dashboard. Under it, a clear filter toolbar with time range 今天/7天/全部, provider, model, status, mode, and reset filters. Main area is a large full-width request table with sticky-looking header and 10-12 readable rows. Columns: 时间, 状态, 服务商 / 账号, 模型 / 模式, 耗时, 输入 Tokens, 输出 Tokens, 总计, 操作. Show mostly 200 success rows and a few plausible 429/500 rows so warning/error styling is demonstrated; preserve data patterns from the reference such as codex, masked emails, gpt-5.6-sol, 推理 max, durations around 49.80s to 107.03s, and large input token counts. Highlight one slow row with a subtle amber edge, not a modal. Bottom pagination “1 / 15” with previous/next controls and row count.
Lighting/mood: calm, precise, dependable, operational, premium, clean.
Color palette: warm ivory #F7F3EA, cool mist #F5F8FF, translucent white surfaces, deep slate #172033, electric blue #2E7BFF for active tabs and filters, mint/green #20B86E for 2xx success, amber #FF9F1C for slow/4xx warning, red #EF4444 for 5xx and clear/destructive action, cyan #16A7C9 for token data, lavender only as a minor accent.
Materials/textures: subtle paper-grain background, frosted panels, thin cool-gray borders, soft shadows, gentle blue and amber glows behind metric cards, precise table separators.
Text (verbatim where legible): “日志”, “追踪请求、代理事件与性能异常”, “请求日志”, “代理日志”, “搜索请求 ID、账号或模型”, “刷新”, “清空”, “总请求”, “299”, “成功率”, “69.0%”, “平均耗时”, “28.5s”, “总 Tokens”, “22.9M”, “请求健康”, “今天”, “7天”, “全部”, “全部服务商”, “全部模型”, “全部状态”, “全部模式”, “重置筛选”, “时间”, “状态”, “服务商 / 账号”, “模型 / 模式”, “耗时”, “输入 Tokens”, “输出 Tokens”, “总计”, “操作”, “查看详情”, “上一页”, “下一页”, “1 / 15”. Keep navigation labels aligned with the references: 仪表盘, 额度, 服务商, 2FA, 智能体, API 密钥, 日志, 设置, 关于.
Constraints: realistic and buildable in React/CSS/SVG; dense table remains the main focus; hierarchy is readable at desktop scale; no dark mode, no phone mockup, no giant illustration, no marketing hero, no brand logos, no watermark. Do not expose full emails or API keys.
Avoid: blurry tiny text, excessive neon, cyberpunk terminal aesthetic, replacing the table with generic charts, oversized empty regions, decorative graphs without meaning, hiding filters, hiding token columns, unrelated infrastructure metrics.
~~~
