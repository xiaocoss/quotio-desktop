use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use serde_json::Value;

const DEFAULT_PROVIDER_ID: &str = "openai";
const CONFIG_FILE_NAME: &str = "config.toml";
const STATE_DB_FILE: &str = "state_5.sqlite";
const SESSION_DIRS: [&str; 2] = ["sessions", "archived_sessions"];

#[derive(Debug, Clone, serde::Serialize)]
pub struct CodexSessionVisibilityRepairSummary {
    pub instance_count: usize,
    pub mutated_instance_count: usize,
    pub changed_rollout_file_count: usize,
    pub updated_sqlite_row_count: usize,
    pub backup_dirs: Vec<String>,
    pub message: String,
}

pub fn repair_session_visibility_in_default_dir(
) -> Result<CodexSessionVisibilityRepairSummary, String> {
    let root = quotio_platform::expand_home_path("~/.codex");
    repair_session_visibility_in_dir(&root, true)
}

/// Like [`repair_session_visibility_in_default_dir`] but without the on-disk
/// backup. Used by the automatic repair on Codex start/stop: it fires every
/// launch/stop and the change is deterministic + reversible, so keeping a backup
/// per provider switch would just pile up copies of the sessions in `~/.codex`.
pub fn repair_session_visibility_in_default_dir_no_backup(
) -> Result<CodexSessionVisibilityRepairSummary, String> {
    let root = quotio_platform::expand_home_path("~/.codex");
    repair_session_visibility_in_dir(&root, false)
}

pub fn repair_session_visibility_in_dir(
    root: &Path,
    make_backup: bool,
) -> Result<CodexSessionVisibilityRepairSummary, String> {
    if !root.exists() {
        return Err(format!("Codex 配置目录不存在：{}", root.display()));
    }

    let target_provider = read_target_provider(root)?;
    let rollout_files = collect_rollout_files(root)?;
    let rollout_changes = collect_rollout_provider_changes(&rollout_files, &target_provider)?;
    let sqlite_rows_to_update = count_sqlite_rows_to_update(root, &target_provider)?;
    let should_mutate = !rollout_changes.is_empty() || sqlite_rows_to_update > 0;

    let mut backup_dirs = Vec::new();
    if should_mutate {
        if make_backup {
            let backup_dir =
                backup_instance_files(root, &rollout_changes, sqlite_rows_to_update > 0)?;
            backup_dirs.push(backup_dir.to_string_lossy().to_string());
        }

        for path in &rollout_changes {
            repair_rollout_file(path, &target_provider)?;
        }
    }

    let updated_sqlite_row_count = if sqlite_rows_to_update > 0 {
        update_sqlite_rows(root, &target_provider)?
    } else {
        0
    };

    let changed_rollout_file_count = rollout_changes.len();
    let mutated_instance_count = usize::from(changed_rollout_file_count > 0 || updated_sqlite_row_count > 0);
    let message = build_summary_message(
        changed_rollout_file_count,
        updated_sqlite_row_count,
        &backup_dirs,
    );

    Ok(CodexSessionVisibilityRepairSummary {
        instance_count: 1,
        mutated_instance_count,
        changed_rollout_file_count,
        updated_sqlite_row_count,
        backup_dirs,
        message,
    })
}

fn read_target_provider(root: &Path) -> Result<String, String> {
    let path = root.join(CONFIG_FILE_NAME);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(DEFAULT_PROVIDER_ID.to_string());
    };

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || !trimmed.starts_with("model_provider") {
            continue;
        }
        let Some((_, value)) = trimmed.split_once('=') else {
            continue;
        };
        let provider = value
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .trim()
            .to_string();
        if !provider.is_empty() {
            return Ok(provider);
        }
    }

    Ok(DEFAULT_PROVIDER_ID.to_string())
}

fn collect_rollout_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for dir_name in SESSION_DIRS {
        let dir = root.join(dir_name);
        collect_rollout_files_in(&dir, &mut files)?;
    }
    Ok(files)
}

fn collect_rollout_files_in(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };

    for entry in entries {
        let entry = entry.map_err(|error| format!("读取 Codex 会话目录失败：{error}"))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("读取 Codex 会话文件类型失败：{error}"))?;
        if file_type.is_dir() {
            collect_rollout_files_in(&path, files)?;
        } else if is_rollout_file(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn is_rollout_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    name.starts_with("rollout-") && name.ends_with(".jsonl")
}

fn collect_rollout_provider_changes(
    files: &[PathBuf],
    target_provider: &str,
) -> Result<Vec<PathBuf>, String> {
    let mut changes = Vec::new();
    for path in files {
        if rollout_file_needs_provider_repair(path, target_provider)? {
            changes.push(path.clone());
        }
    }
    Ok(changes)
}

fn rollout_file_needs_provider_repair(path: &Path, target_provider: &str) -> Result<bool, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("读取 rollout 文件失败 {}：{error}", path.display()))?;
    Ok(text.lines().any(|line| {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return false;
        };
        is_session_meta_with_different_provider(&value, target_provider)
    }))
}

