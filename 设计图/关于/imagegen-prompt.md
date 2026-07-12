# Imagegen Prompt

生成方式：内置 image_gen。

参考图用途：

- Image 1：原始关于页面的信息与窗口结构参考。
- Image 2：已有设置页面重设计稿，作为视觉体系参考。

~~~text
Use case: ui-mockup
Asset type: high-fidelity desktop about/product-information page redesign for a Tauri/React macOS app
Primary request: Redesign the provided 关于 page into a premium, developer-buildable product information center. Preserve the real information and actions from the existing page: Quotio product identity, version v0.5.13, check for updates, platform macOS, architecture x86_64, operating mode full/local proxy, proxy endpoint http://127.0.0.1:28317, configuration directory /Users/mac/Library/Application Support/Quotio, and the description “Quotio · 多服务商 AI 代理与额度管理工具。” Fill the excessive empty space with useful hierarchy derived from these existing facts, without inventing external support links, accounts, licenses, or unrelated features.
Input images: Image 1 is the structural and content reference for the current 关于 page. Image 2 is the established redesign style reference; match its light operational aesthetic, sidebar proportions, typography, cards, borders, and soft depth.
Scene/backdrop: full macOS desktop app window, wide landscape canvas, light theme with a subtle warm ivory to cool blue atmospheric background.
Subject: product identity, version status, runtime environment, local service health, and configuration location for Quotio.
Style/medium: crisp shippable product UI mockup, realistic React/CSS/SVG implementation, refined product information console, premium but restrained, translucent white cards, not concept art and not a marketing landing page.
Composition/framing: keep the left sidebar around 220px with active item 关于 and the same navigation labels. Main header with title “关于” and subtitle “Quotio 产品信息与运行环境”. Top full-width hero card with a large original green-to-blue rounded Q app mark, product name “Quotio”, version “v0.5.13”, description “多服务商 AI 代理与额度管理工具”, and three small capability chips “多服务商代理”, “额度监控”, “本地管理”. On the right side of the hero card place a compact version-status panel: label “版本状态”, green check “当前已是最新版本”, secondary button “检查更新”, and small text “当前版本 v0.5.13”. Use subtle abstract connection lines or soft product glow as background decoration only.
Below use a three-column card grid. Card 1 “运行环境” contains rows “平台 macOS”, “架构 x86_64”, “运行模式 本地代理”, and a small badge “full”. Card 2 “本地服务” contains a green healthy status “运行正常”, endpoint “http://127.0.0.1:28317”, and concise status rows “代理服务 healthy” and “本地优先”. Card 3 “配置与数据” contains label “配置目录”, full path “/Users/mac/Library/Application Support/Quotio” in a readable monospaced path box, plus a small shield note “配置保存在本机”. At the bottom add one slim explanatory strip with three compact benefits derived from the product description: “统一管理服务商”, “查看额度与使用”, “本地代理转发”; use simple SVG-like icons and one sentence each. Finish with a restrained footer line “Quotio · 多服务商 AI 代理与额度管理工具。” Avoid large empty space while preserving generous margins.
Lighting/mood: trustworthy, calm, polished, local-first, technical, premium, clean.
Color palette: warm ivory #F7F3EA, cool mist #F5F8FF, translucent white, deep slate #172033, Quotio green #53D769 and mint for brand/healthy status, electric blue #2E7BFF for actions and technical details, cyan sparingly, amber only if update attention is needed.
Materials/textures: subtle paper grain, frosted panels, thin cool-gray borders, soft shadows, gentle mint and blue glows around the brand mark and healthy status.
Text (verbatim where legible): “关于”, “Quotio 产品信息与运行环境”, “Quotio”, “v0.5.13”, “多服务商 AI 代理与额度管理工具”, “多服务商代理”, “额度监控”, “本地管理”, “版本状态”, “当前已是最新版本”, “检查更新”, “当前版本 v0.5.13”, “运行环境”, “平台”, “macOS”, “架构”, “x86_64”, “运行模式”, “本地代理”, “full”, “本地服务”, “运行正常”, “代理服务”, “healthy”, “http://127.0.0.1:28317”, “配置与数据”, “配置目录”, “/Users/mac/Library/Application Support/Quotio”, “配置保存在本机”, “统一管理服务商”, “查看额度与使用”, “本地代理转发”, “Quotio · 多服务商 AI 代理与额度管理工具。” Keep the same sidebar navigation labels as the reference.
Constraints: realistic and buildable in React/CSS/SVG; preserve every real field from the current AboutScreen; no fake website, social, account, license, or support links; no dark mode, no phone mockup, no giant illustration, no marketing CTA, no brand logos other than an original Q app mark, no watermark. Paths and endpoint must remain readable.
Avoid: oversized empty regions, blurry text, excessive neon, generic marketing hero, testimonials, pricing, fake release notes, fake community links, hiding technical environment details.
~~~
