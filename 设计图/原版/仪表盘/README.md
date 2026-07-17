# 仪表盘页面重设计交付包

这个文件夹是针对当前仪表盘页面的独立设计交付，不包含版本发布内容，也没有改动业务代码。

## 文件说明

- `dashboard-redesign.png`：使用 imagegen 生成的高保真仪表盘重设计效果图。
- `dashboard-reference.html`：开发可打开的静态参考实现，包含布局、卡片、趋势图、表格、响应式规则和素材引用方式。
- `design-tokens.json`：颜色、字体、间距、圆角、阴影、图表参数等设计 token。
- `imagegen-prompt.md`：本次主效果图的生图提示词，方便后续迭代保持风格一致。
- `assets/dashboard-bg.svg`：页面暖色纸感背景与柔光装饰。
- `assets/card-noise.svg`：卡片表面的细微噪点纹理，可作为 `background-image` 叠加。
- `assets/chart-cost-area.svg`：花费趋势图的橙色面积曲线参考素材。
- `assets/ui-icons.svg`：SVG symbol 图标集合，可通过 `<use href="assets/ui-icons.svg#icon-dashboard">` 引用。

## 开发落地建议

1. 先把 `design-tokens.json` 映射到现有 CSS variables，比如 `--surface`、`--primary`、`--shadow-card`。
2. 仪表盘页面保持现有数据结构，只替换视觉层：侧边栏、筛选区、KPI 卡片、图表面板、账号表格。
3. 图表仍建议用 Recharts 实现，颜色与渐变参考 `design-tokens.json.chart` 和 `assets/chart-cost-area.svg`。
4. 卡片背景可以使用两层背景：`linear-gradient(...)` 加 `url("./assets/card-noise.svg")`。
5. 图标如果不想引入新 icon 包，可以直接拆用 `assets/ui-icons.svg` 里的 symbol。

## 关键尺寸

- 设计画布参考：`2560 x 1306`
- 左侧栏宽度：`256px`
- 主内容边距：`32px`
- 面板圆角：`26px`
- KPI 卡片高度：约 `112px`
- 趋势图区域高度：约 `282px`
- 控件高度：`40px`

## 视觉方向

浅色玻璃拟态数据作战台：暖米色页面底、白色半透明卡片、蓝色主操作、橙色花费趋势、绿色成功/输出、红色失败、紫色缓存。整体目标是比当前页面更有层次，但仍然可以用普通 CSS/SVG/Recharts 实现。
