# Quotio UI 样式提示词

本文档用于沉淀 Quotio Desktop 当前 UI 风格，可用于：

- 后续统一 CSS / 组件视觉语言
- 给 UI 生成工具作为提示词
- 给设计师或开发者作为页面风格说明
- 生成不同页面的视觉参考稿

## 统一基础风格

### English Prompt

```text
macOS SwiftUI inspired desktop application UI, light gray system background, fixed left sidebar navigation, right content workspace, frosted glass translucent cards, soft rounded corners, subtle shadows, thin low-contrast borders, compact information density, Apple system blue accent color, clean typography, dashboard management tool, native desktop app feeling, calm professional utility interface, no marketing style, no heavy web admin style
```

### 中文说明

```text
macOS / SwiftUI 风格桌面应用界面，浅灰系统背景，左侧固定导航栏，右侧内容工作区，半透明磨砂玻璃卡片，柔和圆角，轻阴影，低对比细边框，紧凑信息密度，Apple 系统蓝作为强调色，干净字体，专业管理工具质感，不要营销页风格，不要传统后台大屏风格。
```

---

## Dashboard 页面

```text
Design a macOS SwiftUI style dashboard screen for a desktop proxy management app. Fixed light-gray sidebar on the left with compact navigation items and subtle blue active state. Main content on the right with a large frosted-glass hero card at the top, showing dashboard title, short status description, platform chips, connection mode chips, and a blue refresh button. Below it, place five compact KPI cards in one row: Accounts, Requests, Tokens, Success Rate, API Keys. Use translucent white cards, 18px rounded corners, thin gray borders, soft shadows, Apple system blue accents, muted gray labels, bold numeric values. Under KPI cards, create two-column panels for runtime status and settings summary, followed by provider summary and management diagnostics. Overall feeling should be native macOS desktop utility, calm, compact, information-dense, not a web SaaS dashboard.
```

### 页面重点

```text
这是主控台，重点是状态总览、指标、运行态、管理诊断。视觉要稳，不要花。
```

---

## Providers 页面

```text
Design a Providers management screen in a macOS SwiftUI desktop app style. Use the same fixed sidebar and right-side scrollable content area. The page starts with a translucent hero header titled Providers, with a short description about account authorization, API keys, OAuth, and service account import. Use compact section cards arranged in a two-column grid. Left card: account management list with count pill, refresh button, danger delete-all button, and record cards for auth files. Right card: local API keys management with password input, replacement state, masked key records, ghost and danger actions. Below, add OAuth provider cards and Vertex service account import card. Visual style: frosted glass panels, soft gray borders, small blue eyebrow labels, compact forms, subtle Apple blue buttons, red danger buttons, muted metadata, professional system utility interface.
```

### 页面重点

```text
这是账号和授权管理页，信息多但不能乱，表单和记录列表要紧凑。
```

---

## Quota 页面

```text
Design a Quota monitoring screen for a macOS-style desktop management app. Keep the left sidebar fixed and the right workspace independently scrollable. Top hero card titled Quota with a short explanation about request usage and quota status. Below, show compact stat cards for total requests, success, failures, tokens, and quota-related counters. Use account quota cards in a responsive grid, each card with a subtle left status border: green for healthy, orange for warning, red for error, blue for neutral. Include control panels for quota-exceeded behaviors such as switch project and switch preview model. Visual style should be SwiftUI-like, light gray background, translucent white cards, 14-18px rounded corners, tiny status pills, muted text, compact but readable data layout.
```

### 页面重点

```text
这是额度与请求监控页，状态色要明确，但整体仍然轻。
```

---

## Agents 页面

```text
Design an Agents configuration screen for a native macOS-style desktop app. Use a fixed sidebar and a right content area with frosted-glass panels. The page manages CLI agents and proxy configuration. Start with a clean hero card titled Agents, describing CLI configuration and managed proxy setup. Display agent status cards in a grid, each showing agent name, installed status, configured state, binary path, version, and last configured info. Provide compact action buttons for detecting agents, reading configuration, configuring proxy, listing backups, restoring backup, and resetting configuration. Use small status pills, muted metadata, translucent record cards, Apple blue ghost buttons, red reset danger buttons, and minimal iconography. The interface should feel like macOS System Settings plus developer tooling.
```

### 页面重点

```text
这是 CLI 工具配置页，重点是“状态 + 操作 + 备份”，不能像普通表格后台。
```

---

## Fallback 页面

