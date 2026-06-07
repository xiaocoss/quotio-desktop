// Dev-only mock backend.
//
// When the frontend runs in a plain browser (e.g. `npm run dev` for fast UI
// iteration) there is no Tauri runtime to answer `invoke(...)` calls, so the
// app would get stuck on the loading screen. This module returns a realistic
// `AppState` fixture (modelled after ui/dashboard.png) so every screen renders
// for visual development. It is only wired in via src/lib/tauri.ts when the
// Tauri runtime is absent, and is never used inside the real app.

import type {
  AccountQuota,
  AgentStatus,
  AppState,
  AuthFile,
  AuthMethod,
  ProviderSummary,
  QuotaModelUsage,
  RequestLogEntry,
} from "../types";

type ProviderFlags = {
  uses_browser_auth?: boolean;
  uses_cli_quota?: boolean;
  uses_api_key_auth?: boolean;
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
  provider("codex", "Codex (OpenAI)", "o_auth", "111111", { uses_browser_auth: true }),
  provider("copilot", "GitHub Copilot", "o_auth", "1F2328", { uses_browser_auth: true }),
  provider("kiro", "Kiro (CodeWhisperer)", "o_auth", "6C4CF1", { uses_browser_auth: true }),
  provider("gemini", "Gemini CLI", "cli", "4285F4", { uses_cli_quota: true }),
  provider("claude", "Claude Code", "cli", "D97757", { uses_cli_quota: true }),
  provider("qwen", "Qwen Code", "api_key", "615CED", { uses_api_key_auth: true }),
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
): AccountQuota {
  return {
    provider_id: providerId,
    account_label: accountLabel,
    account_key: accountKey,
    is_forbidden: false,
    status_message: null,
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
  quota("codex-1", "codex", "dev@openai.example.com", "Plus", false, false, [
    model("GPT-5", 72, "5h"),
    model("GPT-5 Codex", 40, "5h"),
  ]),
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
  agentStatus("codex", "Codex CLI", "OpenAI's Codex CLI for GPT-5 models", true, false, "/opt/homebrew/bin/codex", "0.8.1", "~/.codex/config.toml"),
  agentStatus("factory", "Factory Droid", "Factory's AI coding agent", true, true, "~/.local/bin/droid", "2.1.0", "~/.factory/config.json"),
  agentStatus("gemini", "Gemini CLI", "Google's Gemini CLI for Gemini models", true, false, "/usr/local/bin/gemini", "0.4.2", "~/.gemini/settings.json"),
  agentStatus("opencode", "OpenCode", "The open source AI coding agent", true, false, "~/.opencode/bin/opencode", "0.6.0", "~/.opencode/config.json"),
  agentStatus("amp", "Amp CLI", "Sourcegraph's agentic coding tool", false, false, null, null, "~/.amp/config.json"),
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

export async function mockInvoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  switch (command) {
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
