#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use serde_json::Value;
    use std::path::{Path, PathBuf};

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

        let summary = repair_session_visibility_in_dir(&root).unwrap();

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

        let summary = repair_session_visibility_in_dir(&root).unwrap();

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

        let summary = repair_session_visibility_in_dir(&root).unwrap();

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