```text
Design a Fallback routing configuration screen for a macOS SwiftUI inspired desktop app. The screen should manage virtual models and fallback entries. Use a light gray background, fixed sidebar, right scrollable workspace, and translucent white panels. Top hero card titled Fallback with a description about model fallback and route caching. Include summary cards for fallback enabled state, route cache state, available models, and active virtual models. Main area contains cards for virtual model configuration, fallback entry list, priority ordering, provider/model selectors, and route state preview. Use compact forms, small segmented-like toggle controls, rounded list cards, subtle drag/order affordances, blue primary actions, gray ghost actions, and orange warning states. The design should feel like a precise desktop configuration tool, not a web form-heavy admin panel.
```

### 页面重点

```text
这是策略配置页，核心是模型兜底规则、优先级、路由状态。
```

---

## API Keys 页面

```text
Design an API Keys management screen in a macOS native desktop utility style. Fixed sidebar on the left, right-side content area with compact panels. Top hero card titled API Keys, explaining local proxy keys and management snapshot. Main content uses translucent panels with masked API key records, key count pills, password input field for adding a new key, replacement flow card, and inline actions for replace and delete. Emphasize security: masked secrets, muted metadata, no raw key exposure. Use Apple system blue for safe actions, soft red for destructive actions, frosted glass backgrounds, thin borders, 14px rounded input fields, compact spacing, calm and trustworthy visual tone.
```

### 页面重点

```text
这是密钥管理页，要有安全感，密钥必须是 masked、低噪音、操作明确。
```

---

## Logs 页面

```text
Design a Logs screen for a macOS SwiftUI style desktop management app. Keep the fixed left sidebar and independent right content scroll. Top hero card titled Logs with a short description about runtime logs and request logs. Main layout includes a control panel with refresh, clear logs, filter by keyword, error-only toggle, and request-log switch. The log list should look like compact native record rows, with monospace log text, subtle gray background, thin borders, rounded corners, and red-tinted rows for errors. Use a maximum-height scrollable log panel inside the content area. Overall design should feel like a lightweight desktop diagnostic console, readable but not terminal-heavy.
```

### 页面重点

```text
这是运行日志页，应该像轻量诊断控制台，不要黑底终端风。
```

---

## Settings 页面

```text
Design a Settings screen for a macOS SwiftUI inspired desktop app. Use a fixed left sidebar and a right scrollable settings workspace. Top hero card titled Settings with actions for opening config directory and saving app settings. Below, show four compact status cards: Credential, Local key, Remote key, Platform. Main content uses two-column translucent panels. Left panel: application settings with operating mode, connection mode, theme, language, notifications toggle, launch at login toggle, credential refresh, and test notification. Right panel: proxy connection settings with host, port, remote endpoint, remote management key, local runtime readiness, remote connection readiness, proxy resources diagnostics. Use compact forms, rounded inputs, Apple blue focus rings, subtle toggle rows, masked secret placeholders, low-contrast borders, and native macOS settings feeling.
```

### 页面重点

```text
这是系统设置页，应该像 macOS Settings：分组清晰、表单紧凑、状态卡明确。
```

---

## 统一负面提示词

### English Negative Prompt

```text
avoid heavy enterprise admin dashboard, avoid dark cyberpunk UI, avoid neon gradients, avoid marketing landing page, avoid oversized hero typography, avoid dense spreadsheet tables, avoid Material Design look, avoid Bootstrap style, avoid harsh shadows, avoid pure black sidebar, avoid colorful illustrations, avoid mobile-first layout
```

### 中文说明

```text
不要传统企业后台，不要黑客风，不要霓虹渐变，不要营销落地页，不要超大标题，不要密集表格，不要 Material Design，不要 Bootstrap 感，不要重阴影，不要纯黑侧栏，不要插画化，不要移动端优先布局。
```

---

## 统一风格母版

```text
A native macOS SwiftUI style desktop management app for proxy, quota, API keys, providers, logs, and settings. Fixed left sidebar, independent right workspace scrolling, light gray system background, frosted translucent cards, subtle shadows, thin borders, 18px rounded panels, 8px rounded compact buttons, Apple system blue accent, green/orange/red semantic status colors, compact typography, small status pills, masked secrets, diagnostic utility feeling, calm professional interface, high information density without clutter.
```

### 使用方式

```text
先使用“统一风格母版”，再追加具体页面的页面提示词，最后追加“统一负面提示词”。
```

示例：

```text
[统一风格母版]
[Dashboard 页面提示词]
[统一负面提示词]
```