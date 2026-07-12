# 日志页面重设计交付包

这个文件夹是针对“日志”页面的独立设计交付，与其他页面设计包分开管理。它只新增设计稿与开发参考素材，不修改现有业务代码。

## 文件说明

- logs-redesign.png：使用内置 imagegen 生成的高保真日志页面效果图。
- logs-reference.html：可直接用浏览器打开的静态参考实现，包含概览指标、请求健康条、筛选工具栏、请求表格和响应式规则。
- proxy-logs-redesign.png：代理日志标签激活时的高保真效果图。
- proxy-logs-reference.html：代理运行概览、级别筛选与实时日志流的静态参考实现。
- design-tokens.json：颜色、字体、间距、圆角、阴影和表格关键尺寸。
- imagegen-prompt.md：本次主效果图的最终提示词和参考图用途。
- proxy-imagegen-prompt.md：代理日志效果图使用的完整提示词。
- assets/logs-bg.svg：暖米色、冷蓝色和轻橙色组合的页面背景。
- assets/card-noise.svg：卡片轻微纸感噪点。
- assets/log-icons.svg：侧栏、筛选、状态和操作图标的 SVG symbol 集合。
- assets/request-health.svg：请求状态分布条，可直接嵌入概览区。
- assets/latency-sparkline.svg：平均耗时指标使用的迷你趋势素材。

## 开发落地建议

1. 页面建议拆成 LogsHeader、LogsSummaryMetrics、RequestHealthStrip、LogsFilterToolbar、RequestLogsTable、LogsPagination。
2. “请求健康”中的 2xx、4xx、5xx 数量应由现有请求状态码聚合得到，不需要新增持久化字段。
3. 表格数据量较大时建议使用虚拟滚动或分页，表头保持 sticky；筛选变化时保留当前滚动位置或明确回到第一页。
4. 状态颜色统一映射：2xx 使用绿色，4xx 使用橙色，5xx 使用红色；慢请求是独立标记，不应覆盖 HTTP 状态。
5. 时间、耗时、Token 数量使用 tabular numbers；账号继续脱敏，不在详情入口之外展示完整敏感信息。
6. 筛选器建议统一维护为一个状态对象，并生成可序列化查询参数，方便刷新后恢复筛选条件。
7. request-health.svg 可直接使用，也可以按相同比例在 React 中用三个 CSS segment 动态绘制。
8. 窄屏下保持表格横向滚动，不要隐藏状态、模型、耗时和 Token 核心列。
9. 代理日志来自原始字符串数组；前端可以通过正则提取时间、INFO/WARN/ERROR 和来源用于着色，解析失败时应原样显示整行。
10. 代理日志默认只渲染最近 200 行，自动滚动仅在用户位于底部时生效，用户向上查看历史时不要强制拉回底部。

## 关键尺寸

- 主效果图画布：1752 x 898
- 左侧栏参考宽度：约 228px
- 主内容边距：约 28px
- 指标卡高度：约 94px
- 请求健康条高度：约 64px
- 表头高度：约 42px
- 请求行高：约 42px
- 桌面首屏显示：约 12 行
- 代理日志行高：约 34px
- 代理日志首屏显示：约 16 行

## 视觉方向

浅色 Request Observatory：白色半透明卡片承载高密度信息，蓝色表示当前模式和筛选，绿色表示成功请求，橙色用于慢请求与 4xx，红色只用于 5xx 和清空日志。设计重点不是增加装饰，而是让用户更快回答三个问题：当前请求是否健康、异常集中在哪里、哪一条记录值得继续查看。
