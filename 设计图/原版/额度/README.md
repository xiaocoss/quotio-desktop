# 额度页面重设计交付包

这个文件夹是针对“额度”页面的独立设计交付，和 `设计图/仪表盘` 分开管理。它不包含版本发布内容，也没有改动现有业务代码。

## 文件说明

- `quota-redesign.png`：使用 imagegen 生成的高保真额度页面重设计效果图。
- `quota-reference.html`：可直接打开的静态参考实现，包含页面布局、策略横幅、总览状态、账号卡片和响应式 CSS。
- `design-tokens.json`：颜色、字体、间距、圆角、阴影、额度卡片尺寸、进度条参数等设计 token。
- `imagegen-prompt.md`：本次主效果图的生图提示词，方便后续继续迭代。
- `assets/quota-bg.svg`：额度页背景柔光和网格。
- `assets/card-noise.svg`：卡片细微纸感噪点纹理。
- `assets/quota-icons.svg`：SVG symbol 图标集合。
- `assets/health-dots.svg`：健康状态点阵参考素材。
- `assets/progress-bars.svg`：Session / Weekly 进度条参考素材。

## 开发落地建议

1. 把 `design-tokens.json` 映射成 CSS variables，并尽量复用仪表盘页已有的 token 命名。
2. 页面组件建议拆成 `QuotaStrategyBanner`、`QuotaSummaryGrid`、`ProviderGroupHeader`、`QuotaAccountCard`。
3. 账号卡片的数据结构不需要改：rank、plan、email、expiresAt、health dots、success/fail counts、session/weekly quota 都可按现有字段映射。
4. 健康点阵建议用真实数组渲染，不要直接使用 `health-dots.svg`；SVG 只是视觉参考。
5. Session / Weekly 进度条建议用 CSS 宽度绑定剩余额度百分比，颜色按阈值切换：低于 35% 用 `warning`，低于 15% 用 `danger`。
6. “待命账号”保留灰色 rank 和 `待命中` 状态，不参与调度时降低视觉权重。

## 关键尺寸

- 设计画布参考：`2560 x 1310`
- 左侧栏宽度：`256px`
- 主内容边距：`32px`
- 策略横幅高度：约 `142px`
- 状态总览高度：约 `118px`
- 账号卡片宽度：约 `344px`
- 账号卡片最小高度：约 `320px`
- 健康点直径：`9px`
- 进度条高度：`6px`

## 视觉方向

浅色账号池调度控制台：暖米色背景、白色玻璃卡片、蓝紫色调度策略、青绿色服务商和健康状态、橙色额度压力、红色失败状态。目标是让“哪些账号可用、哪些账号风险高、当前策略怎么调度”一眼可见，同时仍然能用普通 React/CSS/SVG 实现。
