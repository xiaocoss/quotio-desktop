# 关于页面重设计交付包

这个文件夹是针对“关于”页面的独立设计交付，与其他页面设计包分开管理。它只新增设计稿与开发参考素材，不修改现有业务代码。

## 文件说明

- about-redesign.png：使用内置 imagegen 生成的高保真关于页面效果图。
- about-reference.html：可直接打开的静态参考实现，包含品牌区、版本状态、运行环境、本地服务、配置目录和能力说明。
- design-tokens.json：颜色、字体、间距、圆角、阴影和关于页核心组件尺寸。
- imagegen-prompt.md：主效果图使用的完整提示词。
- assets/about-bg.svg：暖米色与冷蓝色组合的页面背景。
- assets/card-noise.svg：卡片轻微纸感噪点。
- assets/quotio-mark.svg：可直接引用的 Quotio 渐变 Q 应用图标。
- assets/about-icons.svg：侧栏、环境、服务、配置和能力图标集合。
- assets/brand-orbit.svg：品牌 Hero 背景中的连接轨道与节点素材。

## 开发落地建议

1. 页面建议拆成 AboutHeroCard、VersionStatusPanel、RuntimeEnvironmentCard、LocalServiceCard、ConfigDataCard、ProductCapabilitiesStrip。
2. 应用版本继续通过 Tauri getVersion 获取，不要在组件中硬编码；设计稿中的 v0.5.13 只是当前截图数据。
3. 版本状态需要覆盖 checking、latest、available 和 error，只有确认已是最新版本时才显示绿色状态。
4. 运行模式应把 operating_mode 的 full、quota_only、remote 映射为可读标签，本稿中的 full 对应“本地代理”。
5. 本地服务状态从 appState.proxy.status 与 health.ok 派生，不能仅根据端点存在就显示健康。
6. 配置目录使用等宽字体，并允许文本选择；复制按钮可以纯前端实现，不需要新增后端接口。
7. 产品能力条只复述现有定位“多服务商 AI 代理与额度管理工具”，不应扩展成营销功能列表。
8. 窄屏时三张详情卡和能力条改为单列，路径文本允许换行但不能被截断到不可读。

## 关键尺寸

- 主效果图画布：1665 x 945
- 左侧栏参考宽度：约 232px
- 主内容边距：约 28px
- 品牌 Hero 高度：约 300px
- 品牌图标：约 148px
- 详情卡高度：约 286px
- 详情行高：约 54px
- 产品能力条高度：约 118px

## 视觉方向

浅色 Product Information Console：品牌区域使用 Quotio 绿色到青蓝色渐变，运行状态使用绿色，环境与端点信息使用蓝色，配置目录使用淡紫色。页面保持技术产品感，不添加虚构的官网、社区、许可证或支持入口。
