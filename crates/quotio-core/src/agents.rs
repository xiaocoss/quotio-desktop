use std::{fs, path::Path, time::UNIX_EPOCH};

use quotio_types::{default_cli_agents, AgentStatus, CliAgentSummary};

pub fn detect_agents() -> Vec<AgentStatus> {
    default_cli_agents().into_iter().map(detect_agent).collect()
}

pub fn detect_agent(agent: CliAgentSummary) -> AgentStatus {
    let executable = quotio_platform::find_first_executable(&agent.binary_names, &[]);
    let installed = executable.is_some();
    let binary_path = executable
        .as_ref()
        .map(|executable| executable.path.display().to_string());
    let version = executable
        .as_ref()
        .and_then(|executable| quotio_platform::read_version(&executable.path));
    let configured = is_configured(&agent);
    let last_configured = if configured {
        latest_config_modified_at(&agent)
    } else {
        None
    };

    AgentStatus {
        agent,
        installed,
        configured,
        binary_path,
        version,
        last_configured,
    }
}

pub fn is_configured(agent: &CliAgentSummary) -> bool {
    if agent.id == "gemini-cli" {
        return shell_profiles()
            .into_iter()
            .filter_map(|path| fs::read_to_string(path).ok())
            .any(|content| {
                content.contains("CODE_ASSIST_ENDPOINT")
                    || content.contains("GOOGLE_GEMINI_BASE_URL")
                    || content.contains("GEMINI_API_KEY")
            });
    }

    agent.config_paths.iter().any(|path| {
        let path = quotio_platform::expand_home_path(path);
        fs::read_to_string(path)
            .map(|content| content_contains_managed_proxy(&content))
            .unwrap_or(false)
    })
}

pub fn shell_profiles() -> Vec<std::path::PathBuf> {
    let home = quotio_platform::home_dir();
    let mut profiles = vec![
        home.join(".zshrc"),
        home.join(".bashrc"),
        home.join(".bash_profile"),
        home.join(".profile"),
        home.join(".config").join("fish").join("config.fish"),
    ];

    if cfg!(target_os = "windows") {
        profiles.push(
            home.join("Documents")
                .join("WindowsPowerShell")
                .join("Microsoft.PowerShell_profile.ps1"),
        );
        profiles.push(
            home.join("Documents")
                .join("PowerShell")
                .join("Microsoft.PowerShell_profile.ps1"),
        );
    }

    profiles
}

fn content_contains_managed_proxy(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    lower.contains("127.0.0.1")
        || lower.contains("localhost")
        || lower.contains("cliproxyapi")
        || lower.contains("quotio")
}

fn latest_config_modified_at(agent: &CliAgentSummary) -> Option<String> {
    agent
        .config_paths
        .iter()
        .map(|path| quotio_platform::expand_home_path(path))
        .filter_map(|path| modified_unix_seconds(&path))
        .max()
        .map(|seconds| seconds.to_string())
}

fn modified_unix_seconds(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_proxy_markers_in_config_text() {
        assert!(content_contains_managed_proxy(
            "base_url = \"http://127.0.0.1:28317/v1\""
        ));
        assert!(content_contains_managed_proxy(
            "[model_providers.cliproxyapi]"
        ));
        assert!(!content_contains_managed_proxy("model = \"gpt-5\""));
    }
}
