# Imagegen Prompt

生成方式：内置 image_gen。

参考图用途：

- Image 1：设置页上半部分，提供运行模式、基础设置、隐私和代理连接字段。
- Image 2：设置页下半部分，提供管理 API、高级参数、Cloudflared 和 Antigravity 字段。
- Image 3：已有智能体重设计稿，作为视觉体系参考。

~~~text
Use case: ui-mockup
Asset type: high-fidelity desktop settings control-center redesign for a Tauri/React macOS app
Primary request: Redesign the complete 设置 page shown across the two provided screenshots into a premium, developer-buildable settings control center. Preserve the real capabilities from both screenshots and the current product: application mode selection, general settings, language, appearance, privacy, proxy connection, management API, advanced CLIProxyAPI options, Cloudflared public tunnel, Antigravity account warmup, and open config/log directory actions. Replace the extremely long undifferentiated list with clear hierarchy and compact two-column groups without removing important settings.
Input images: Image 1 is the upper half and primary structural reference for application mode, general preferences, privacy, and proxy connection. Image 2 is the lower half and primary structural reference for management API, advanced settings, Cloudflared, and account warmup. Image 3 is the established visual style reference for this redesign suite; match its light operational aesthetic, sidebar, card depth, typography, and spacing.
Scene/backdrop: full macOS desktop app window, very wide landscape canvas, light theme with subtle warm ivory to cool blue atmospheric background.
Subject: settings hub for a local AI proxy and account management desktop application.
Style/medium: crisp shippable product UI mockup, realistic React/CSS/SVG implementation, refined settings console, translucent white cards, precise controls, restrained soft depth, not concept art.
Composition/framing: keep the left sidebar around 220px with active item 设置 and the same navigation labels. Main header with title “设置”, subtitle “配置运行模式、代理连接与请求行为”, right actions “打开配置目录” and “打开日志目录”, plus a compact healthy status badge “本地代理运行正常 · 127.0.0.1:28317”. Directly below, a three-card application mode selector: “仅监控” with 默认 badge, “本地代理” selected in blue, and “远程代理” with 实验性 badge. Under the mode selector use a balanced two-column grid. Top-left card “基础设置” contains compact rows for 开机自启, 通知 with 测试 action, 语言 简体中文, 主题 跟随系统, 隐藏敏感信息. Top-right card “代理连接” contains Host 127.0.0.1 and Port 28317 on one line, remote endpoint, masked remote management key, allow remote switch, runtime status, and actions 清除远程密钥 / 刷新凭据 / 保存连接设置. Bottom-left card “管理 API” contains Debug, 请求日志, 日志写入文件 switches, 路由策略 填充, 请求重试 3, and upstream proxy http://127.0.0.1:3067 with 读取 / 写入 / 清空 actions. Bottom-right card “高级设置” uses a compact two-column field grid for 推理强度 默认(不覆盖), 强制模型 gpt-5.5, 会话粘连, 会话粘连有效期 1h, 失败最多换凭证 0, 日志总大小上限 0 MB, plus compact switches for 禁用冷却, 禁用图像生成, 强制模型前缀, 透传上游头. At the bottom add a slim “工具与自动化” strip with two cards: Cloudflared public tunnel with “下载 cloudflared” action and Antigravity 预热 with “预热” action. Use concise helper text only where important. Make the whole page fit one desktop viewport while retaining readable controls.
Lighting/mood: calm, trustworthy, technical, organized, premium, clean.
Color palette: warm ivory #F7F3EA, cool mist #F5F8FF, translucent white surfaces, deep slate #172033, electric blue #2E7BFF for selected mode and primary actions, mint/green #20B86E for healthy runtime and enabled safe settings, amber #FF9F1C for experimental/unsaved/warning, red only for clearing credentials or destructive reset, lavender sparingly for remote and advanced settings.
Materials/textures: subtle paper-grain background, frosted panels, thin cool-gray borders, soft shadows, gentle blue and mint glows around selected and healthy cards.
Text (verbatim where legible): “设置”, “配置运行模式、代理连接与请求行为”, “打开配置目录”, “打开日志目录”, “本地代理运行正常”, “应用模式”, “仅监控”, “默认”, “本地代理”, “远程代理”, “实验性”, “基础设置”, “开机自启”, “通知”, “测试”, “语言”, “简体中文”, “主题”, “跟随系统”, “隐藏敏感信息”, “代理连接”, “主机”, “127.0.0.1”, “端口”, “28317”, “远程端点”, “远程管理密钥”, “允许远程”, “清除远程密钥”, “刷新凭据”, “保存连接设置”, “管理 API”, “调试”, “请求日志”, “日志写入文件”, “路由策略”, “填充”, “请求重试”, “上游代理地址”, “读取”, “写入”, “清空”, “高级设置”, “推理强度”, “默认（不覆盖）”, “强制模型”, “gpt-5.5”, “会话粘连”, “会话粘连有效期”, “1h”, “失败最多换凭证”, “日志总大小上限 (MB)”, “禁用冷却”, “禁用图像生成”, “强制模型前缀”, “透传上游头”, “工具与自动化”, “Cloudflared”, “下载 cloudflared”, “Antigravity 预热”, “预热”.
Constraints: realistic and buildable in React/CSS/SVG; preserve all major settings represented in the two screenshots; readable at desktop scale; fit in one viewport through grouping rather than tiny text; no dark mode, no phone mockup, no giant illustration, no marketing hero, no brand logos, no watermark. Password/key fields must remain masked. Destructive actions must be visually secondary.
Avoid: blurry tiny labels, excessive neon, cyberpunk, generic dashboard charts, removing advanced settings, hiding proxy host/port, giant empty regions, turning settings into a marketing page, using a single endless list.
~~~
