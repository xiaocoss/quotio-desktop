use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use quotio_types::{
    default_available_models, default_cli_agents, default_model_slots, mask_secret,
    AgentBackupFile, AgentConfigMode, AgentConfigStorageOption, AgentConfigurationRequest,
    AgentConfigurationResult, AgentSetupMode, AvailableModel, ModelSlot, RawAgentConfigOutput,
    RawConfigFormat, SavedAgentConfiguration,
};
use serde_json::{json, Map, Value};

use crate::ManagementCoreError;

const MANAGED_BLOCK_START: &str = "# >>> Quotio managed CLIProxyAPI >>>";
const MANAGED_BLOCK_END: &str = "# <<< Quotio managed CLIProxyAPI <<<";

pub fn read_agent_configuration(
    agent_id: &str,
) -> Result<SavedAgentConfiguration, ManagementCoreError> {
    let agent = find_agent(agent_id)?;
    let mut base_url = None;
    let mut api_key = None;
    let mut model_slots = BTreeMap::new();
    let mut is_proxy_configured = false;

    match agent_id {
        "claude-code" => {
            let path = quotio_platform::expand_home_path("~/.claude/settings.json");
            if let Some(json) = read_json_file(&path) {
                if let Some(env) = json.get("env").and_then(Value::as_object) {
                    base_url = env
                        .get("ANTHROPIC_BASE_URL")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                    api_key = env
                        .get("ANTHROPIC_AUTH_TOKEN")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                    insert_env_slot(
                        env,
                        &mut model_slots,
                        ModelSlot::Opus,
                        "ANTHROPIC_DEFAULT_OPUS_MODEL",
                    );
                    insert_env_slot(
                        env,
                        &mut model_slots,
                        ModelSlot::Sonnet,
                        "ANTHROPIC_DEFAULT_SONNET_MODEL",
                    );
                    insert_env_slot(
                        env,
                        &mut model_slots,
                        ModelSlot::Haiku,
                        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
                    );
                }
                is_proxy_configured = base_url.as_deref().is_some_and(is_local_proxy_url);
            }
        }
        "codex" => {
            let path = quotio_platform::expand_home_path("~/.codex/config.toml");
            let content = fs::read_to_string(path).unwrap_or_default();
            base_url = extract_toml_value(&content, "base_url");
            api_key = extract_toml_value(&content, "experimental_bearer_token");
            if let Some(model) = extract_toml_value(&content, "model") {
                model_slots.insert(ModelSlot::Sonnet, model);
            }
            is_proxy_configured = content.contains("model_providers.cliproxyapi")
                || base_url.as_deref().is_some_and(is_local_proxy_url);
        }
        "gemini-cli" => {
            for path in crate::agents::shell_profiles() {
                let content = fs::read_to_string(path).unwrap_or_default();
                if content.is_empty() {
                    continue;
                }
                base_url = extract_export_value(&content, "CODE_ASSIST_ENDPOINT")
                    .or_else(|| extract_export_value(&content, "GOOGLE_GEMINI_BASE_URL"));
                api_key = extract_export_value(&content, "GEMINI_API_KEY");
                if base_url.is_some() || api_key.is_some() {
                    break;
                }
            }
            is_proxy_configured = base_url.as_deref().is_some_and(is_local_proxy_url);
        }
        "amp" => {
            let settings_path = quotio_platform::expand_home_path("~/.config/amp/settings.json");
            let secrets_path = quotio_platform::expand_home_path("~/.local/share/amp/secrets.json");
            if let Some(json) = read_json_file(&settings_path) {
                base_url = json
                    .get("amp.url")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
            }
            if let Some(json) = read_json_file(&secrets_path) {
                api_key = json
                    .as_object()
                    .and_then(|object| object.values().find_map(Value::as_str))
                    .map(ToOwned::to_owned);
            }
            is_proxy_configured = base_url.as_deref().is_some_and(is_local_proxy_url);
        }
        "opencode" => {
            let path = quotio_platform::expand_home_path("~/.config/opencode/opencode.json");
            if let Some(json) = read_json_file(&path) {
                let options = json
                    .get("provider")
                    .and_then(|value| value.get("quotio"))
                    .and_then(|value| value.get("options"));
                base_url = options
                    .and_then(|value| value.get("baseURL"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                api_key = options
                    .and_then(|value| value.get("apiKey"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                is_proxy_configured = base_url.as_deref().is_some_and(is_local_proxy_url);
            }
        }
        "factory-droid" => {
            let path = quotio_platform::expand_home_path("~/.factory/config.json");
            if let Some(json) = read_json_file(&path) {
                let first = json
                    .get("custom_models")
                    .and_then(Value::as_array)
                    .and_then(|models| models.first());
                base_url = first
                    .and_then(|value| value.get("base_url"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                api_key = first
                    .and_then(|value| value.get("api_key"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                is_proxy_configured = base_url.as_deref().is_some_and(is_local_proxy_url);
            }
        }
        _ => {}
    }

    Ok(SavedAgentConfiguration {
        agent_id: agent.id.clone(),
        base_url,
        api_key_masked: api_key.as_deref().map(mask_secret),
        model_slots,
        is_proxy_configured,
        backups: list_agent_backups(agent_id)?,
    })
}

pub fn configure_agent(
    request: AgentConfigurationRequest,
) -> Result<AgentConfigurationResult, ManagementCoreError> {
    let agent = find_agent(&request.agent_id)?;
    let raw_configs = raw_configs_for_request(&request)?;
    let mut result = AgentConfigurationResult {
        success: true,
        config_type: agent.config_type.clone(),
        mode: request.mode.clone(),
        config_path: raw_configs
            .first()
            .and_then(|config| config.target_path.clone()),
        auth_path: raw_configs
            .iter()
            .find(|config| {
                config
                    .filename
                    .as_deref()
                    .is_some_and(|name| name.contains("auth") || name.contains("secret"))
            })
            .and_then(|config| config.target_path.clone()),
        shell_config: raw_configs
            .iter()
            .find(|config| matches!(config.format, RawConfigFormat::Shell))
            .map(|config| config.content.clone()),
        raw_configs: raw_configs.clone(),
        instructions: instructions_for(&request.agent_id, &request.mode, &request.setup_mode),
        models_configured: models_configured(&request),
        error: None,
        backup_path: None,
        backups: Vec::new(),
    };

    if matches!(request.mode, AgentConfigMode::Automatic) {
        let write_result = match request.setup_mode {
            AgentSetupMode::Proxy => write_proxy_config(&request, &raw_configs),
            AgentSetupMode::Default => write_default_config(&request),
        }?;
        result.config_path = write_result.config_path;
        result.auth_path = write_result.auth_path;
        result.backup_path = write_result.backup_path;
    }

    result.backups = list_agent_backups(&request.agent_id)?;
    Ok(result)
}

pub fn list_agent_backups(agent_id: &str) -> Result<Vec<AgentBackupFile>, ManagementCoreError> {
    let agent = find_agent(agent_id)?;
    let mut backups = quotio_platform::list_backups(&backup_namespace(agent_id))
        .map_err(|error| unavailable("无法读取 agent 备份列表", error))?
        .into_iter()
        .map(|backup| AgentBackupFile {
            path: backup.path.display().to_string(),
            timestamp_unix_seconds: backup.timestamp_unix_seconds,
            agent_id: agent_id.to_string(),
            display_name: agent.display_name.clone(),
        })
        .collect::<Vec<_>>();
    backups.sort_by(|left, right| {
        right
            .timestamp_unix_seconds
            .cmp(&left.timestamp_unix_seconds)
    });
    Ok(backups)
}

pub fn restore_agent_backup(
    agent_id: &str,
    backup_path: &str,
) -> Result<AgentConfigurationResult, ManagementCoreError> {
    // 防路径穿越:backup_path 来自前端,必须是本 agent 备份目录里真实存在的某个备份,
    // 不能是任意路径——否则可借「恢复」把任意文件读出并写进 agent 配置/敏感位置。
    let known = quotio_platform::list_backups(&backup_namespace(agent_id))
        .map_err(|error| unavailable("无法读取 agent 备份列表", error))?;
    let requested =
        std::fs::canonicalize(backup_path).unwrap_or_else(|_| PathBuf::from(backup_path));
    let is_known = known.iter().any(|backup| {
        std::fs::canonicalize(&backup.path).unwrap_or_else(|_| backup.path.clone()) == requested
    });
    if !is_known {
        return Err(ManagementCoreError::Unavailable(
            "备份路径无效:不在该 agent 的备份目录中。".to_string(),
        ));
    }
    let target_path = backup_restore_target(agent_id, backup_path)?;
    let restored = quotio_platform::restore_backup(
        &PathBuf::from(backup_path),
        &target_path,
        is_sensitive_path(&target_path),
        &backup_namespace(agent_id),
    )
    .map_err(|error| unavailable("无法恢复 agent 备份", error))?;

    let agent = find_agent(agent_id)?;
    Ok(AgentConfigurationResult {
        success: true,
        config_type: agent.config_type,
        mode: AgentConfigMode::Automatic,
        config_path: Some(restored.path.display().to_string()),
        auth_path: None,
        shell_config: None,
        raw_configs: Vec::new(),
        instructions: "已从备份恢复配置。".to_string(),
        models_configured: 0,
        error: None,
        backup_path: restored.backup_path.map(|path| path.display().to_string()),
        backups: list_agent_backups(agent_id)?,
    })
}

pub fn reset_agent_configuration(
    agent_id: &str,
) -> Result<AgentConfigurationResult, ManagementCoreError> {
    let mut request = AgentConfigurationRequest {
        agent_id: agent_id.to_string(),
        mode: AgentConfigMode::Automatic,
        setup_mode: AgentSetupMode::Default,
        storage_option: AgentConfigStorageOption::Json,
        proxy_url: String::new(),
        api_key: String::new(),
        model_slots: default_model_slots(),
        use_oauth: false,
        available_models: default_available_models(),
        reasoning_effort: String::new(),
    };
    if agent_id == "gemini-cli" {
        request.storage_option = AgentConfigStorageOption::Shell;
    }
    configure_agent(request)
}

struct WriteResult {
    config_path: Option<String>,
    auth_path: Option<String>,
    backup_path: Option<String>,
}

fn write_proxy_config(
    request: &AgentConfigurationRequest,
    raw_configs: &[RawAgentConfigOutput],
) -> Result<WriteResult, ManagementCoreError> {
    let mut config_path = None;
    let mut auth_path = None;
    let mut backup_path = None;

    for config in raw_configs {
        if matches!(config.format, RawConfigFormat::Shell)
            && !matches!(request.storage_option, AgentConfigStorageOption::Json)
        {
            let path = preferred_shell_profile();
            let result = write_managed_shell_block(&path, &config.content, &request.agent_id)?;
            backup_path =
                backup_path.or_else(|| result.backup_path.map(|path| path.display().to_string()));
            config_path = config_path.or(Some(path.display().to_string()));
            continue;
        }

        if matches!(config.format, RawConfigFormat::Shell) {
            continue;
        }

        let Some(path) = config.target_path.as_ref().map(PathBuf::from) else {
            continue;
        };
        let result = quotio_platform::write_text_file(
            &path,
            &config.content,
            is_sensitive_path(&path),
            &backup_namespace(&request.agent_id),
        )
        .map_err(|error| unavailable("无法写入 agent 配置", error))?;
        backup_path =
            backup_path.or_else(|| result.backup_path.map(|path| path.display().to_string()));

        if is_sensitive_path(&path) {
            auth_path = Some(path.display().to_string());
        } else {
            config_path = Some(path.display().to_string());
        }
    }

    Ok(WriteResult {
        config_path,
        auth_path,
        backup_path,
    })
}

fn write_default_config(
    request: &AgentConfigurationRequest,
) -> Result<WriteResult, ManagementCoreError> {
    let mut config_path = None;
    let mut backup_path = None;

    match request.agent_id.as_str() {
        "claude-code" => {
            let path = quotio_platform::expand_home_path("~/.claude/settings.json");
            let mut json = read_json_file(&path).unwrap_or_else(|| Value::Object(Map::new()));
            if let Some(env) = json.get_mut("env").and_then(Value::as_object_mut) {
                for key in [
                    "ANTHROPIC_BASE_URL",
                    "ANTHROPIC_AUTH_TOKEN",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
                ] {
                    env.remove(key);
                }
            }
            let content = pretty_json(&json)?;
            let result = quotio_platform::write_text_file(
                &path,
                &content,
                false,
                &backup_namespace(&request.agent_id),
            )
            .map_err(|error| unavailable("无法重置 Claude Code 配置", error))?;
            backup_path = result.backup_path.map(|path| path.display().to_string());
            config_path = Some(path.display().to_string());
        }
        "codex" => {
            let path = quotio_platform::expand_home_path("~/.codex/config.toml");
            let content = fs::read_to_string(&path).unwrap_or_default();
            let filtered = remove_codex_managed_config(&content);
            let result = quotio_platform::write_text_file(
                &path,
                &filtered,
                false,
                &backup_namespace(&request.agent_id),
            )
            .map_err(|error| unavailable("无法重置 Codex 配置", error))?;
            backup_path = result.backup_path.map(|path| path.display().to_string());
            config_path = Some(path.display().to_string());
        }
        "gemini-cli" => {
            let path = preferred_shell_profile();
            let existing = fs::read_to_string(&path).unwrap_or_default();
            let content = remove_managed_block(&existing, &request.agent_id);
            let result = quotio_platform::write_text_file(
                &path,
                &content,
                false,
                &backup_namespace(&request.agent_id),
            )
            .map_err(|error| unavailable("无法重置 Gemini CLI shell 配置", error))?;
            backup_path = result.backup_path.map(|path| path.display().to_string());
            config_path = Some(path.display().to_string());
        }
        "amp" => {
            let path = quotio_platform::expand_home_path("~/.config/amp/settings.json");
            let mut json = read_json_file(&path).unwrap_or_else(|| Value::Object(Map::new()));
            if let Some(object) = json.as_object_mut() {
                object.remove("amp.url");
            }
            let content = pretty_json(&json)?;
            let result = quotio_platform::write_text_file(
                &path,
                &content,
                false,
                &backup_namespace(&request.agent_id),
            )
            .map_err(|error| unavailable("无法重置 Amp 配置", error))?;
            backup_path = result.backup_path.map(|path| path.display().to_string());
            config_path = Some(path.display().to_string());
        }
        "opencode" => {
            let path = quotio_platform::expand_home_path("~/.config/opencode/opencode.json");
            let mut json = read_json_file(&path).unwrap_or_else(|| Value::Object(Map::new()));
            if let Some(providers) = json.get_mut("provider").and_then(Value::as_object_mut) {
                providers.remove("quotio");
            }
            let content = pretty_json(&json)?;
            let result = quotio_platform::write_text_file(
                &path,
                &content,
                false,
                &backup_namespace(&request.agent_id),
            )
            .map_err(|error| unavailable("无法重置 OpenCode 配置", error))?;
            backup_path = result.backup_path.map(|path| path.display().to_string());
            config_path = Some(path.display().to_string());
        }
        "factory-droid" => {
            let path = quotio_platform::expand_home_path("~/.factory/config.json");
            let mut json = read_json_file(&path).unwrap_or_else(|| Value::Object(Map::new()));
            if let Some(models) = json.get_mut("custom_models").and_then(Value::as_array_mut) {
                models.retain(|model| {
                    model
                        .get("base_url")
                        .and_then(Value::as_str)
                        .map(|base_url| !is_local_proxy_url(base_url))
                        .unwrap_or(true)
                });
            }
            let content = pretty_json(&json)?;
            let result = quotio_platform::write_text_file(
                &path,
                &content,
                false,
                &backup_namespace(&request.agent_id),
            )
            .map_err(|error| unavailable("无法重置 Factory Droid 配置", error))?;
            backup_path = result.backup_path.map(|path| path.display().to_string());
            config_path = Some(path.display().to_string());
        }
        _ => {}
    }

    Ok(WriteResult {
        config_path,
        auth_path: None,
        backup_path,
    })
}

fn raw_configs_for_request(
    request: &AgentConfigurationRequest,
) -> Result<Vec<RawAgentConfigOutput>, ManagementCoreError> {
    if matches!(request.setup_mode, AgentSetupMode::Default) {
        return Ok(default_reset_raw_configs(&request.agent_id));
    }

    match request.agent_id.as_str() {
        "claude-code" => Ok(claude_configs(request)?),
        "codex" => Ok(codex_configs(request)),
        "gemini-cli" => Ok(gemini_configs(request)),
        "amp" => Ok(amp_configs(request)),
        "opencode" => Ok(opencode_configs(request)?),
        "factory-droid" => Ok(factory_configs(request)?),
        _ => Err(ManagementCoreError::Unavailable(
            "未知 CLI agent。".to_string(),
        )),
    }
}

fn claude_configs(
    request: &AgentConfigurationRequest,
) -> Result<Vec<RawAgentConfigOutput>, ManagementCoreError> {
    let path = quotio_platform::expand_home_path("~/.claude/settings.json");
    let mut settings = read_json_file(&path).unwrap_or_else(|| Value::Object(Map::new()));
    let object = ensure_object(&mut settings)?;
    let env = object
        .entry("env".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let env = ensure_object(env)?;

    let base_url = strip_v1(&request.proxy_url);
    env.insert(
        "ANTHROPIC_BASE_URL".to_string(),
        Value::String(base_url.clone()),
    );
    env.insert(
        "ANTHROPIC_AUTH_TOKEN".to_string(),
        Value::String(request.api_key.clone()),
    );
    env.insert(
        "ANTHROPIC_DEFAULT_OPUS_MODEL".to_string(),
        Value::String(slot_model(request, ModelSlot::Opus)),
    );
    env.insert(
        "ANTHROPIC_DEFAULT_SONNET_MODEL".to_string(),
        Value::String(slot_model(request, ModelSlot::Sonnet)),
    );
    env.insert(
        "ANTHROPIC_DEFAULT_HAIKU_MODEL".to_string(),
        Value::String(slot_model(request, ModelSlot::Haiku)),
    );
    object.insert(
        "model".to_string(),
        Value::String(slot_model(request, ModelSlot::Opus)),
    );

    let shell = format!(
        "export ANTHROPIC_BASE_URL={}\nexport ANTHROPIC_AUTH_TOKEN={}\nexport ANTHROPIC_DEFAULT_OPUS_MODEL={}\nexport ANTHROPIC_DEFAULT_SONNET_MODEL={}\nexport ANTHROPIC_DEFAULT_HAIKU_MODEL={}\n",
        shell_quote(&base_url),
        shell_quote(&request.api_key),
        shell_quote(&slot_model(request, ModelSlot::Opus)),
        shell_quote(&slot_model(request, ModelSlot::Sonnet)),
        shell_quote(&slot_model(request, ModelSlot::Haiku)),
    );

    let mut configs = Vec::new();
    if matches!(
        request.storage_option,
        AgentConfigStorageOption::Json | AgentConfigStorageOption::Both
    ) {
        configs.push(RawAgentConfigOutput {
            format: RawConfigFormat::Json,
            content: pretty_json(&settings)?,
            filename: Some("settings.json".to_string()),
            target_path: Some(path.display().to_string()),
            instructions: "合并保存到 ~/.claude/settings.json。".to_string(),
        });
    }
    if matches!(
        request.storage_option,
        AgentConfigStorageOption::Shell | AgentConfigStorageOption::Both
    ) {
        configs.push(RawAgentConfigOutput {
            format: RawConfigFormat::Shell,
            content: shell,
            filename: None,
            target_path: Some(preferred_shell_profile().display().to_string()),
            instructions: "追加到 shell profile 的 Quotio 管理块。".to_string(),
        });
    }
    Ok(configs)
}

fn codex_configs(request: &AgentConfigurationRequest) -> Vec<RawAgentConfigOutput> {
    let config_path = quotio_platform::expand_home_path("~/.codex/config.toml");
    let reasoning = if request.reasoning_effort.trim().is_empty() {
        "high"
    } else {
        request.reasoning_effort.trim()
    };
    // base_url 必须带 /v1（Codex 桌面应用要求；官方文档示例即 http://host:port/v1）。
    let trimmed_url = request.proxy_url.trim().trim_end_matches('/');
    let base_url = if trimmed_url.ends_with("/v1") {
        trimmed_url.to_string()
    } else {
        format!("{trimmed_url}/v1")
    };
    // 对齐 CLIProxyAPI 官方文档：必须有 model_provider + supports_websockets（App 走 websocket）。
    let managed = format!(
        "# CLIProxyAPI Configuration for Codex\nmodel_provider = \"cliproxyapi\"\nmodel = \"{}\"\nmodel_reasoning_effort = \"{}\"\nplan_mode_reasoning_effort = \"{}\"\nsupports_websockets = true\n\n[model_providers.cliproxyapi]\nname = \"cliproxyapi\"\nbase_url = \"{}\"\nexperimental_bearer_token = \"{}\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n",
        toml_escape(&slot_model(request, ModelSlot::Sonnet)),
        toml_escape(reasoning),
        toml_escape(reasoning),
        toml_escape(&base_url),
        toml_escape(&request.api_key),
    );
    let existing = fs::read_to_string(&config_path).unwrap_or_default();
    let config = merge_codex_config(&existing, &managed);

    vec![RawAgentConfigOutput {
        format: RawConfigFormat::Toml,
        content: config,
        filename: Some("config.toml".to_string()),
        target_path: Some(config_path.display().to_string()),
        instructions: "合并保存到 ~/.codex/config.toml；代理密钥写入 provider 配置，不覆盖 ~/.codex/auth.json。".to_string(),
    }]
}

/// 直接把 Codex 代理配置合并写入 `~/.codex/config.toml`，**不创建备份文件**。
/// 用于 codex 一键启动这种临时流程（停止时用内存备份还原），避免每次启动都刷一个备份。
pub fn write_codex_proxy_config_no_backup(
    request: &AgentConfigurationRequest,
) -> Result<(), ManagementCoreError> {
    for config in codex_configs(request) {
        let Some(path) = config.target_path.as_ref().map(PathBuf::from) else {
            continue;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(&path, &config.content).map_err(|error| {
            ManagementCoreError::Unavailable(format!("无法写入 Codex 配置：{error}"))
        })?;
    }
    Ok(())
}

fn gemini_configs(request: &AgentConfigurationRequest) -> Vec<RawAgentConfigOutput> {
    let base_url = strip_v1(&request.proxy_url);
    let content = if request.use_oauth {
        format!("export CODE_ASSIST_ENDPOINT={}\n", shell_quote(&base_url))
    } else {
        format!(
            "export GOOGLE_GEMINI_BASE_URL={}\nexport GEMINI_API_KEY={}\n",
            shell_quote(&base_url),
            shell_quote(&request.api_key),
        )
    };

    vec![RawAgentConfigOutput {
        format: RawConfigFormat::Shell,
        content,
        filename: None,
        target_path: Some(preferred_shell_profile().display().to_string()),
        instructions: "保存到 shell profile 的 Quotio 管理块。".to_string(),
    }]
}

fn amp_configs(request: &AgentConfigurationRequest) -> Vec<RawAgentConfigOutput> {
    let settings_path = quotio_platform::expand_home_path("~/.config/amp/settings.json");
    let secrets_path = quotio_platform::expand_home_path("~/.local/share/amp/secrets.json");
    let base_url = strip_v1(&request.proxy_url);
    let mut secrets = Map::new();
    secrets.insert(
        format!("apiKey@{}", base_url),
        Value::String(request.api_key.clone()),
    );

    vec![
        RawAgentConfigOutput {
            format: RawConfigFormat::Json,
            content: pretty_json(&json!({ "amp.url": base_url }))
                .unwrap_or_else(|_| "{}".to_string()),
            filename: Some("settings.json".to_string()),
            target_path: Some(settings_path.display().to_string()),
            instructions: "保存到 ~/.config/amp/settings.json。".to_string(),
        },
        RawAgentConfigOutput {
            format: RawConfigFormat::Json,
            content: pretty_json(&Value::Object(secrets)).unwrap_or_else(|_| "{}".to_string()),
            filename: Some("secrets.json".to_string()),
            target_path: Some(secrets_path.display().to_string()),
            instructions: "保存到 ~/.local/share/amp/secrets.json。".to_string(),
        },
        RawAgentConfigOutput {
            format: RawConfigFormat::Shell,
            content: format!(
                "export AMP_URL={}\nexport AMP_API_KEY={}\n",
                shell_quote(&base_url),
                shell_quote(&request.api_key)
            ),
            filename: None,
            target_path: Some(preferred_shell_profile().display().to_string()),
            instructions: "可选：以环境变量方式配置 Amp。".to_string(),
        },
    ]
}

fn opencode_configs(
    request: &AgentConfigurationRequest,
) -> Result<Vec<RawAgentConfigOutput>, ManagementCoreError> {
    let path = quotio_platform::expand_home_path("~/.config/opencode/opencode.json");
    let base_url = format!("{}/v1", strip_v1(&request.proxy_url));
    let mut config = read_json_file(&path).unwrap_or_else(|| Value::Object(Map::new()));
    let object = ensure_object(&mut config)?;
    object
        .entry("$schema".to_string())
        .or_insert_with(|| Value::String("https://opencode.ai/config.json".to_string()));
    let provider = object
        .entry("provider".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let providers = ensure_object(provider)?;

    let models = models_for(request)
        .into_iter()
        .map(|model| (model.name.clone(), opencode_model_config(&model.name)))
        .collect::<Map<_, _>>();

    providers.insert(
        "quotio".to_string(),
        json!({
            "models": models,
            "name": "Quotio",
            "npm": "@ai-sdk/anthropic",
            "options": {
                "apiKey": request.api_key.clone(),
                "baseURL": base_url,
                "litellmProxy": true
            }
        }),
    );

    Ok(vec![RawAgentConfigOutput {
        format: RawConfigFormat::Json,
        content: pretty_json(&config)?,
        filename: Some("opencode.json".to_string()),
        target_path: Some(path.display().to_string()),
        instructions: "合并 provider.quotio 到 ~/.config/opencode/opencode.json。".to_string(),
    }])
}

fn factory_configs(
    request: &AgentConfigurationRequest,
) -> Result<Vec<RawAgentConfigOutput>, ManagementCoreError> {
    let path = quotio_platform::expand_home_path("~/.factory/config.json");
    let base_url = format!("{}/v1", strip_v1(&request.proxy_url));
    let custom_models = models_for(request)
        .into_iter()
        .map(|model| {
            json!({
                "model": model.name.clone(),
                "model_display_name": model.name,
                "base_url": base_url.clone(),
                "api_key": request.api_key.clone(),
                "provider": "openai"
            })
        })
        .collect::<Vec<_>>();

    Ok(vec![RawAgentConfigOutput {
        format: RawConfigFormat::Json,
        content: pretty_json(&json!({ "custom_models": custom_models }))?,
        filename: Some("config.json".to_string()),
        target_path: Some(path.display().to_string()),
        instructions: "保存到 ~/.factory/config.json。".to_string(),
    }])
}

fn default_reset_raw_configs(agent_id: &str) -> Vec<RawAgentConfigOutput> {
    vec![RawAgentConfigOutput {
        format: RawConfigFormat::Text,
        content: format!("将移除 {} 中由 Quotio/CLIProxyAPI 管理的配置块。", agent_id),
        filename: Some("reset-instructions.txt".to_string()),
        target_path: None,
        instructions: "恢复默认配置不会删除用户其它配置。".to_string(),
    }]
}

fn write_managed_shell_block(
    path: &Path,
    content: &str,
    agent_id: &str,
) -> Result<quotio_platform::SafeWriteResult, ManagementCoreError> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    let mut next = remove_managed_block(&existing, agent_id);
    if !next.ends_with('\n') && !next.is_empty() {
        next.push('\n');
    }
    next.push_str(&format!(
        "{} {}\n{}{} {}\n",
        MANAGED_BLOCK_START, agent_id, content, MANAGED_BLOCK_END, agent_id
    ));
    quotio_platform::write_text_file(path, &next, false, &backup_namespace(agent_id))
        .map_err(|error| unavailable("无法写入 shell profile", error))
}

fn remove_managed_block(content: &str, agent_id: &str) -> String {
    let start = format!("{} {}", MANAGED_BLOCK_START, agent_id);
    let end = format!("{} {}", MANAGED_BLOCK_END, agent_id);
    let mut result = Vec::new();
    let mut skipping = false;

    for line in content.lines() {
        if line.trim() == start {
            skipping = true;
            continue;
        }
        if line.trim() == end {
            skipping = false;
            continue;
        }
        if !skipping {
            result.push(line);
        }
    }

    let mut output = result.join("\n");
    if !output.is_empty() {
        output.push('\n');
    }
    output
}

fn find_agent(agent_id: &str) -> Result<quotio_types::CliAgentSummary, ManagementCoreError> {
    default_cli_agents()
        .into_iter()
        .find(|agent| agent.id == agent_id)
        .ok_or_else(|| ManagementCoreError::Unavailable("未知 CLI agent。".to_string()))
}

fn insert_env_slot(
    env: &Map<String, Value>,
    model_slots: &mut BTreeMap<ModelSlot, String>,
    slot: ModelSlot,
    key: &str,
) {
    if let Some(value) = env.get(key).and_then(Value::as_str) {
        model_slots.insert(slot, value.to_string());
    }
}

fn read_json_file(path: &Path) -> Option<Value> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn pretty_json(value: &Value) -> Result<String, ManagementCoreError> {
    serde_json::to_string_pretty(value)
        .map(|content| format!("{}\n", content))
        .map_err(|error| ManagementCoreError::Unavailable(format!("JSON 生成失败：{}", error)))
}

fn ensure_object(value: &mut Value) -> Result<&mut Map<String, Value>, ManagementCoreError> {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value
        .as_object_mut()
        .ok_or_else(|| ManagementCoreError::Unavailable("配置对象格式无效。".to_string()))
}

fn strip_v1(value: &str) -> String {
    value
        .trim()
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .to_string()
}

fn shell_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn toml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn slot_model(request: &AgentConfigurationRequest, slot: ModelSlot) -> String {
    request
        .model_slots
        .get(&slot)
        .cloned()
        .or_else(|| default_model_slots().get(&slot).cloned())
        .unwrap_or_else(|| "gemini-claude-sonnet-4-5".to_string())
}

fn models_for(request: &AgentConfigurationRequest) -> Vec<AvailableModel> {
    if request.available_models.is_empty() {
        default_available_models()
    } else {
        request.available_models.clone()
    }
}

fn models_configured(request: &AgentConfigurationRequest) -> usize {
    match request.agent_id.as_str() {
        "claude-code" => 3,
        "codex" | "amp" => 1,
        "gemini-cli" => 0,
        "opencode" | "factory-droid" => models_for(request).len(),
        _ => 0,
    }
}

fn opencode_model_config(model_name: &str) -> Value {
    let display_name = model_name
        .split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    let (context, output, attachment) = if model_name.contains("claude") {
        (200_000, 64_000, true)
    } else if model_name.contains("gemini") {
        (1_048_576, 65_536, true)
    } else if model_name.contains("gpt") {
        (400_000, 32_768, true)
    } else {
        (128_000, 16_384, false)
    };

    let mut value = json!({
        "name": display_name,
        "limit": { "context": context, "output": output },
        "attachment": attachment,
        "modalities": {
            "input": if attachment { vec!["text", "image"] } else { vec!["text"] },
            "output": ["text"]
        }
    });

    if model_name.contains("thinking")
        || model_name.contains("codex")
        || model_name.starts_with("gpt-5")
    {
        if let Some(object) = value.as_object_mut() {
            object.insert("reasoning".to_string(), Value::Bool(true));
        }
    }

    value
}

fn merge_codex_config(existing: &str, managed: &str) -> String {
    let filtered = remove_codex_managed_config(existing);
    // 托管块拆成「顶层键」和「[model_providers.cliproxyapi] 表」两段：
    // 顶层键（model_provider / supports_websockets 等）必须放在**所有表之前**，
    // 否则会被并进上一个表里（TOML 作用域），导致类型错误（如 boolean 落进字符串环境变量表）。
    let (managed_top, managed_table) = split_managed_block(managed);

    let mut content = String::new();
    let top = managed_top.trim();
    if !top.is_empty() {
        content.push_str(top);
        content.push_str("\n\n");
    }
    let body = filtered.trim();
    if !body.is_empty() {
        content.push_str(body);
        content.push_str("\n\n");
    }
    let table = managed_table.trim();
    if !table.is_empty() {
        content.push_str(table);
    }
    content.push('\n');
    content
}

/// 把托管配置块拆成「顶层键部分」和「[model_providers.cliproxyapi] 表部分」。
fn split_managed_block(managed: &str) -> (String, String) {
    let mut top = Vec::new();
    let mut table = Vec::new();
    let mut in_table = false;
    for line in managed.lines() {
        if line
            .trim_start()
            .starts_with("[model_providers.cliproxyapi")
        {
            in_table = true;
        }
        if in_table {
            table.push(line);
        } else {
            top.push(line);
        }
    }
    (top.join("\n"), table.join("\n"))
}

fn remove_codex_managed_config(existing: &str) -> String {
    let mut filtered = Vec::new();
    let mut skipping_cliproxy_section = false;

    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let section = trimmed.trim_start_matches('[').trim_end_matches(']');
            skipping_cliproxy_section = section == "model_providers.cliproxyapi"
                || section.starts_with("model_providers.cliproxyapi.");
            if skipping_cliproxy_section {
                continue;
            }
        }

        if skipping_cliproxy_section {
            continue;
        }

        if trimmed == "# CLIProxyAPI Configuration for Codex CLI"
            || trimmed == "# CLIProxyAPI Configuration for Codex"
        {
            continue;
        }

        let is_top_level_managed = !trimmed.starts_with('[')
            && [
                "model_provider",
                "model",
                "model_reasoning_effort",
                "plan_mode_reasoning_effort",
                "supports_websockets",
            ]
            .iter()
            .any(|key| trimmed.starts_with(&format!("{} =", key)));
        if is_top_level_managed {
            continue;
        }

        filtered.push(line);
    }

    let mut output = filtered.join("\n").trim().to_string();
    if !output.is_empty() {
        output.push('\n');
    }
    output
}

fn extract_toml_value(content: &str, key: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let trimmed = line.trim();
        if !trimmed.starts_with(key) {
            return None;
        }
        let (_, value) = trimmed.split_once('=')?;
        Some(value.trim().trim_matches('"').to_string()).filter(|value| !value.is_empty())
    })
}

fn extract_export_value(content: &str, key: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let trimmed = line.trim();
        if !trimmed.starts_with("export ") || !trimmed.contains(key) {
            return None;
        }
        let (_, value) = trimmed.split_once('=')?;
        Some(
            value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string(),
        )
        .filter(|value| !value.is_empty())
    })
}

fn preferred_shell_profile() -> PathBuf {
    if cfg!(target_os = "windows") {
        return quotio_platform::home_dir()
            .join("Documents")
            .join("PowerShell")
            .join("Microsoft.PowerShell_profile.ps1");
    }

    if std::env::var("SHELL")
        .map(|shell| shell.contains("bash"))
        .unwrap_or(false)
    {
        quotio_platform::home_dir().join(".bashrc")
    } else {
        quotio_platform::home_dir().join(".zshrc")
    }
}

fn backup_restore_target(
    agent_id: &str,
    backup_path: &str,
) -> Result<PathBuf, ManagementCoreError> {
    let file_name = Path::new(backup_path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();

    let target = if file_name.starts_with("settings.json") && agent_id == "claude-code" {
        "~/.claude/settings.json"
    } else if file_name.starts_with("config.toml") && agent_id == "codex" {
        "~/.codex/config.toml"
    } else if file_name.starts_with("auth.json") && agent_id == "codex" {
        "~/.codex/auth.json"
    } else if file_name.starts_with("settings.json") && agent_id == "amp" {
        "~/.config/amp/settings.json"
    } else if file_name.starts_with("secrets.json") && agent_id == "amp" {
        "~/.local/share/amp/secrets.json"
    } else if file_name.starts_with("opencode.json") && agent_id == "opencode" {
        "~/.config/opencode/opencode.json"
    } else if file_name.starts_with("config.json") && agent_id == "factory-droid" {
        "~/.factory/config.json"
    } else {
        return Err(ManagementCoreError::Unavailable(
            "无法根据备份文件推断恢复目标。".to_string(),
        ));
    };

    Ok(quotio_platform::expand_home_path(target))
}

fn backup_namespace(agent_id: &str) -> String {
    format!("agent-{}", agent_id)
}

fn is_sensitive_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(|name| name.contains("auth") || name.contains("secret"))
        .unwrap_or(false)
}

fn is_local_proxy_url(value: &str) -> bool {
    value.contains("127.0.0.1") || value.contains("localhost")
}

fn instructions_for(agent_id: &str, mode: &AgentConfigMode, setup_mode: &AgentSetupMode) -> String {
    match (mode, setup_mode) {
        (AgentConfigMode::Manual, AgentSetupMode::Proxy) => {
            format!(
                "复制下面的配置并按路径保存，即可让 {} 使用 Quotio 代理。",
                agent_id
            )
        }
        (AgentConfigMode::Automatic, AgentSetupMode::Proxy) => {
            format!("已写入 {} 的 Quotio 代理配置。", agent_id)
        }
        (_, AgentSetupMode::Default) => {
            format!("已移除 {} 中由 Quotio 管理的代理配置。", agent_id)
        }
    }
}

fn unavailable(context: &'static str, error: std::io::Error) -> ManagementCoreError {
    ManagementCoreError::Unavailable(format!("{}：{}", context, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_codex_managed_section_without_dropping_user_sections() {
        let input = r#"model_provider = "cliproxyapi"
model = "managed"

[model_providers.cliproxyapi]
base_url = "http://127.0.0.1:28317/v1"

[profiles.user]
model = "gpt-5"
"#;

        let output = remove_codex_managed_config(input);
        assert!(!output.contains("model_providers.cliproxyapi"));
        assert!(output.contains("[profiles.user]"));
    }

    #[test]
    fn merge_codex_config_keeps_top_level_keys_before_tables() {
        // 复现 Codex App 的情况：用户配置末尾是一个值为字符串的环境变量表。
        let existing =
            "personality = \"x\"\n\n[runtime.env]\nSKY_PIPE = \"1\"\nCLI_PATH = \"c:/x\"\n";
        let managed = "# CLIProxyAPI Configuration for Codex\nmodel_provider = \"cliproxyapi\"\nsupports_websockets = true\n\n[model_providers.cliproxyapi]\nname = \"cliproxyapi\"\nbase_url = \"http://127.0.0.1:28317/v1\"\n";
        let merged = merge_codex_config(existing, managed);
        let first_table = merged.find('[').expect("应有表头");
        // 顶层键（含布尔 supports_websockets）必须在第一个表之前，否则会被并进字符串环境变量表里。
        assert!(merged.find("model_provider").unwrap() < first_table);
        assert!(merged.find("supports_websockets").unwrap() < first_table);
        // 字符串环境变量键不应紧挨着我们的布尔顶层键。
        assert!(!merged.contains("\"1\"\nsupports_websockets"));
    }

    #[test]
    fn replaces_managed_shell_block_for_one_agent() {
        let input = "before\n# >>> Quotio managed CLIProxyAPI >>> gemini-cli\nold\n# <<< Quotio managed CLIProxyAPI <<< gemini-cli\nafter\n";
        let output = remove_managed_block(input, "gemini-cli");
        assert_eq!(output, "before\nafter\n");
    }

    #[test]
    fn codex_proxy_config_uses_provider_token_without_overwriting_codex_auth() {
        let request = AgentConfigurationRequest {
            agent_id: "codex".to_string(),
            mode: AgentConfigMode::Automatic,
            setup_mode: AgentSetupMode::Proxy,
            storage_option: AgentConfigStorageOption::Json,
            proxy_url: "http://127.0.0.1:28317/v1".to_string(),
            api_key: "sk-test-proxy-key".to_string(),
            model_slots: default_model_slots(),
            use_oauth: false,
            available_models: default_available_models(),
            reasoning_effort: String::new(),
        };

        let configs = codex_configs(&request);

        assert_eq!(configs.len(), 1);
        assert!(configs
            .iter()
            .all(|config| config.filename.as_deref() != Some("auth.json")));
        let content = &configs[0].content;
        assert!(content.contains("model_provider = \"cliproxyapi\""));
        assert!(content.contains("base_url = \"http://127.0.0.1:28317/v1\""));
        assert!(content.contains("experimental_bearer_token = \"sk-test-proxy-key\""));
        assert!(content.contains("requires_openai_auth = true"));
    }
}
