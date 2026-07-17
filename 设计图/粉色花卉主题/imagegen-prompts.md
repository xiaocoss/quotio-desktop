# Image 2 提示词记录

## 统一角色

原创虚构的年轻东亚女性角色：暖棕色眼睛、深色双马尾长卷发、轻薄刘海、粉色丝带和珍珠发饰、浅粉礼服、柔和美容棚拍光。人物只参考用户截图的浪漫粉色氛围，不复制或识别截图中的真人身份。

统一约束：相同面部设定、无文字、无标志、无水印、自然手部与人体结构；界面效果图中的正式文字和控件仍由程序渲染。

## 页面共享提示

```text
Use case: ui-mockup
Asset type: high-fidelity Quotio desktop page
Preserve the archived page's information architecture and all major controls.
Use the accepted original mascot identity when a person appears.
Style: premium selectable rose-floral theme; warm ivory paper, blush translucent cards,
dusty-rose borders and active controls, restrained champagne-gold sparkles,
delicate roses and petals only at outer edges.
Keep semantic green, amber, red, blue and purple states distinguishable.
Feasible with React, CSS, SVG and Recharts. No Codex branding, no watermark,
no decoration over data or controls, no oversized marketing block.
```

页面变化：

- 仪表盘：允许右上角半身人物主视觉；保留筛选器、八个 KPI、趋势图和账号表。
- 额度：人物缩小为头像；重点保留账号卡、健康点、Session/Weekly 进度和重置时间。
- 服务商：保留服务商统计、账号列表、自定义接口和洞察面板。
- 智能体：花卉远离路由流程，保留配置、备份、恢复、重置和发现区。
- 日志：表格近乎纯色，保留筛选器、状态、延迟、Tokens、分页和详情。
- 代理日志：日志正文使用高可读等宽排版，保留级别、来源、自动滚动和清空操作。
- 设置：显示跟随系统、浅色、粉色花卉、深色四个主题选项，粉色花卉为选中态。
- 2FA：OTP 和二维码区域保持纯净高对比，花卉只在外围。
- 悬浮窗：维持紧凑尺寸，使用小头像，保留标签、额度卡和底部代理操作。
- 关于：允许较强人物与品牌主视觉，保留版本、更新、链接和运行环境信息。

## 透明素材共享提示

简单不透明素材使用 Image 2 的纯色键控背景，再通过本地去背脚本输出带 Alpha 的 PNG。

```text
Use case: background-extraction
Create the requested reusable UI subject on a perfectly flat chroma-key background.
No shadow, gradient, texture, floor plane, text, logo, watermark or frame.
Keep the subject separated from the background with crisp edges and generous padding.
```

- 花卉角饰和花瓣使用 `#0000ff` 键控背景。
- 空状态插画与最终人物主视觉使用 `#00ff00` 键控背景。
- 正式透明输出位于 `apps/desktop/public/rose/`；带纯色背景的中间图仅保存在 `角色设定/` 供复现与检查。
