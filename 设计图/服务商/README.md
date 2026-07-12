# 服务商页面重设计交付包

这个文件夹是针对“服务商”页面的独立设计交付，和 `仪表盘`、`额度` 分开管理。它只新增设计稿和开发参考素材，不改动现有业务代码。

## 文件说明

- `providers-redesign.png`：使用 imagegen 生成的高保真服务商页面重设计效果图。
- `providers-reference.html`：可直接打开的静态参考实现，包含概览条、服务商卡、洞察卡、自定义接口模板和响应式 CSS。
- `design-tokens.json`：颜色、字体、间距、圆角、阴影、服务商卡与接口模板尺寸等设计 token。
- `imagegen-prompt.md`：本次主效果图的生图提示词，方便后续迭代。
- `assets/providers-bg.svg`：服务商页背景柔光和网格。
- `assets/card-noise.svg`：卡片细微纸感噪点纹理。
- `assets/provider-icons.svg`：SVG symbol 图标集合。
- `assets/integration-pattern.svg`：自定义接口模板卡背景参考素材。

## 开发落地建议

1. 页面组件建议拆成 `ProvidersOverviewStrip`、`ConnectedProviderCard`、`ProviderInsightsPanel`、`CustomEndpointTemplates`。
2. 服务商卡沿用现有数据：provider name、mode、多账号数量、accounts、status、view/delete 操作即可。
3. “服务商洞察”是现有数据的视觉汇总，不一定需要新增后端字段；可以由账户状态派生出路由就绪、账户健康、下一步建议。
4. 自定义接口模板卡可以作为空状态增强：即使接口数量为 0，也能给用户明确入口。
5. 图标可以拆用 `assets/provider-icons.svg`，也可以替换成现有图标库，只要保持尺寸和颜色 token。
6. 卡片背景推荐使用 `linear-gradient(...)` 叠加 `url("./assets/card-noise.svg")`，和前两个页面视觉保持统一。

## 关键尺寸

- 设计画布参考：`2558 x 1307`
- 左侧栏宽度：`256px`
- 主内容边距：`32px`
- 顶部概览条高度：约 `128px`
- 服务商卡最小高度：约 `460px`
- 服务商 Logo：`64px`
- 账户行高度：`56px`
- 自定义接口模板卡高度：约 `174px`

## 视觉方向

浅色 Provider Hub：暖米色背景、白色玻璃卡片、青绿色表示已连接和正常账户、蓝色表示主操作、橙色表示登录或上游风险、红色仅用于删除等危险动作。目标是减少原页面的大面积空白，让“服务商是否可用、账户是否健康、自定义接口如何接入”一眼可见。
