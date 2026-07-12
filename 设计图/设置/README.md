# 设置页面重设计交付包

这个文件夹是针对“设置”页面的独立设计交付，与仪表盘、额度、服务商、2FA、智能体和日志设计包分开管理。它只新增设计稿与开发参考素材，不修改现有业务代码。

## 文件说明

- settings-redesign.png：使用内置 imagegen 生成的高保真设置控制中心效果图。
- settings-reference.html：可直接打开的静态参考实现，覆盖运行模式、基础设置、代理连接、管理 API、高级参数与工具自动化。
- design-tokens.json：颜色、字体、间距、圆角、阴影和核心设置卡尺寸。
- imagegen-prompt.md：本次效果图使用的完整提示词及参考图用途。
- assets/settings-bg.svg：暖米色、冷蓝色、薄荷绿与淡紫色组合的背景。
- assets/card-noise.svg：设置卡片轻微纸感噪点。
- assets/settings-icons.svg：侧栏、模式、设置项和工具操作图标集合。
- assets/connection-flow.svg：应用、本地代理与上游服务的连接关系素材。

## 开发落地建议

1. 页面建议拆成 SettingsHeader、AppModeSelector、BasePreferencesCard、ProxyConnectionCard、ManagementApiCard、AdvancedSettingsCard、SettingsToolsStrip。
2. 设置项仍使用现有 AppSettings 和 management config 数据，不需要为新布局增加后端字段。
3. 代理连接字段继续使用本地 draft；只有点击“保存连接设置”才写盘并更新左下角端口，未保存状态应在卡片标题或底部明确显示。
4. 管理 API 中支持实时应用的设置仍同时写入 settings.json 和运行中的管理接口，UI 应区分“已保存”和“正在写入”。
5. 高级设置使用两列紧凑网格，但危险或影响范围较大的选项要保留说明文字，不能只显示开关名称。
6. 密钥输入保持遮罩，清除远程密钥使用次级危险样式，避免与保存按钮竞争。
7. 运行模式三选一应由 operating_mode 和 connection_mode 共同派生，切换时整卡可点击。
8. Cloudflared 与 Antigravity 作为工具卡放在页面底部，不与核心代理配置混在同一个表单。
9. 窄屏时模式卡和双栏设置卡改为单列；设置行可以纵向堆叠，但字段标签和当前值必须始终可见。

## 关键尺寸

- 主效果图画布：1663 x 946
- 左侧栏参考宽度：约 216px
- 主内容边距：约 26px
- 运行模式区域高度：约 132px
- 设置行高度：约 38-42px
- 基础设置与代理连接卡高度：约 252px
- 管理 API 与高级设置卡高度：约 302px
- 工具与自动化区域高度：约 104px

## 视觉方向

浅色 Settings Control Center：继续使用套系中的暖米色与冷蓝色背景，蓝色表示本地代理和保存操作，绿色表示运行健康，紫色表示远程与高级能力，橙色表示实验性或未保存状态，红色只用于清除密钥和重置。核心目标是把原来的长列表变成可快速理解、可安全操作的设置控制台。
