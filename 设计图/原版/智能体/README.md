# 智能体页面重设计交付包

这个文件夹是针对“智能体”页面的独立设计交付，与 `仪表盘`、`额度`、`服务商`、`2FA` 分开管理。它只包含设计稿和开发参考素材，不修改现有业务代码。

## 文件说明

- `agent-redesign.png`：使用 imagegen 生成的高保真智能体页面效果图。
- `agent-reference.html`：可直接用浏览器打开的静态参考实现，包含完整布局、响应式规则和交互状态样式。
- `design-tokens.json`：颜色、字体、间距、圆角、阴影以及核心组件尺寸。
- `imagegen-prompt.md`：主效果图使用的最终提示词，方便继续迭代。
- `assets/agents-bg.svg`：页面暖色柔光、冷色光晕和细网格背景。
- `assets/card-noise.svg`：卡片表面的轻微纸感噪点。
- `assets/agent-icons.svg`：侧栏、状态、工具和操作图标的 SVG symbol 集合。
- `assets/route-flow.svg`：Codex、启动方案和本地代理的路由关系素材。

## 开发落地建议

1. 页面建议拆成 `AgentsOverviewStrip`、`ConnectedAgentsPanel`、`RuntimeOverviewPanel`、`LaunchSchemesPanel`、`AgentDiscoveryGrid`。
2. “运行概览”不要求新增后端接口，可以用现有代理状态、当前应用、活动方案和路由状态组合生成。
3. 工具状态建议统一为 `detected`、`installed`、`configured`、`running`，页面 badge 和按钮由状态映射，不在组件内写分散判断。
4. 可执行文件路径、端点和 API Key 使用等宽字体；窄屏时允许省略，但 hover 或复制操作应保留完整值。
5. 启动方案卡优先展示运行态，危险操作和主操作分组，避免“停止”和“编辑”视觉权重相同。
6. 图标可通过 `<svg><use href="./assets/agent-icons.svg#play" /></svg>` 直接引用，也可替换为项目现有图标库。
7. 背景和卡片建议组合 `agents-bg.svg`、`card-noise.svg` 与 CSS 渐变，保持设计套系一致。

## 关键尺寸

- 主效果图画布：`1748 x 899`
- 左侧栏参考宽度：约 `228px`；开发实现可按 `220-256px` 自适应
- 主内容边距：`32px`
- 顶部指标卡高度：约 `118px`
- 已接入智能体行高：约 `92px`
- 当前启动方案行高：约 `116px`
- 发现智能体卡最小高度：约 `128px`

## 视觉方向

浅色 Agent Operations Console：暖米色与冷蓝色背景叠加，白色半透明卡片作为信息容器，蓝色承担配置和新增操作，青绿色表示代理健康与运行状态，橙色表示待配置，红色仅用于停止或删除。目标是让“哪些工具可用、代理是否健康、哪个方案正在运行、下一步能做什么”在一个视口内完成判断。
