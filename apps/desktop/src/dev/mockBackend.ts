// Dev-only mock backend.
//
// When the frontend runs in a plain browser (e.g. `npm run dev` for fast UI
// iteration) there is no Tauri runtime to answer `invoke(...)` calls, so the
// app would get stuck on the loading screen. This module returns a realistic
// `AppState` fixture (modelled after ui/dashboard.png) so every screen renders
// for visual development. It is only wired in via src/lib/tauri.ts when the
// Tauri runtime is absent, and is never used inside the real app.

import type {
  AccountAuthHealth,
  AccountQuota,
  AccountSummaryRow,
  AgentStatus,
  AppState,
  AuthFile,
  AuthMethod,
  ProviderSummary,
  QuotaModelUsage,
  RequestLogEntry,
  UsageAggregate,
  UsageFilterOptions,
  UsageModelBreakdownRow,
  UsageTimeSeriesPoint,
} from "../types";

type ProviderFlags = {
  uses_browser_auth?: boolean;
  uses_cli_quota?: boolean;
  uses_api_key_auth?: boolean;
  native_oauth?: boolean;
};

function provider(
  id: string,
  display_name: string,
  auth_method: AuthMethod,
  color_hex: string,
  flags: ProviderFlags = {},
): ProviderSummary {
  return {
    id,
    display_name,
    auth_method,
    role: "provider",
    logo_asset_name: `${id}.svg`,
    color_hex,
    oauth_endpoint: auth_method === "o_auth" ? `https://auth.example.com/${id}` : null,
    supports_quota_only_mode: true,
    supports_manual_auth: true,
    uses_browser_auth: flags.uses_browser_auth ?? false,
    uses_cli_quota: flags.uses_cli_quota ?? false,
    uses_api_key_auth: flags.uses_api_key_auth ?? false,
    enabled: true,
    native_oauth: flags.native_oauth ?? false,
  };
}

function authFile(provider: string, index: number, email: string, activeInIde = false): AuthFile {
  return {
    id: `${provider}-${index}`,
    name: `${provider}-${index}.json`,
    provider,
    label: null,
    status: "active",
    status_message: null,
    disabled: false,
    unavailable: false,
    runtime_only: false,
    source: "oauth",
    path: `~/.quotio/auth/${provider}-${index}.json`,
    email,
    account_type: "Pro",
    account: email,
    auth_index: String(index),
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-06-01T00:00:00Z",
    last_refresh: "2026-06-06T10:00:00Z",
    active_in_ide: activeInIde,
  };
}

const providers: ProviderSummary[] = [
  provider("antigravity", "Antigravity", "o_auth", "F2682C", { uses_browser_auth: true }),
  provider("codex", "Codex (OpenAI)", "o_auth", "111111", { uses_browser_auth: true, native_oauth: true }),
  provider("copilot", "GitHub Copilot", "o_auth", "1F2328", { native_oauth: true }),
  provider("kiro", "Kiro (CodeWhisperer)", "o_auth", "6C4CF1", { native_oauth: true }),
  provider("gemini", "Gemini CLI", "o_auth", "4285F4", { uses_cli_quota: true, native_oauth: true }),
  provider("claude", "Claude Code", "o_auth", "D97757", { uses_cli_quota: true, native_oauth: true }),
  provider("qwen", "Qwen Code", "o_auth", "615CED", { native_oauth: true }),
  provider("iflow", "iFlow", "api_key", "2D7CF6", { uses_api_key_auth: true }),
  provider("vertex", "Vertex AI", "service_account", "34A853"),
];

const authFiles: AuthFile[] = [
  authFile("antigravity", 1, "aurora@gmail.com"),
  authFile("antigravity", 2, "borealis@gmail.com"),
  authFile("antigravity", 3, "cosmos@gmail.com", true),
  authFile("antigravity", 4, "delta@gmail.com"),
  authFile("antigravity", 5, "echo@gmail.com"),
  authFile("antigravity", 6, "fjord@gmail.com"),
  authFile("codex", 1, "dev@openai.example.com"),
  authFile("copilot", 1, "team@github.example.com"),
  authFile("copilot", 2, "ops@github.example.com"),
  authFile("kiro", 1, "cloud@kiro.example.com"),
];

