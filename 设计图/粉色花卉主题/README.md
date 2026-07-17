# Quotio 粉色花卉主题设计包

本目录保存粉色花卉可切换主题的高保真页面参考。页面 PNG 只用于开发对照，程序界面不得直接把整张效果图当背景；人物、花卉、花瓣、空状态图和背景纹理均提供了可直接引用的独立资源。

## 页面效果图

| 页面 | 文件 |
| --- | --- |
| 仪表盘 | `页面设计/仪表盘/dashboard-rose.png` |
| 额度 | `页面设计/额度/quota-rose.png` |
| 服务商 | `页面设计/服务商/providers-rose.png` |
| 智能体 | `页面设计/智能体/agents-rose.png` |
| 请求日志 | `页面设计/日志/logs-rose.png` |
| 代理日志 | `页面设计/日志/proxy-logs-rose.png` |
| 设置 | `页面设计/设置/settings-rose.png` |
| 2FA | `页面设计/2FA/2fa-rose.png` |
| 悬浮窗 | `页面设计/悬浮窗/floating-window-rose.png` |
| 关于 | `页面设计/关于/about-rose.png` |

## 可直接用于程序的素材

正式程序资源位于 `apps/desktop/public/rose/`：

- `character-avatar.png`：方形人物头像，有柔和背景。
- `character-hero.webp`：与页面底色自然融合的人物半身主视觉；使用 `object-fit` 或作为卡片背景引用。
- `rose-corner.png`：带 Alpha 通道的透明花卉角饰，可用 CSS 旋转或镜像。
- `rose-petals.png`：带 Alpha 通道的透明花瓣与星光叠层。
- `empty-endpoints.png`：带 Alpha 通道的透明空状态插画。
- `rose-bg.webp`：页面柔和纸感背景。
- `rose-mark.svg`：使用 `currentColor` 的可缩放玫瑰标识。
- `cloud-filigree-corner.svg`：主内容区单角云纹/卷草纹。
- `cloud-panel-9slice.svg`：关于页、额度页等宽面板的不对称九宫格云纹框。
- `cloud-card-9slice.svg`：智能体卡片使用的小型四角回勾纹。
- `cloud-rail-junction.svg`：日志页侧栏分界线顶部/底部的 T 形云结。
- `cloud-segment-bracket.svg`：仪表盘分段控件左侧括号纹。
- `cloud-botanical-sprig.svg`：人物两侧枝叶卷草线稿。
- `cloud-avatar-wreath.svg`：人物头像或圆形照片背后的枝叶花环。

`素材/` 是按主题归档的完整设计素材包，分为人物、花卉、云纹、背景和空状态。程序只引用 `apps/desktop/public/rose/` 中的正式运行时文件。

## 实现约束

- 中文标题、数据、按钮和表格必须由 React/CSS 渲染，不能使用效果图中的文字。
- 花卉素材只放在页面外缘或留白区域，不能覆盖筛选器、表格、进度条、OTP 或二维码。
- 状态颜色继续使用语义化绿、橙、红、蓝紫色，粉色仅作为主题主色。
- 人物头像和主视觉应使用独立文件，避免从效果图裁切。花卉、花瓣和空状态图使用透明 PNG；人物主视觉使用无抠图色边的独立 WebP。
- 云纹素材使用透明 SVG。长直边优先用 CSS border，角纹使用镜像/旋转或 `border-image`，避免把完整效果图裁成背景。
