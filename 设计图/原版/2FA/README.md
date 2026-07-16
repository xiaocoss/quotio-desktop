# 2FA 页面重设计交付包

这个文件夹是针对“2FA 管理”页面的独立设计交付，和 `仪表盘`、`额度`、`服务商` 分开管理。它只新增设计稿和开发参考素材，不改动现有业务代码。

## 文件说明

- `2fa-redesign.png`：使用 imagegen 生成的高保真 2FA 页面重设计效果图。
- `2fa-reference.html`：可直接打开的静态参考实现，包含 TOTP 输入区、验证码卡、倒计时环、本地保险箱、已保存空状态和响应式 CSS。
- `design-tokens.json`：颜色、字体、间距、圆角、阴影、验证码卡和保险箱组件尺寸等设计 token。
- `imagegen-prompt.md`：本次主效果图的生图提示词，方便后续迭代。
- `assets/twofa-bg.svg`：2FA 页背景柔光和网格。
- `assets/card-noise.svg`：卡片细微纸感噪点纹理。
- `assets/twofa-icons.svg`：SVG symbol 图标集合。
- `assets/countdown-ring.svg`：TOTP 28 秒倒计时环参考素材。
- `assets/vault-empty.svg`：已保存密钥空状态保险箱插画。

## 开发落地建议

1. 页面组件建议拆成 `TotpGeneratorCard`、`LocalVaultCard`、`SavedTotpPanel`、`TotpCountdownRing`。
2. 验证码显示建议使用等宽或 rounded 数字字体，保持 `letter-spacing`，例如 `428 916`。
3. 倒计时环建议开发时用 SVG circle + `stroke-dasharray` 动态渲染，`countdown-ring.svg` 只是视觉参考。
4. “本地保险箱”三行是安全能力说明：本地保存、加密保险箱、定期备份。可由现有 vault/migration 逻辑直接映射。
5. 已保存列表为空时展示 `vault-empty.svg`；有数据时替换为密钥条目列表，保留顶部搜索、导入、导出操作。
6. 卡片背景推荐使用 `linear-gradient(...)` 叠加 `url("./assets/card-noise.svg")`，保持和其他页面一致。

## 关键尺寸

- 设计画布参考：`2556 x 1306`
- 左侧栏宽度：`256px`
- 主内容边距：`32px`
- 生成器卡片最小高度：约 `468px`
- 验证码展示区高度：约 `154px`
- 倒计时环尺寸：`112px`
- 已保存列表面板最小高度：约 `380px`
- 输入控件高度：`46px`

## 视觉方向

浅色本地 2FA Vault：暖米色背景、白色玻璃卡片、蓝色主操作、青绿色强调本地安全、橙色倒计时、少量薰衣草色表示加密保险箱。目标是让页面从“表单工具”升级成“安全保险箱”，同时仍然能用普通 React/CSS/SVG 实现。