function model(name: string, remainingPercent: number, resetLabel: string, count: number | null = null): QuotaModelUsage {
  return {
    model: name,
    used_percent: Math.max(0, 100 - remainingPercent),
    remaining_percent: remainingPercent,
    reset_at: resetLabel,
    count,
  };
}

function quota(
  accountKey: string,
  providerId: string,
  accountLabel: string,
  accountType: string,
  warmingUp: boolean,
  inUse: boolean,
  models: QuotaModelUsage[],
  statusMessage: string | null = null,
): AccountQuota {
  return {
    provider_id: providerId,
    account_label: accountLabel,
    account_key: accountKey,
    is_forbidden: false,
    status_message: statusMessage,
    models,
    account_type: accountType,
    warming_up: warmingUp,
    in_use: inUse,
  };
}

function antigravityModels(claudeLeft: number, claudeReset: string): QuotaModelUsage[] {
  return [
    model("Claude", claudeLeft, claudeReset, 3),
    model("Gemini 3 Flash", 93, "2h"),
    model("Gemini 3 Pro", 100, "4h 59m", 2),
    model("Gemini 3 Image", 100, "4h 59m"),
  ];
}

const accountQuotas: AccountQuota[] = [
  quota("antigravity-1", "antigravity", "aurora@gmail.com", "Pro", true, true, antigravityModels(0, "2d 17h")),
  quota("antigravity-2", "antigravity", "borealis@gmail.com", "Pro", false, true, antigravityModels(9, "1h 47m")),
  quota("antigravity-3", "antigravity", "cosmos@gmail.com", "Pro", false, false, antigravityModels(64, "3h 12m")),
  quota(
    "codex-1",
    "codex",
    "dev@openai.example.com",
    "Plus",
    false,
    false,
    [model("GPT-5", 72, "5h"), model("GPT-5 Codex", 40, "5h")],
    "plan: Plus | until: 2026-07-09 | resets: 2",
  ),
  quota("copilot-1", "copilot", "team@github.example.com", "Business", false, true, [
    model("Claude Sonnet 4.5", 58, "12h"),
    model("GPT-5", 85, "12h"),
    model("Gemini 2.5 Pro", 100, "12h"),
  ]),
  quota("copilot-2", "copilot", "ops@github.example.com", "Business", false, false, [
    model("Claude Sonnet 4.5", 12, "12h"),
    model("GPT-5", 85, "12h"),
    model("Gemini 2.5 Pro", 100, "12h"),
  ]),
  quota("kiro-1", "kiro", "cloud@kiro.example.com", "Pro", false, false, [
    model("Claude", 22, "18h"),
    model("Claude Haiku", 100, "18h"),
  ]),
];

function agentStatus(
  id: string,
  displayName: string,
  description: string,
  installed: boolean,
  configured: boolean,
  binaryPath: string | null,
  version: string | null,
  configPath: string,
): AgentStatus {
  return {
    agent: {
      id,
      display_name: displayName,
      description,
      config_type: "both",
      binary_names: [id],
      config_paths: [configPath],
      docs_url: `https://docs.quotio.dev/agents/${id}`,
    },
    installed,
    configured,
    binary_path: binaryPath,
    version,
    last_configured: configured ? "2026-06-01T10:00:00Z" : null,
  };
}