fn is_session_meta_with_different_provider(value: &Value, target_provider: &str) -> bool {
    if value.get("type").and_then(|value| value.as_str()) != Some("session_meta") {
        return false;
    }
    value
        .get("payload")
        .and_then(|payload| payload.get("model_provider"))
        .and_then(|provider| provider.as_str())
        .map(|provider| provider != target_provider)
        .unwrap_or(true)
}

fn repair_rollout_file(path: &Path, target_provider: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("读取 rollout 文件失败 {}：{error}", path.display()))?;
    let had_trailing_newline = text.ends_with('\n');
    let mut changed = false;
    let mut repaired_lines = Vec::new();

    for line in text.lines() {
        let Ok(mut value) = serde_json::from_str::<Value>(line) else {
            repaired_lines.push(line.to_string());
            continue;
        };
        let mut line_changed = false;
        if is_session_meta_with_different_provider(&value, target_provider) {
            if let Some(payload) = value.get_mut("payload").and_then(|payload| payload.as_object_mut())
            {
                payload.insert(
                    "model_provider".to_string(),
                    Value::String(target_provider.to_string()),
                );
                line_changed = true;
                changed = true;
            }
        }
        // 只重新序列化真正被改的那一行;其余行保留原始字节。原先用粘连的 changed 标志,
        // 导致某行一旦被改,后续所有行都会被 serde 重新序列化、丢失原始键序/格式。
        if line_changed {
            repaired_lines.push(
                serde_json::to_string(&value)
                    .map_err(|error| format!("序列化 rollout 文件失败：{error}"))?,
            );
        } else {
            repaired_lines.push(line.to_string());
        }
    }

    if !changed {
        return Ok(());
    }

    let mut repaired = repaired_lines.join("\n");
    if had_trailing_newline {
        repaired.push('\n');
    }
    std::fs::write(path, repaired)
        .map_err(|error| format!("写入 rollout 文件失败 {}：{error}", path.display()))
}

fn state_db_path(root: &Path) -> PathBuf {
    root.join(STATE_DB_FILE)
}

fn count_sqlite_rows_to_update(root: &Path, target_provider: &str) -> Result<usize, String> {
    let path = state_db_path(root);
    if !path.exists() {
        return Ok(0);
    }
    let connection = Connection::open(&path)
        .map_err(|error| format!("打开 Codex 会话数据库失败 {}：{error}", path.display()))?;
    let columns = thread_columns(&connection)?;
    if !columns.iter().any(|column| column == "model_provider") {
        return Ok(0);
    }

    let where_clause = visibility_where_clause(&columns);
    let sql = format!("SELECT COUNT(*) FROM threads WHERE {where_clause}");
    let count: i64 = connection
        .query_row(&sql, [target_provider], |row| row.get(0))
        .map_err(|error| format!("统计 Codex 会话数据库失败：{error}"))?;
    Ok(count.max(0) as usize)
}

fn update_sqlite_rows(root: &Path, target_provider: &str) -> Result<usize, String> {
    let path = state_db_path(root);
    if !path.exists() {
        return Ok(0);
    }
    let connection = Connection::open(&path)
        .map_err(|error| format!("打开 Codex 会话数据库失败 {}：{error}", path.display()))?;
    let columns = thread_columns(&connection)?;
    if !columns.iter().any(|column| column == "model_provider") {
        return Ok(0);
    }

    let mut assignments = vec!["model_provider = ?1".to_string()];
    if columns.iter().any(|column| column == "has_user_event") {
        assignments.push("has_user_event = 1".to_string());
    }
    if columns.iter().any(|column| column == "thread_source") {
        assignments.push("thread_source = 'user'".to_string());
    }
    let where_clause = visibility_where_clause(&columns);
    let sql = format!(
        "UPDATE threads SET {} WHERE {where_clause}",
        assignments.join(", ")
    );
    connection
        .execute(&sql, [target_provider])
        .map_err(|error| format!("更新 Codex 会话数据库失败：{error}"))
}

fn thread_columns(connection: &Connection) -> Result<Vec<String>, String> {
    let mut statement = connection
        .prepare("PRAGMA table_info(threads)")
        .map_err(|error| format!("读取 Codex 会话数据库结构失败：{error}"))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("读取 Codex 会话数据库结构失败：{error}"))?;
    let mut columns = Vec::new();
    for row in rows {
        columns.push(row.map_err(|error| format!("读取 Codex 会话数据库列失败：{error}"))?);
    }
    Ok(columns)
}

fn visibility_where_clause(columns: &[String]) -> String {
    let mut clauses = vec!["(model_provider IS NULL OR model_provider <> ?1)".to_string()];
    if columns.iter().any(|column| column == "has_user_event") {
        clauses.push("COALESCE(has_user_event, 0) <> 1".to_string());
    }
    if columns.iter().any(|column| column == "thread_source") {
        clauses.push("COALESCE(thread_source, '') <> 'user'".to_string());
    }
    clauses.join(" OR ")
}

