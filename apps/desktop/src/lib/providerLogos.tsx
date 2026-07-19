// 各服务商的品牌 logo 映射(素材见 public/quota/provider-logos.svg)。
// 额度页(分组头)与服务商页(provider 卡头)共用,保证同一 provider 图标一致。
// 未命中的 provider(自定义接口 / 内部网关等)走通用层叠图标 logo-generic。
const PROVIDER_LOGO: Record<string, string> = {
  openai: "logo-openai",
  codex: "logo-openai",
  gemini: "logo-gemini",
  claude: "logo-claude",
  anthropic: "logo-claude",
  copilot: "logo-copilot",
  antigravity: "logo-gemini",
  xai: "logo-grok",
  kiro: "logo-kiro",
  glm: "logo-glm",
  trae: "logo-trae",
  deepseek: "logo-deepseek",
  openrouter: "logo-openrouter",
  cursor: "logo-cursor",
};

export function providerLogoId(providerId: string): string {
  const key = providerId.toLowerCase();
  for (const [k, v] of Object.entries(PROVIDER_LOGO)) {
    if (key === k || key.includes(k)) return v;
  }
  return "logo-generic";
}

export function ProviderLogo({ providerId, className = "qr-logo" }: { providerId: string; className?: string }) {
  return (
    <svg className={className} aria-hidden="true">
      <use href={`/quota/provider-logos.svg#${providerLogoId(providerId)}`} />
    </svg>
  );
}