const agents: AgentStatus[] = [
  agentStatus("claude", "Claude Code", "Anthropic's official CLI for Claude models", true, true, "~/.local/bin/claude", "1.2.0", "~/.claude/settings.json"),
  agentStatus("codex", "Codex", "OpenAI's Codex CLI for GPT-5 models", true, false, "/opt/homebrew/bin/codex", "0.8.1", "~/.codex/config.toml"),
  agentStatus("factory", "Factory Droid", "Factory's AI coding agent", true, true, "~/.local/bin/droid", "2.1.0", "~/.factory/config.json"),
  agentStatus("gemini", "Gemini", "Google's Gemini CLI for Gemini models", true, false, "/usr/local/bin/gemini", "0.4.2", "~/.gemini/settings.json"),
  agentStatus("opencode", "OpenCode", "The open source AI coding agent", true, false, "~/.opencode/bin/opencode", "0.6.0", "~/.opencode/config.json"),
  agentStatus("amp", "Amp", "Sourcegraph's agentic coding tool", false, false, null, null, "~/.amp/config.json"),
];

function reqLog(
  id: string,
  time: string,
  status: number,
  provider: string,
  model: string,
  durationMs: number,
  inputTokens: number,
  outputTokens: number,
): RequestLogEntry {
  return {
    id,
    timestamp: time,
    method: "POST",
    endpoint: "/v1/chat/completions",
    provider,
    model,
    resolved_model: null,
    resolved_provider: null,
    input_tokens: inputTokens,
    output_tokens: outputTokens,
    duration_ms: durationMs,
    status_code: status,
    request_size: 0,
    response_size: 0,
    error_message: null,
    fallback_attempts: null,
    fallback_started_from_cache: false,
    reasoning_effort: durationMs > 5000 ? "high" : "medium",
  };
}

const requestLogs: RequestLogEntry[] = [
  reqLog("r1", "18:24:01", 200, "Gemini", "gemini-claude-opus-4-5-thinking", 30540, 1290, 96500),
  reqLog("r2", "18:23:55", 200, "Openai", "gemini-3-flash-preview", 2140, 880, 2790),
  reqLog("r3", "18:23:40", 200, "Openai", "gemini-3-flash-preview", 2310, 510, 1750),
  reqLog("r4", "18:23:22", 200, "Openai", "gemini-3-flash-preview", 1980, 640, 1530),
  reqLog("r5", "18:22:58", 200, "Gemini", "gemini-3-pro", 17240, 1500, 3460),
  reqLog("r6", "18:22:31", 200, "Gemini", "gemini-3-pro", 9120, 920, 2470),
  reqLog("r7", "18:22:05", 200, "Gemini", "gemini-claude-opus-4-5-thinking", 36500, 2100, 40300),
  reqLog("r8", "18:21:42", 200, "Gemini", "gemini-claude-opus-4-5-thinking", 17500, 1640, 3340),
  reqLog("r9", "18:21:18", 429, "Gemini", "gemini-3-pro", 860, 0, 0),
  reqLog("r10", "18:20:50", 200, "Openai", "gemini-3-flash-preview", 1760, 420, 980),
];