fn backup_instance_files(
    root: &Path,
    rollout_changes: &[PathBuf],
    include_sqlite: bool,
) -> Result<PathBuf, String> {
    let backup_dir = root.join(format!(
        "session-visibility-repair-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("读取系统时间失败：{error}"))?
            .as_secs()
    ));
    std::fs::create_dir_all(&backup_dir)
        .map_err(|error| format!("创建可见性修复备份目录失败：{error}"))?;

    for source in rollout_changes {
        let relative = source.strip_prefix(root).unwrap_or(source);
        let destination = backup_dir.join(relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("创建 rollout 备份目录失败：{error}"))?;
        }
        std::fs::copy(source, &destination).map_err(|error| {
            format!(
                "备份 rollout 文件失败 {} -> {}：{error}",
                source.display(),
                destination.display()
            )
        })?;
    }

    if include_sqlite {
        let source = state_db_path(root);
        if source.exists() {
            std::fs::copy(&source, backup_dir.join(STATE_DB_FILE))
                .map_err(|error| format!("备份 Codex 会话数据库失败：{error}"))?;
        }
    }

    Ok(backup_dir)
}

fn build_summary_message(
    changed_rollout_file_count: usize,
    updated_sqlite_row_count: usize,
    backup_dirs: &[String],
) -> String {
    if changed_rollout_file_count == 0 && updated_sqlite_row_count == 0 {
        return "Codex 历史会话可见性已是最新，无需修复".to_string();
    }

    let mut message = format!(
        "已修复 Codex 历史会话可见性：改写 {} 个 rollout 文件，更新 {} 条 SQLite 记录",
        changed_rollout_file_count, updated_sqlite_row_count
    );
    if let Some(dir) = backup_dirs.first() {
        message.push_str(&format!("。修改前已备份到：{}", dir));
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use serde_json::Value;

    #[test]
    fn repairs_rollout_session_meta_provider_to_config_provider() {
        let root = temp_codex_dir("quotio_visibility_rollout");
        std::fs::create_dir_all(root.join("sessions").join("2026")).unwrap();
        std::fs::write(root.join("config.toml"), "model_provider = \"cliproxyapi\"\n").unwrap();
        let rollout = root.join("sessions").join("2026").join("rollout-a.jsonl");
        std::fs::write(
            &rollout,
            "{\"type\":\"session_meta\",\"payload\":{\"model_provider\":\"openai\",\"id\":\"s1\"}}\n{\"type\":\"event\"}\n",
        )
        .unwrap();

        let summary = repair_session_visibility_in_dir(&root, true).unwrap();

        assert_eq!(summary.changed_rollout_file_count, 1);
        assert_eq!(summary.updated_sqlite_row_count, 0);
        assert_eq!(summary.mutated_instance_count, 1);
        assert_eq!(summary.backup_dirs.len(), 1);
        let repaired = std::fs::read_to_string(&rollout).unwrap();
        let first_line = repaired.lines().next().unwrap();
        let parsed: Value = serde_json::from_str(first_line).unwrap();
        assert_eq!(parsed["payload"]["model_provider"], "cliproxyapi");
        assert!(repaired.contains("{\"type\":\"event\"}"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn repairs_threads_sqlite_provider_and_visibility_columns() {
        let root = temp_codex_dir("quotio_visibility_sqlite");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("config.toml"), "model_provider = \"cliproxyapi\"\n").unwrap();
        let db_path = root.join("state_5.sqlite");
        let connection = Connection::open(&db_path).unwrap();
        connection
            .execute(
                "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT, first_user_message TEXT, has_user_event INTEGER, thread_source TEXT)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO threads (id, model_provider, first_user_message, has_user_event, thread_source) VALUES ('s1', 'openai', 'hello', 0, '')",
                [],
            )
            .unwrap();
        drop(connection);

        let summary = repair_session_visibility_in_dir(&root, true).unwrap();

        assert_eq!(summary.changed_rollout_file_count, 0);
        assert_eq!(summary.updated_sqlite_row_count, 1);
        let connection = Connection::open(&db_path).unwrap();
        let row: (String, i64, String) = connection
            .query_row(
                "SELECT model_provider, has_user_event, thread_source FROM threads WHERE id = 's1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(row, ("cliproxyapi".to_string(), 1, "user".to_string()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn leaves_matching_history_unchanged_without_backup() {
        let root = temp_codex_dir("quotio_visibility_noop");
        std::fs::create_dir_all(root.join("sessions")).unwrap();
        std::fs::write(root.join("config.toml"), "model_provider = \"cliproxyapi\"\n").unwrap();
        std::fs::write(
            root.join("sessions").join("rollout-a.jsonl"),
            "{\"type\":\"session_meta\",\"payload\":{\"model_provider\":\"cliproxyapi\"}}\n",
        )
        .unwrap();

        let summary = repair_session_visibility_in_dir(&root, true).unwrap();

        assert_eq!(summary.mutated_instance_count, 0);
        assert_eq!(summary.changed_rollout_file_count, 0);
        assert!(summary.backup_dirs.is_empty());
        assert!(
            std::fs::read_dir(&root)
                .unwrap()
                .flatten()
                .all(|entry| !entry.file_name().to_string_lossy().contains("session-visibility-repair"))
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    fn temp_codex_dir(prefix: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), nanos))
    }
}