export const mockAppState: AppState = {
  migration_phase: "completed",
  platform: { os: "macos", family: "unix", arch: "aarch64" },
  settings: {
    operating_mode: "full",
    connection_mode: "local",
    proxy_host: "127.0.0.1",
    proxy_port: 28317,
    allow_remote: false,
    launch_at_login: true,
    notifications_enabled: true,
    theme: "system",
    language: "zh-CN",
    routing_strategy: "round-robin",
    debug: false,
    proxy_url: "",
    logging_to_file: false,
    logs_max_total_size_mb: 0,
    session_affinity: false,
    session_affinity_ttl: "1h",
    max_retry_credentials: 0,
    disable_cooling: false,
    disable_image_generation: false,
    force_model_prefix: false,
    passthrough_headers: false,
    reasoning_effort: "",
    force_model: "",
    request_retry: 3,
    max_retry_interval_seconds: 30,
    remote_endpoint_url: null,
    remote_management_key: null,
    codex_app_path: "",
    codex_launch_mode: "app",
    codex_bound_account: "",
    codex_model: "",
    codex_reasoning: "high",
    codex_api_key: "",
    codex_profiles: [
      {
        id: "codex-mock-daily",
        name: "日常-5.5极高",
        launch_mode: "app",
        bound_account: "codex-demo@example.com-plus",
        proxy_url: "http://127.0.0.1:28317",
        model: "gpt-5.5",
        reasoning: "xhigh",
        api_key: "sk-pool-a-demo",
      },
      {
        id: "codex-mock-spare",
        name: "备用-5.4中",
        launch_mode: "cli",
        bound_account: "codex-spare@example.com-free",
        proxy_url: "http://127.0.0.1:28317",
        model: "gpt-5.4",
        reasoning: "medium",
        api_key: "sk-pool-b-demo",
      },
    ],
    scheduler_rule: "off",
    scheduler_min_hold_minutes: 10,
    scheduler_switch_margin_minutes: 15,
  },
  proxy: {
    status: "running",
    endpoint: "http://127.0.0.1:28317",
    management_endpoint: "http://127.0.0.1:28317",
    pid: 43127,
    binary_path: "~/Library/Application Support/Quotio/bin/cli-proxy-api",
    config_path: "~/.quotio/config.yaml",
    auth_dir: "~/.quotio/auth",
    resource_dir: "~/Library/Application Support/Quotio/bin",
    exit_code: null,
    crash_count: 0,
    health: { ok: true, checked_at_unix_seconds: 1749200000, message: "代理健康检查通过" },
    message: "运行中",
  },
  proxy_resources: {
    current_platform: "macos-aarch64",
    resource_root: "~/Library/Application Support/Quotio/bin",
    current_resource_dir: "~/Library/Application Support/Quotio/bin/macos-aarch64",
    expected_binary_names: ["cli-proxy-api"],
    detected_binary_path: "~/Library/Application Support/Quotio/bin/macos-aarch64/cli-proxy-api",
    has_current_platform_binary: true,
    platforms: [],
    message: "已检测到当前平台二进制",
  },
  providers,
  management: {
    auth_files: authFiles,
    usage: {
      usage: {
        total_requests: 0,
        success_count: 0,
        failure_count: 0,
        total_tokens: 0,
        input_tokens: 0,
        output_tokens: 0,
      },
      failed_requests: 0,
    },
    api_keys: ["quotio-live-xxxxxxxx4C3C", "proxyp-live-xxxxxxxxocal"],
    config: {
      debug: false,
      proxy_url: null,
      routing_strategy: "round-robin",
      request_retry: 3,
      max_retry_interval: 30,
      logging_to_file: true,
      request_log: false,
      quota_exceeded: { switch_project: true, switch_preview_model: false },
    },
    logs: {
      lines: ["[2026-06-06 10:00:00] proxy started on 127.0.0.1:28317"],
      line_count: 1,
      latest_timestamp: 1749200000,
    },
    latest_version: null,
  },
  auth_files: authFiles,
  quotas: accountQuotas,
  logs: requestLogs,
  agents: agents,
  api_keys: [
    { value: "quotio-live-xxxxxxxx4C3C", masked_value: "quotio••••••4C3C", source: "local" },
    { value: "proxyp-live-xxxxxxxxocal", masked_value: "proxyp••••••ocal", source: "local" },
  ],
  api_key_bindings: [
    { api_key: "proxyp-live-xxxxxxxxocal", provider_id: "cp-1" },
    { api_key: "sk-pool-a-demo", provider_id: "codex" }, // 日常方案密钥绑到 codex(不告警);备用方案未绑定(告警)
  ],
  request_stats: {
    total_requests: 0,
    successful_requests: 0,
    failed_requests: 0,
    total_input_tokens: 0,
    total_output_tokens: 0,
    average_duration_ms: 0,
  },
  fallback: {
    is_enabled: true,
    is_route_caching_enabled: false,
    virtual_models: [
      {
        id: "vm-1",
        name: "gemini-claude-opus-4-5-thinking",
        is_enabled: true,
        fallback_entries: [
          { id: "fe-1", provider_id: "antigravity", model_id: "gemini-claude-opus-4-5-thinking", priority: 1 },
        ],
      },
    ],
  },
  fallback_runtime: {
    route_states: [],
    available_models: [
      { id: "gemini-claude-opus-4-5-thinking", name: "gemini-claude-opus-4-5-thinking", provider: "antigravity", is_default: true },
      { id: "gemini-3-flash", name: "Gemini 3 Flash", provider: "antigravity", is_default: false },
      { id: "gemini-3-pro", name: "Gemini 3 Pro", provider: "antigravity", is_default: false },
    ],
    model_discovery_status: "ready",
  },
  credentials: {
    availability: "available",
    local_management_key_configured: true,
    remote_management_key_configured: false,
    remote_management_key_masked: null,
    message: "本地管理密钥已配置",
  },
  platform_features: {
    launch_at_login_available: true,
    launch_at_login_enabled: true,
    notifications_available: true,
    notifications_enabled: true,
    file_manager_available: true,
    message: "",
  },
  config_root: "~/Library/Application Support/Quotio",
};

// Commands that should resolve to the full AppState snapshot.
const APP_STATE_COMMANDS = new Set<string>([
  "get_app_state",
  "refresh_management_state",
  "detect_agents",
  "refresh_fallback_route_state",
  "discover_available_models_state",
  "start_proxy",
  "stop_proxy",
  "restart_proxy",
  "check_proxy_health",
  "save_settings",
  "update_fallback_configuration",
  "set_launch_at_login",
  "clear_remote_management_key",
]);

let mockState: AppState = mockAppState;

type MockCustomProvider = {
  id: string; name: string; base_url: string; api_key: string; kind: string;
  prefix: string; default_model: string; models?: string[]; proxy_mode?: string;
  keys: { id: string; label: string; api_key: string; enabled: boolean; weight: number }[];
};

function parseMockModels(raw: unknown): string[] {
  if (typeof raw !== "string") return [];
  return [...new Set(raw.split(/[,\s]+/).map((m) => m.trim()).filter(Boolean))];
}

let mockCustomProviders: MockCustomProvider[] = [
  { id: "cp-1", name: "DeepSeek 代理", base_url: "https://proxy.deepseek.com/v1", api_key: "", kind: "openai", prefix: "", default_model: "deepseek-coder", keys: [
    { id: "k1a", label: "主力", api_key: "sk-ds-main-xxx", enabled: true, weight: 2 },
    { id: "k1b", label: "备用", api_key: "sk-ds-backup-xxx", enabled: true, weight: 1 },
    { id: "k1c", label: "已过期", api_key: "sk-ds-expired-xxx", enabled: false, weight: 1 },
  ]},
  { id: "cp-2", name: "公司内部网关", base_url: "https://ai-gateway.internal.corp/api", api_key: "", kind: "openai", prefix: "", default_model: "gpt-4-turbo", keys: [
    { id: "k2a", label: "默认", api_key: "gw-key-xxx", enabled: true, weight: 1 },
  ]},
  { id: "cp-3", name: "OpenRouter", base_url: "https://openrouter.ai/api/v1", api_key: "", kind: "openai", prefix: "openrouter", default_model: "auto", keys: [] },
];

let mockBindings = [...(mockAppState.api_key_bindings ?? [])];
let nextCpId = 4;
let nextKeyId = 100;

function stateWithSettings(settings: AppState["settings"]): AppState {
  const endpoint = `http://${settings.proxy_host}:${settings.proxy_port}`;
  return {
    ...mockState,
    settings,
    proxy: {
      ...mockState.proxy,
      endpoint,
      management_endpoint:
        settings.connection_mode === "remote" && settings.remote_endpoint_url
          ? settings.remote_endpoint_url
          : `${endpoint}/v0/management`,
    },
  };
}

function mockUsageAggregate(): UsageAggregate {
  const input = requestLogs.reduce((sum, log) => sum + (log.input_tokens ?? 0), 0);
  const output = requestLogs.reduce((sum, log) => sum + (log.output_tokens ?? 0), 0);
  const total = input + output;
  const success = requestLogs.filter((log) => (log.status_code ?? 0) < 400).length;
  // cached (cache-read) tokens are a subset of input, so keep hit-rate <= 100%.
  const cached = Math.round(input * 0.55);
  return {
    total_requests: requestLogs.length,
    success_requests: success,
    failed_requests: requestLogs.length - success,
    success_rate: requestLogs.length ? (success / requestLogs.length) * 100 : 0,
    account_count: 4,
    total_tokens: total,
    input_tokens: input,
    output_tokens: output,
    reasoning_tokens: 1280,
    cached_tokens: cached,
    cache_creation_tokens: 0,
    cache_read_tokens: 0,
    input_token_ratio: total ? (input / total) * 100 : 0,
    output_token_ratio: total ? (output / total) * 100 : 0,
    cache_hit_rate: input ? (cached / input) * 100 : 0,
    avg_latency_ms: 12000,
    estimated_cost: null,
    prices_configured: false,
  };
}

function mockAccountSummary(): AccountSummaryRow[] {
  const base = Date.UTC(2026, 5, 6, 10, 0, 0);
  return authFiles.slice(0, 5).map((file, index) => {
    const total = 40 - index * 7;
    const failed = index;
    return {
      account: file.email ?? file.name,
      provider: file.provider,
      total_requests: total,
      success_requests: total - failed,
      failed_requests: failed,
      success_rate: total ? ((total - failed) / total) * 100 : 0,
      total_tokens: 120000 - index * 18000,
      input_tokens: 90000 - index * 12000,
      output_tokens: 30000 - index * 6000,
      estimated_cost: null,
      last_request_ms: base - index * 600000,
      last_request: new Date(base - index * 600000).toISOString(),
    };
  });
}

function mockAuthHealth(): AccountAuthHealth[] {
  const h = (
    account: string,
    auth: number,
    rate: number,
    server: number,
    ok: number,
    last: number,
  ): AccountAuthHealth => ({
    account,
    recent_total: auth + rate + server + ok,
    auth_failures: auth,
    rate_limited: rate,
    server_errors: server,
    successes: ok,
    last_status_code: last,
    recommend_reauth: ok === 0 && auth >= 2 && auth >= rate + server,
  });
  return [
    h("aurora@gmail.com", 0, 0, 0, 12, 200), // healthy
    h("borealis@gmail.com", 0, 6, 1, 8, 429), // rate limited
    h("cosmos@gmail.com", 0, 0, 7, 9, 500), // failing (server)
    h("delta@gmail.com", 4, 0, 1, 0, 401), // genuine auth failure → re-auth
    h("echo@gmail.com", 0, 1, 2, 10, 200), // partial
  ];
}

function mockFilterOptions(): UsageFilterOptions {
  return {
    accounts: authFiles.map((file) => file.email ?? file.name),
    providers: ["antigravity", "codex", "copilot", "kiro"],
    models: ["gemini-3-pro", "gemini-3-flash-preview", "gpt-5.5", "claude-opus-4-5"],
    channels: ["oauth", "api_key"],
    api_keys: [{ hash: "mock-key-hash", alias: null }],
  };
}

function mockTimeseries(): UsageTimeSeriesPoint[] {
  const base = Date.UTC(2026, 5, 6, 0, 0, 0);
  return Array.from({ length: 8 }, (_, index) => {
    const input = 12000 + index * 1600;
    const cached = Math.round(input * 0.5);
    const output = 3000 + index * 420;
    return {
      bucket: `${String(index * 3).padStart(2, "0")}:00`,
      bucket_start_ms: base + index * 3 * 3_600_000,
      total_requests: 20 + index * 3,
      success_requests: 18 + index * 3,
      failed_requests: 2,
      input_tokens: input,
      output_tokens: output,
      cached_tokens: cached,
      uncached_input_tokens: input - cached,
      total_tokens: input + output,
      estimated_cost: null,
    };
  });
}

function mockModelBreakdown(): UsageModelBreakdownRow[] {
  return [
    { model: "gpt-5.5", reqs: 120, input: 90000, output: 22000, cached: 60000 },
    { model: "gemini-3-pro", reqs: 64, input: 48000, output: 12000, cached: 20000 },
    { model: "claude-opus-4-5", reqs: 30, input: 26000, output: 8000, cached: 9000 },
  ].map((row) => ({
    model: row.model,
    total_requests: row.reqs,
    input_tokens: row.input,
    output_tokens: row.output,
    cached_tokens: row.cached,
    uncached_input_tokens: row.input - row.cached,
    total_tokens: row.input + row.output,
    cache_hit_rate: row.input ? (row.cached / row.input) * 100 : 0,
    estimated_cost: null,
  }));
}

export async function mockInvoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  switch (command) {
    case "query_usage_stats":
      return mockUsageAggregate() as unknown as T;
    case "query_account_summary":
      return mockAccountSummary() as unknown as T;
    case "list_usage_filter_options":
      return mockFilterOptions() as unknown as T;
    case "query_usage_timeseries":
      return mockTimeseries() as unknown as T;
    case "query_usage_model_breakdown":
      return mockModelBreakdown() as unknown as T;
    case "query_account_auth_health":
      return mockAuthHealth() as unknown as T;
    case "list_custom_providers":
      return mockCustomProviders as unknown as T;
    case "key_router_available":
      return false as unknown as T; // 演示防呆警告(有绑定 + 插件缺失 → 横幅)
    case "add_custom_provider": {
      const cp: MockCustomProvider = {
        id: `cp-${nextCpId++}`,
        name: (args?.name as string) ?? "",
        base_url: (args?.base_url as string) ?? "",
        api_key: "",
        kind: (args?.kind as string) ?? "openai",
        prefix: (args?.prefix as string) ?? "",
        default_model: "",
        models: parseMockModels(args?.models),
        proxy_mode: (args?.proxyMode as string) === "direct" ? "direct" : "",
        keys: (args?.api_key as string)
          ? [{ id: `k-${nextKeyId++}`, label: "默认", api_key: args!.api_key as string, enabled: true, weight: 1 }]
          : [],
      };
      mockCustomProviders = [...mockCustomProviders, cp];
      return mockCustomProviders as unknown as T;
    }
    case "update_custom_provider": {
      mockCustomProviders = mockCustomProviders.map((p) =>
        p.id === args?.id
          ? { ...p, name: (args?.name as string) ?? p.name, base_url: (args?.base_url as string) ?? p.base_url, kind: (args?.kind as string) ?? p.kind, prefix: (args?.prefix as string) ?? p.prefix, models: args?.models !== undefined ? parseMockModels(args?.models) : p.models, proxy_mode: args?.proxyMode !== undefined ? ((args?.proxyMode as string) === "direct" ? "direct" : "") : p.proxy_mode }
          : p,
      );
      return mockCustomProviders as unknown as T;
    }
    case "delete_custom_provider": {
      mockCustomProviders = mockCustomProviders.filter((p) => p.id !== args?.id);
      mockBindings = mockBindings.filter((b) => b.provider_id !== args?.id);
      return mockCustomProviders as unknown as T;
    }
    case "add_provider_key": {
      mockCustomProviders = mockCustomProviders.map((p) =>
        p.id === args?.providerId
          ? { ...p, keys: [...p.keys, { id: `k-${nextKeyId++}`, label: (args?.label as string) ?? "", api_key: (args?.apiKey as string) ?? "", enabled: true, weight: 1 }] }
          : p,
      );
      return mockCustomProviders as unknown as T;
    }
    case "remove_provider_key": {
      mockCustomProviders = mockCustomProviders.map((p) =>
        p.id === args?.providerId ? { ...p, keys: p.keys.filter((k) => k.id !== args?.keyId) } : p,
      );
      return mockCustomProviders as unknown as T;
    }
    case "toggle_provider_key": {
      mockCustomProviders = mockCustomProviders.map((p) =>
        p.id === args?.providerId
          ? { ...p, keys: p.keys.map((k) => (k.id === args?.keyId ? { ...k, enabled: !k.enabled } : k)) }
          : p,
      );
      return mockCustomProviders as unknown as T;
    }
    case "get_api_key_bindings":
      return mockBindings as unknown as T;
    case "set_api_key_binding": {
      const ak = args?.apiKey as string;
      const pid = args?.providerId as string;
      mockBindings = mockBindings.filter((b) => b.api_key !== ak);
      if (pid) mockBindings.push({ api_key: ak, provider_id: pid });
      mockState = { ...mockState, api_key_bindings: [...mockBindings] };
      return mockBindings as unknown as T;
    }
    case "get_model_prices":
    case "set_model_prices":
      return [] as unknown as T;
    case "detect_codex_app":
      return "C:\\Program Files\\WindowsApps\\OpenAI.Codex_x64\\app\\Codex.exe" as unknown as T;
    case "list_codex_launch_accounts":
      return [
        { key: "codex-demo@example.com-plus", email: "demo@example.com", disabled: false },
        { key: "codex-spare@example.com-free", email: "spare@example.com", disabled: true },
      ] as unknown as T;
    case "codex_start":
      return "已启动 Codex 应用（mock）" as unknown as T;
    case "codex_stop":
      return "已停止 Codex 并还原配置（mock）" as unknown as T;
    case "codex_launch_active":
      return false as unknown as T;
    case "codex_active_profile":
      return null as unknown as T;
    case "fetch_codex_models":
      return ["gpt-5.5", "gpt-5.4", "gpt-5.4-mini", "gpt-5.3-codex-spark", "codex-auto-review"] as unknown as T;
    case "fetch_codex_reasoning_levels": {
      const model = String(args?.model ?? "").trim().toLowerCase();
      if (model === "gpt-5.6-sol" || model === "gpt-5.6-terra") {
        return ["low", "medium", "high", "xhigh", "max", "ultra"] as unknown as T;
      }
      if (model === "gpt-5.6-luna") {
        return ["low", "medium", "high", "xhigh", "max"] as unknown as T;
      }
      if (/^gpt-5(?:$|\.(?:[0-5])(?:$|[.-]))/.test(model)) {
        return ["low", "medium", "high", "xhigh"] as unknown as T;
      }
      return [] as unknown as T;
    }
    case "save_settings": {
      const settings = args?.settings as AppState["settings"] | undefined;
      if (settings) {
        mockState = stateWithSettings(settings);
      }
      return mockState as unknown as T;
    }
    case "get_management_proxy_url":
      return "" as unknown as T;
    case "discover_available_models":
      return mockAppState.fallback_runtime.available_models as unknown as T;
    case "credential_status":
      return mockState.credentials as unknown as T;
    case "open_config_root":
      return undefined as unknown as T;
    case "start_management_oauth":
      return {
        url: "https://accounts.example.com/o/oauth2/auth?client_id=mock&redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback&scope=openid+email&state=mock-state-token",
        state: "mock-state-token",
      } as unknown as T;
    case "poll_management_oauth":
      return { status: "pending" } as unknown as T;
    case "submit_oauth_callback":
      return undefined as unknown as T;
    case "import_auth_token":
      return undefined as unknown as T;
    case "native_oauth_start":
      return {
        login_id: "mock-native-login-id",
        auth_url: "https://example.com/oauth?mock=true",
        user_code: "",
        verification_uri: "",
        provider_id: (args as Record<string, unknown>)?.providerId ?? "codex",
        flow: "authorization_code",
      } as unknown as T;
    case "native_oauth_complete":
      return { status: "pending", error: null, provider_id: "codex", account_email: null } as unknown as T;
    case "native_oauth_cancel":
    case "native_oauth_submit_callback":
      return undefined as unknown as T;
    case "consume_codex_reset_credit":
      // Dev-only: pretend the reset succeeded; the UI then re-fetches quotas.
      return undefined as unknown as T;
    default:
      if (APP_STATE_COMMANDS.has(command)) return mockState as unknown as T;
      // Most remaining commands also return AppState; fall back to it so the UI
      // never crashes during browser-only iteration.
      return mockState as unknown as T;
  }
}

export function isMockEnv(): boolean {
  return import.meta.env.DEV && typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);
}
