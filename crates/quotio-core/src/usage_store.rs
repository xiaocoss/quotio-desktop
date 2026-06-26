//! SQLite persistence + aggregation for request-level usage events.
//!
//! The proxy's `/usage-queue` is a destructive, ~60s-retention buffer (records
//! are removed on read). To build history with arbitrary time ranges we drain it
//! continuously and `INSERT OR IGNORE` each event here (deduped by `event_hash`),
//! then answer the dashboard's KPI / account-summary / filter queries with SQL
//! aggregations. Modeled on cpa-manager's `usage_events` design.

use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, params_from_iter, types::Value, Connection};

use quotio_types::{
    AccountAuthHealth, AccountSummaryRow, ApiKeyOption, ModelPrice, RequestLogEntry,
    UsageAggregate, UsageChartBucket, UsageEvent, UsageFilterOptions, UsageModelBreakdownRow,
    UsageQuery, UsageStatusFilter, UsageTimeSeriesPoint,
};

/// Thread-safe handle to the usage database. Shared via `Arc` between the
/// background collector (writer) and the query commands (readers); a single
/// connection behind a `Mutex` keeps concurrent access trivially correct.
pub struct UsageStore {
    conn: Mutex<Connection>,
}

impl UsageStore {
    /// Open (creating + migrating) the database at `path`.
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        configure(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// In-memory fallback used when the on-disk database can't be opened, so the
    /// app still runs (stats simply won't persist across restarts).
    pub fn open_in_memory() -> Self {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        configure(&conn).expect("init in-memory schema");
        Self {
            conn: Mutex::new(conn),
        }
    }

    /// 清空所有请求记录(usage_events)。日志页「请求」tab 的删除按钮调用。
    /// 注意:请求记录与仪表盘历史用量同源,清空会一并抹掉仪表盘的历史数据。
    pub fn clear(&self) {
        let conn = match self.conn.lock() {
            Ok(conn) => conn,
            Err(poisoned) => {
                eprintln!("[usage_store] clear: mutex poisoned, recovering");
                poisoned.into_inner()
            }
        };
        if let Err(err) = conn.execute("DELETE FROM usage_events", []) {
            eprintln!("[usage_store] clear: delete failed: {err}");
        }
    }

    /// 当前请求记录总数。删除前的二次确认据此告诉用户「将永久删除 N 条」。
    pub fn count(&self) -> usize {
        let conn = match self.conn.lock() {
            Ok(conn) => conn,
            Err(poisoned) => {
                eprintln!("[usage_store] count: mutex poisoned, recovering");
                poisoned.into_inner()
            }
        };
        conn.query_row("SELECT COUNT(*) FROM usage_events", [], |row| row.get::<_, i64>(0))
            .map(|n| n.max(0) as usize)
            .unwrap_or(0)
    }

    /// Persist a batch of drained events. Returns the number of NEW rows
    /// inserted (duplicates are ignored via the `event_hash` UNIQUE constraint).
    pub fn insert_events(&self, events: &[UsageEvent]) -> usize {
        if events.is_empty() {
            return 0;
        }
        let mut conn = match self.conn.lock() {
            Ok(conn) => conn,
            Err(poisoned) => {
                eprintln!("[usage_store] insert_events: mutex poisoned, recovering");
                poisoned.into_inner()
            }
        };
        let now = now_ms();
        let tx = match conn.transaction() {
            Ok(tx) => tx,
            Err(err) => {
                eprintln!("[usage_store] insert_events: begin transaction failed: {err}");
                return 0;
            }
        };
        let mut inserted = 0usize;
        {
            let mut stmt = match tx.prepare_cached(
                "INSERT OR IGNORE INTO usage_events (\
                    event_hash, request_id, timestamp_ms, timestamp, provider, model, \
                    requested_model, resolved_model, endpoint, method, path, auth_type, \
                    auth_index, source, api_key_hash, input_tokens, output_tokens, \
                    reasoning_tokens, cached_tokens, cache_creation_tokens, cache_read_tokens, \
                    total_tokens, latency_ms, failed, status_code, reasoning_effort, raw_json, \
                    created_at_ms) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,\
                    ?21,?22,?23,?24,?25,?26,?27,?28)",
            ) {
                Ok(stmt) => stmt,
                Err(_) => return 0,
            };
            for event in events {
                let result = stmt.execute(params![
                    event.event_hash,
                    event.request_id,
                    event.timestamp_ms,
                    event.timestamp,
                    event.provider,
                    event.model,
                    event.requested_model,
                    event.resolved_model,
                    event.endpoint,
                    event.method,
                    event.path,
                    event.auth_type,
                    event.auth_index,
                    event.source,
                    event.api_key_hash,
                    event.input_tokens as i64,
                    event.output_tokens as i64,
                    event.reasoning_tokens as i64,
                    event.cached_tokens as i64,
                    event.cache_creation_tokens as i64,
                    event.cache_read_tokens as i64,
                    event.total_tokens as i64,
                    event.latency_ms as i64,
                    event.failed as i64,
                    event.status_code.map(|code| code as i64),
                    event.reasoning_effort,
                    event.raw_json,
                    now,
                ]);
                if let Ok(changed) = result {
                    inserted += changed;
                }
            }
        }
        let _ = tx.commit();
        inserted
    }

    /// Most recent events as `RequestLogEntry` rows, for the Logs screen and the
    /// `AppState.logs` backfill (newest first).
    pub fn recent_events(&self, limit: usize) -> Vec<RequestLogEntry> {
        let conn = match self.conn.lock() {
            Ok(conn) => conn,
            Err(poisoned) => {
                eprintln!("[usage_store] recent_events: mutex poisoned, recovering");
                poisoned.into_inner()
            }
        };
        let mut stmt = match conn.prepare(
            "SELECT request_id, timestamp, method, endpoint, provider, model, resolved_model, \
                input_tokens, output_tokens, latency_ms, status_code, failed, source, \
                reasoning_effort \
             FROM usage_events ORDER BY timestamp_ms DESC LIMIT ?1",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map([limit as i64], |row| {
            let failed: i64 = row.get(11)?;
            Ok(RequestLogEntry {
                id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                timestamp: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                method: row
                    .get::<_, Option<String>>(2)?
                    .unwrap_or_else(|| "POST".to_string()),
                endpoint: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                provider: row.get(4)?,
                model: row.get(5)?,
                resolved_model: row.get(6)?,
                resolved_provider: None,
                input_tokens: row.get::<_, Option<i64>>(7)?.map(|value| value as u64),
                output_tokens: row.get::<_, Option<i64>>(8)?.map(|value| value as u64),
                duration_ms: row.get::<_, Option<i64>>(9)?.unwrap_or(0) as u64,
                status_code: row.get::<_, Option<i64>>(10)?.map(|value| value as u16),
                request_size: 0,
                response_size: 0,
                error_message: if failed != 0 {
                    Some("请求失败".to_string())
                } else {
                    None
                },
                fallback_attempts: None,
                fallback_started_from_cache: false,
                reasoning_effort: row.get(13)?,
                account: row.get(12)?,
            })
        });
        match rows {
            Ok(iter) => iter.flatten().collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Aggregate KPI totals over the filtered range.
    pub fn query_stats(&self, query: &UsageQuery) -> UsageAggregate {
        let conn = match self.conn.lock() {
            Ok(conn) => conn,
            Err(poisoned) => {
                eprintln!("[usage_store] query_stats: mutex poisoned, recovering");
                poisoned.into_inner()
            }
        };
        let (where_sql, query_params) = build_where(query);
        let sql = format!(
            "SELECT COUNT(*), \
                COALESCE(SUM(CASE WHEN e.failed=0 THEN 1 ELSE 0 END),0), \
                COALESCE(SUM(e.failed),0), \
                COALESCE(SUM(e.input_tokens),0), \
                COALESCE(SUM(e.output_tokens),0), \
                COALESCE(SUM(e.reasoning_tokens),0), \
                COALESCE(SUM(e.cached_tokens),0), \
                COALESCE(SUM(e.cache_creation_tokens),0), \
                COALESCE(SUM(e.cache_read_tokens),0), \
                COALESCE(SUM(e.total_tokens),0), \
                COUNT(DISTINCT e.source), \
                COALESCE(AVG(e.latency_ms),0) \
             FROM usage_events e{where_sql}"
        );
        let totals = conn
            .query_row(&sql, params_from_iter(query_params.iter()), |row| {
                Ok(Totals {
                    total: row.get::<_, i64>(0)? as u64,
                    success: row.get::<_, i64>(1)? as u64,
                    failed: row.get::<_, i64>(2)? as u64,
                    input: row.get::<_, i64>(3)? as u64,
                    output: row.get::<_, i64>(4)? as u64,
                    reasoning: row.get::<_, i64>(5)? as u64,
                    cached: row.get::<_, i64>(6)? as u64,
                    cache_creation: row.get::<_, i64>(7)? as u64,
                    cache_read: row.get::<_, i64>(8)? as u64,
                    total_tokens: row.get::<_, i64>(9)? as u64,
                    accounts: row.get::<_, i64>(10)? as u64,
                    avg_latency: row.get::<_, f64>(11)?,
                })
            })
            .unwrap_or_default();

        let prices_configured = has_prices(&conn);
        let estimated_cost = if prices_configured {
            Some(cost_for(&conn, &where_sql, &query_params))
        } else {
            None
        };

        UsageAggregate {
            total_requests: totals.total,
            success_requests: totals.success,
            failed_requests: totals.failed,
            success_rate: pct(totals.success, totals.total),
            account_count: totals.accounts,
            total_tokens: totals.total_tokens,
            input_tokens: totals.input,
            output_tokens: totals.output,
            reasoning_tokens: totals.reasoning,
            cached_tokens: totals.cached,
            cache_creation_tokens: totals.cache_creation,
            cache_read_tokens: totals.cache_read,
            input_token_ratio: pct(totals.input, totals.total_tokens),
            output_token_ratio: pct(totals.output, totals.total_tokens),
            cache_hit_rate: pct(totals.cached, totals.input),
            avg_latency_ms: totals.avg_latency,
            estimated_cost,
            prices_configured,
        }
    }

    /// Per-account rollup for the summary table (grouped by account + provider).
    pub fn account_summary(&self, query: &UsageQuery) -> Vec<AccountSummaryRow> {
        let conn = match self.conn.lock() {
            Ok(conn) => conn,
            Err(poisoned) => {
                eprintln!("[usage_store] account_summary: mutex poisoned, recovering");
                poisoned.into_inner()
            }
        };
        let (where_sql, query_params) = build_where(query);
        let prices_configured = has_prices(&conn);
        // last_request 用裸列 e.timestamp(不再 MAX(e.timestamp)):SQLite 在一个查询里
        // 只有一个聚合 MAX/MIN 时,裸列取自该 MAX 命中的同一行,保证显示时间串与
        // last_request_ms 来自同一条记录(否则字符串 MAX 与数值 MAX 可能取自不同行)。
        let sql = format!(
            "SELECT e.source, e.provider, COUNT(*), \
                COALESCE(SUM(CASE WHEN e.failed=0 THEN 1 ELSE 0 END),0), \
                COALESCE(SUM(e.failed),0), \
                COALESCE(SUM(e.input_tokens),0), \
                COALESCE(SUM(e.output_tokens),0), \
                COALESCE(SUM(e.total_tokens),0), \
                COALESCE(SUM(MAX(e.input_tokens - e.cached_tokens, 0)*COALESCE(p.prompt_per_1m,0)/1000000.0 \
                    + e.output_tokens*COALESCE(p.completion_per_1m,0)/1000000.0 \
                    + e.cached_tokens*COALESCE(p.cache_per_1m,0)/1000000.0),0), \
                MAX(e.timestamp_ms), e.timestamp \
             FROM usage_events e LEFT JOIN model_prices p ON p.model = e.model{where_sql} \
             GROUP BY e.source, e.provider ORDER BY COUNT(*) DESC"
        );
        let mut stmt = match conn.prepare(&sql) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params_from_iter(query_params.iter()), |row| {
            let total = row.get::<_, i64>(2)? as u64;
            let success = row.get::<_, i64>(3)? as u64;
            let failed = row.get::<_, i64>(4)? as u64;
            let cost = row.get::<_, f64>(8)?;
            Ok(AccountSummaryRow {
                account: row
                    .get::<_, Option<String>>(0)?
                    .unwrap_or_else(|| "未知账号".to_string()),
                provider: row.get(1)?,
                total_requests: total,
                success_requests: success,
                failed_requests: failed,
                success_rate: pct(success, total),
                input_tokens: row.get::<_, i64>(5)? as u64,
                output_tokens: row.get::<_, i64>(6)? as u64,
                total_tokens: row.get::<_, i64>(7)? as u64,
                estimated_cost: if prices_configured { Some(cost) } else { None },
                last_request_ms: row.get::<_, i64>(9)?,
                last_request: row.get::<_, Option<String>>(10)?.unwrap_or_default(),
            })
        });
        match rows {
            Ok(iter) => iter.flatten().collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Time-bucketed totals for the dashboard trend and token-composition charts.
    pub fn usage_timeseries(
        &self,
        query: &UsageQuery,
        bucket: UsageChartBucket,
    ) -> Vec<UsageTimeSeriesPoint> {
        let conn = match self.conn.lock() {
            Ok(conn) => conn,
            Err(poisoned) => {
                eprintln!("[usage_store] usage_timeseries: mutex poisoned, recovering");
                poisoned.into_inner()
            }
        };
        let (where_sql, query_params) = build_where(query);
        let prices_configured = has_prices(&conn);
        let bucket_label = bucket_expr(bucket);
        let sql = format!(
            "SELECT {bucket_label}, MIN(e.timestamp_ms), COUNT(*), \
                COALESCE(SUM(CASE WHEN e.failed=0 THEN 1 ELSE 0 END),0), \
                COALESCE(SUM(e.failed),0), \
                COALESCE(SUM(e.input_tokens),0), \
                COALESCE(SUM(e.output_tokens),0), \
                COALESCE(SUM(e.cached_tokens),0), \
                COALESCE(SUM(MAX(e.input_tokens - e.cached_tokens, 0)),0), \
                COALESCE(SUM(e.total_tokens),0), \
                COALESCE(SUM(MAX(e.input_tokens - e.cached_tokens, 0)*COALESCE(p.prompt_per_1m,0)/1000000.0 \
                    + e.output_tokens*COALESCE(p.completion_per_1m,0)/1000000.0 \
                    + e.cached_tokens*COALESCE(p.cache_per_1m,0)/1000000.0),0) \
             FROM usage_events e LEFT JOIN model_prices p ON p.model = e.model{where_sql} \
             GROUP BY 1 ORDER BY 2 ASC"
        );
        let mut stmt = match conn.prepare(&sql) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params_from_iter(query_params.iter()), |row| {
            let cost = row.get::<_, f64>(10)?;
            Ok(UsageTimeSeriesPoint {
                bucket: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                bucket_start_ms: row.get::<_, i64>(1)?,
                total_requests: row.get::<_, i64>(2)? as u64,
                success_requests: row.get::<_, i64>(3)? as u64,
                failed_requests: row.get::<_, i64>(4)? as u64,
                input_tokens: row.get::<_, i64>(5)? as u64,
                output_tokens: row.get::<_, i64>(6)? as u64,
                cached_tokens: row.get::<_, i64>(7)? as u64,
                uncached_input_tokens: row.get::<_, i64>(8)? as u64,
                total_tokens: row.get::<_, i64>(9)? as u64,
                estimated_cost: if prices_configured { Some(cost) } else { None },
            })
        });
        match rows {
            Ok(iter) => iter.flatten().collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Per-model rollup for the dashboard model-cost ranking chart.
    pub fn model_breakdown(&self, query: &UsageQuery, limit: usize) -> Vec<UsageModelBreakdownRow> {
        let conn = match self.conn.lock() {
            Ok(conn) => conn,
            Err(poisoned) => {
                eprintln!("[usage_store] model_breakdown: mutex poisoned, recovering");
                poisoned.into_inner()
            }
        };
        let (where_sql, query_params) = build_where(query);
        let prices_configured = has_prices(&conn);
        let sql = format!(
            "SELECT e.model, COUNT(*), \
                COALESCE(SUM(e.input_tokens),0), \
                COALESCE(SUM(e.output_tokens),0), \
                COALESCE(SUM(e.cached_tokens),0), \
                COALESCE(SUM(MAX(e.input_tokens - e.cached_tokens, 0)),0), \
                COALESCE(SUM(e.total_tokens),0), \
                COALESCE(SUM(MAX(e.input_tokens - e.cached_tokens, 0)*COALESCE(p.prompt_per_1m,0)/1000000.0 \
                    + e.output_tokens*COALESCE(p.completion_per_1m,0)/1000000.0 \
                    + e.cached_tokens*COALESCE(p.cache_per_1m,0)/1000000.0),0) \
             FROM usage_events e LEFT JOIN model_prices p ON p.model = e.model{where_sql} \
             GROUP BY e.model ORDER BY 8 DESC, COUNT(*) DESC LIMIT ?"
        );
        let mut params = query_params;
        params.push(Value::Integer(limit as i64));
        let mut stmt = match conn.prepare(&sql) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params_from_iter(params.iter()), |row| {
            let input = row.get::<_, i64>(2)? as u64;
            let cached = row.get::<_, i64>(4)? as u64;
            let cost = row.get::<_, f64>(7)?;
            Ok(UsageModelBreakdownRow {
                model: row
                    .get::<_, Option<String>>(0)?
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "unknown".to_string()),
                total_requests: row.get::<_, i64>(1)? as u64,
                input_tokens: input,
                output_tokens: row.get::<_, i64>(3)? as u64,
                cached_tokens: cached,
                uncached_input_tokens: row.get::<_, i64>(5)? as u64,
                total_tokens: row.get::<_, i64>(6)? as u64,
                cache_hit_rate: pct(cached, input),
                estimated_cost: if prices_configured { Some(cost) } else { None },
            })
        });
        match rows {
            Ok(iter) => iter.flatten().collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Distinct values for the dashboard filter dropdowns.
    pub fn filter_options(&self) -> UsageFilterOptions {
        let conn = match self.conn.lock() {
            Ok(conn) => conn,
            Err(poisoned) => {
                eprintln!("[usage_store] filter_options: mutex poisoned, recovering");
                poisoned.into_inner()
            }
        };
        UsageFilterOptions {
            accounts: distinct(&conn, "source"),
            providers: distinct(&conn, "provider"),
            models: distinct(&conn, "model"),
            channels: distinct(&conn, "auth_type"),
            api_keys: distinct(&conn, "api_key_hash")
                .into_iter()
                .map(|hash| ApiKeyOption { hash, alias: None })
                .collect(),
        }
    }

    /// Per-account health over each account's most recent `window` requests,
    /// classified by REAL status code so a genuine auth failure (401/403) is
    /// told apart from rate-limiting (429) and transient/server errors. This is
    /// how the reference (cpa-manager) judges a "real 401": by the actual HTTP
    /// status, never by a blanket failure count.
    pub fn account_auth_health(&self, window: u32) -> Vec<AccountAuthHealth> {
        let conn = match self.conn.lock() {
            Ok(conn) => conn,
            Err(poisoned) => {
                eprintln!("[usage_store] account_auth_health: mutex poisoned, recovering");
                poisoned.into_inner()
            }
        };
        let sql = "WITH ranked AS (\
                SELECT source, failed, status_code, \
                    ROW_NUMBER() OVER (PARTITION BY source ORDER BY timestamp_ms DESC, id DESC) AS rn \
                FROM usage_events WHERE source IS NOT NULL AND source <> '' \
            ) \
            SELECT source, COUNT(*), \
                COALESCE(SUM(CASE WHEN failed=1 AND status_code IN (401,403) THEN 1 ELSE 0 END),0), \
                COALESCE(SUM(CASE WHEN failed=1 AND status_code=429 THEN 1 ELSE 0 END),0), \
                COALESCE(SUM(CASE WHEN failed=1 AND (status_code IS NULL OR status_code NOT IN (401,403,429)) THEN 1 ELSE 0 END),0), \
                COALESCE(SUM(CASE WHEN failed=0 THEN 1 ELSE 0 END),0), \
                MAX(CASE WHEN rn=1 THEN status_code END) \
            FROM ranked WHERE rn <= ?1 GROUP BY source";
        let mut stmt = match conn.prepare(sql) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map([window as i64], |row| {
            let auth_failures = row.get::<_, i64>(2)? as u64;
            let rate_limited = row.get::<_, i64>(3)? as u64;
            let server_errors = row.get::<_, i64>(4)? as u64;
            let successes = row.get::<_, i64>(5)? as u64;
            // Suggest re-auth ONLY when the recent window has no successes and is
            // dominated by genuine 401/403 — never on 429/5xx-heavy accounts.
            let recommend_reauth = successes == 0
                && auth_failures >= 2
                && auth_failures >= rate_limited + server_errors;
            Ok(AccountAuthHealth {
                account: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                recent_total: row.get::<_, i64>(1)? as u64,
                auth_failures,
                rate_limited,
                server_errors,
                successes,
                last_status_code: row.get::<_, Option<i64>>(6)?.map(|code| code as u16),
                recommend_reauth,
            })
        });
        match rows {
            Ok(iter) => iter.flatten().collect(),
            Err(_) => Vec::new(),
        }
    }

    /// All configured model prices.
    pub fn model_prices(&self) -> Vec<ModelPrice> {
        let conn = match self.conn.lock() {
            Ok(conn) => conn,
            Err(poisoned) => {
                eprintln!("[usage_store] model_prices: mutex poisoned, recovering");
                poisoned.into_inner()
            }
        };
        let mut stmt = match conn.prepare(
            "SELECT model, prompt_per_1m, completion_per_1m, cache_per_1m, source \
             FROM model_prices ORDER BY model",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map([], |row| {
            Ok(ModelPrice {
                model: row.get(0)?,
                prompt_per_1m: row.get(1)?,
                completion_per_1m: row.get(2)?,
                cache_per_1m: row.get(3)?,
                source: row.get(4)?,
            })
        });
        match rows {
            Ok(iter) => iter.flatten().collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Replace the full set of model prices (empty `prices` clears the table).
    pub fn set_model_prices(&self, prices: &[ModelPrice]) {
        let mut conn = match self.conn.lock() {
            Ok(conn) => conn,
            Err(poisoned) => {
                eprintln!("[usage_store] set_model_prices: mutex poisoned, recovering");
                poisoned.into_inner()
            }
        };
        let now = now_ms();
        let tx = match conn.transaction() {
            Ok(tx) => tx,
            Err(_) => return,
        };
        let _ = tx.execute("DELETE FROM model_prices", []);
        {
            let mut stmt = match tx.prepare(
                "INSERT OR REPLACE INTO model_prices \
                    (model, prompt_per_1m, completion_per_1m, cache_per_1m, source, updated_at_ms) \
                 VALUES (?1,?2,?3,?4,?5,?6)",
            ) {
                Ok(stmt) => stmt,
                Err(_) => return,
            };
            for price in prices {
                let model = price.model.trim();
                if model.is_empty() {
                    continue;
                }
                let _ = stmt.execute(params![
                    model,
                    price.prompt_per_1m,
                    price.completion_per_1m,
                    price.cache_per_1m,
                    price.source,
                    now,
                ]);
            }
        }
        let _ = tx.commit();
    }
}

struct Totals {
    total: u64,
    success: u64,
    failed: u64,
    input: u64,
    output: u64,
    reasoning: u64,
    cached: u64,
    cache_creation: u64,
    cache_read: u64,
    total_tokens: u64,
    accounts: u64,
    avg_latency: f64,
}

impl Default for Totals {
    fn default() -> Self {
        Totals {
            total: 0,
            success: 0,
            failed: 0,
            input: 0,
            output: 0,
            reasoning: 0,
            cached: 0,
            cache_creation: 0,
            cache_read: 0,
            total_tokens: 0,
            accounts: 0,
            avg_latency: 0.0,
        }
    }
}

fn configure(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA busy_timeout=5000;
         CREATE TABLE IF NOT EXISTS usage_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            event_hash TEXT NOT NULL UNIQUE,
            request_id TEXT,
            timestamp_ms INTEGER NOT NULL,
            timestamp TEXT NOT NULL,
            provider TEXT,
            model TEXT NOT NULL,
            requested_model TEXT,
            resolved_model TEXT,
            endpoint TEXT,
            method TEXT,
            path TEXT,
            auth_type TEXT,
            auth_index TEXT,
            source TEXT,
            api_key_hash TEXT,
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            reasoning_tokens INTEGER NOT NULL DEFAULT 0,
            cached_tokens INTEGER NOT NULL DEFAULT 0,
            cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
            cache_read_tokens INTEGER NOT NULL DEFAULT 0,
            total_tokens INTEGER NOT NULL DEFAULT 0,
            latency_ms INTEGER NOT NULL DEFAULT 0,
            failed INTEGER NOT NULL DEFAULT 0,
            status_code INTEGER,
            reasoning_effort TEXT,
            raw_json TEXT,
            created_at_ms INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_usage_events_timestamp ON usage_events(timestamp_ms);
         CREATE INDEX IF NOT EXISTS idx_usage_events_model ON usage_events(model);
         CREATE INDEX IF NOT EXISTS idx_usage_events_provider ON usage_events(provider);
         CREATE INDEX IF NOT EXISTS idx_usage_events_source ON usage_events(source);
         CREATE TABLE IF NOT EXISTS model_prices (
            model TEXT PRIMARY KEY,
            prompt_per_1m REAL NOT NULL,
            completion_per_1m REAL NOT NULL,
            cache_per_1m REAL NOT NULL DEFAULT 0,
            source TEXT,
            updated_at_ms INTEGER NOT NULL
         );",
    )?;
    migrate(conn)
}

/// 当前 schema 版本。将来给 usage_events 加列时:把本常量 +1,并在 migrate() 里按
/// `current < N` 追加对应的 `ALTER TABLE ... ADD COLUMN ...`。没有迁移框架时,直接改
/// CREATE TABLE 只对新库生效,老库不会加列、随后带新列的 INSERT 会全部失败。
const SCHEMA_VERSION: i64 = 1;

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    // 示例(将来用):
    //   if current < 2 {
    //       conn.execute_batch("ALTER TABLE usage_events ADD COLUMN xxx INTEGER NOT NULL DEFAULT 0;")?;
    //   }
    if current < SCHEMA_VERSION {
        conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))?;
    }
    Ok(())
}

/// Build the `WHERE` clause + positional params for a usage query. All columns
/// are aliased `e.` so the same clause works for the plain stats query and the
/// `model_prices`-joined cost/summary queries.
fn build_where(query: &UsageQuery) -> (String, Vec<Value>) {
    let mut clauses: Vec<String> = Vec::new();
    let mut query_params: Vec<Value> = Vec::new();

    if let Some(start) = query.start_ms {
        clauses.push("e.timestamp_ms >= ?".to_string());
        query_params.push(Value::Integer(start));
    }
    if let Some(end) = query.end_ms {
        clauses.push("e.timestamp_ms <= ?".to_string());
        query_params.push(Value::Integer(end));
    }
    if let Some(provider) = nonempty(&query.provider) {
        clauses.push("e.provider = ?".to_string());
        query_params.push(Value::Text(provider));
    }
    if let Some(model) = nonempty(&query.model) {
        clauses.push("e.model = ?".to_string());
        query_params.push(Value::Text(model));
    }
    if let Some(account) = nonempty(&query.account) {
        clauses.push("e.source = ?".to_string());
        query_params.push(Value::Text(account));
    }
    if let Some(api_key_hash) = nonempty(&query.api_key_hash) {
        clauses.push("e.api_key_hash = ?".to_string());
        query_params.push(Value::Text(api_key_hash));
    }
    if let Some(channel) = nonempty(&query.channel) {
        clauses.push("e.auth_type = ?".to_string());
        query_params.push(Value::Text(channel));
    }
    match query.status {
        Some(UsageStatusFilter::Success) => clauses.push("e.failed = 0".to_string()),
        Some(UsageStatusFilter::Failed) => clauses.push("e.failed = 1".to_string()),
        _ => {}
    }
    if let Some(term) = nonempty(&query.search) {
        let like = format!("%{}%", term);
        let columns = [
            "e.source",
            "e.model",
            "e.provider",
            "e.auth_index",
            "e.auth_type",
            "e.api_key_hash",
            "e.endpoint",
            "e.path",
            "e.request_id",
        ];
        let ors: Vec<String> = columns
            .iter()
            .map(|column| format!("{} LIKE ?", column))
            .collect();
        clauses.push(format!("({})", ors.join(" OR ")));
        for _ in columns {
            query_params.push(Value::Text(like.clone()));
        }
    }

    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    (where_sql, query_params)
}

fn cost_for(conn: &Connection, where_sql: &str, query_params: &[Value]) -> f64 {
    let sql = format!(
        "SELECT COALESCE(SUM(MAX(e.input_tokens - e.cached_tokens, 0)*p.prompt_per_1m/1000000.0 \
            + e.output_tokens*p.completion_per_1m/1000000.0 \
            + e.cached_tokens*p.cache_per_1m/1000000.0),0) \
         FROM usage_events e JOIN model_prices p ON p.model = e.model{where_sql}"
    );
    conn.query_row(&sql, params_from_iter(query_params.iter()), |row| {
        row.get::<_, f64>(0)
    })
    .unwrap_or(0.0)
}

fn bucket_expr(bucket: UsageChartBucket) -> &'static str {
    match bucket {
        UsageChartBucket::TwentyMinute => {
            "strftime('%Y-%m-%d %H:', e.timestamp_ms / 1000, 'unixepoch', 'localtime') || printf('%02d', (CAST(strftime('%M', e.timestamp_ms / 1000, 'unixepoch', 'localtime') AS INTEGER) / 20) * 20)"
        }
        UsageChartBucket::Hour => {
            "strftime('%Y-%m-%d %H:00', e.timestamp_ms / 1000, 'unixepoch', 'localtime')"
        }
        UsageChartBucket::Day => "strftime('%Y-%m-%d', e.timestamp_ms / 1000, 'unixepoch', 'localtime')",
    }
}

fn has_prices(conn: &Connection) -> bool {
    conn.query_row("SELECT COUNT(*) FROM model_prices", [], |row| {
        row.get::<_, i64>(0)
    })
    .unwrap_or(0)
        > 0
}

/// Distinct non-empty values of an internal column (never user input — safe to
/// interpolate the column name).
fn distinct(conn: &Connection, column: &str) -> Vec<String> {
    let sql = format!(
        "SELECT DISTINCT {0} FROM usage_events \
         WHERE {0} IS NOT NULL AND {0} <> '' ORDER BY {0}",
        column
    );
    let mut stmt = match conn.prepare(&sql) {
        Ok(stmt) => stmt,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map([], |row| row.get::<_, String>(0));
    match rows {
        Ok(iter) => iter.flatten().collect(),
        Err(_) => Vec::new(),
    }
}

fn nonempty(value: &Option<String>) -> Option<String> {
    value
        .as_ref()
        .map(|inner| inner.trim().to_string())
        .filter(|inner| !inner.is_empty())
}

/// Percentage `part / whole * 100`, 0 when `whole == 0`.
fn pct(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64 * 100.0
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(
        hash: &str,
        ts: i64,
        model: &str,
        source: &str,
        failed: bool,
        input: u64,
    ) -> UsageEvent {
        UsageEvent {
            event_hash: hash.to_string(),
            request_id: Some(hash.to_string()),
            timestamp_ms: ts,
            timestamp: "2026-06-05T10:00:00Z".to_string(),
            provider: Some("codex".to_string()),
            model: model.to_string(),
            requested_model: Some(model.to_string()),
            resolved_model: Some(model.to_string()),
            endpoint: Some("POST /v1/responses".to_string()),
            method: Some("POST".to_string()),
            path: Some("/v1/responses".to_string()),
            auth_type: Some("oauth".to_string()),
            auth_index: Some("idx".to_string()),
            source: Some(source.to_string()),
            api_key_hash: Some("keyhash".to_string()),
            input_tokens: input,
            output_tokens: 10,
            reasoning_tokens: 2,
            cached_tokens: 5,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            total_tokens: input + 10,
            latency_ms: 100,
            failed,
            status_code: Some(if failed { 500 } else { 200 }),
            reasoning_effort: None,
            raw_json: None,
        }
    }

    #[test]
    fn dedup_and_aggregate() {
        let store = UsageStore::open_in_memory();
        let events = vec![
            event("a", 1000, "gpt-5.5", "alice@example.com", false, 100),
            event("b", 2000, "gpt-5.5", "alice@example.com", true, 50),
            event("c", 3000, "claude", "bob@example.com", false, 200),
        ];
        assert_eq!(store.insert_events(&events), 3);
        // Re-inserting the same hashes is ignored.
        assert_eq!(store.insert_events(&events), 0);

        let stats = store.query_stats(&UsageQuery::default());
        assert_eq!(stats.total_requests, 3);
        assert_eq!(stats.success_requests, 2);
        assert_eq!(stats.failed_requests, 1);
        assert_eq!(stats.account_count, 2);
        assert_eq!(stats.input_tokens, 350);
        assert!((stats.success_rate - 66.6667).abs() < 0.01);
        assert!(stats.estimated_cost.is_none());
        assert!(!stats.prices_configured);

        // Range + status filters.
        let mut query = UsageQuery::default();
        query.start_ms = Some(1500);
        query.status = Some(UsageStatusFilter::Failed);
        let failed = store.query_stats(&query);
        assert_eq!(failed.total_requests, 1);

        let summary = store.account_summary(&UsageQuery::default());
        assert_eq!(summary.len(), 2);

        let options = store.filter_options();
        assert_eq!(options.providers, vec!["codex".to_string()]);
        assert_eq!(
            options.models,
            vec!["claude".to_string(), "gpt-5.5".to_string()]
        );
    }

    #[test]
    fn cost_estimation_uses_prices() {
        let store = UsageStore::open_in_memory();
        store.insert_events(&[event(
            "a",
            1000,
            "gpt-5.5",
            "alice@example.com",
            false,
            1_000_000,
        )]);
        store.set_model_prices(&[ModelPrice {
            model: "gpt-5.5".to_string(),
            prompt_per_1m: 2.0,
            completion_per_1m: 8.0,
            cache_per_1m: 0.5,
            source: Some("manual".to_string()),
        }]);
        let stats = store.query_stats(&UsageQuery::default());
        assert!(stats.prices_configured);
        // Cached input is priced at the cache rate instead of the full prompt rate.
        let cost = stats.estimated_cost.unwrap();
        assert!((cost - 2.00008).abs() < 0.001, "cost was {cost}");
    }

    #[test]
    fn cached_tokens_replace_prompt_cost_in_estimates() {
        let store = UsageStore::open_in_memory();
        let mut cached = event(
            "cached",
            1000,
            "gpt-5.5",
            "alice@example.com",
            false,
            1_000_000,
        );
        cached.cached_tokens = 800_000;
        cached.output_tokens = 100_000;
        cached.total_tokens = 1_100_000;
        store.insert_events(&[cached]);
        store.set_model_prices(&[ModelPrice {
            model: "gpt-5.5".to_string(),
            prompt_per_1m: 10.0,
            completion_per_1m: 20.0,
            cache_per_1m: 1.0,
            source: Some("manual".to_string()),
        }]);

        let stats = store.query_stats(&UsageQuery::default());
        let summary = store.account_summary(&UsageQuery::default());

        // Only uncached input should use the full prompt price:
        // 200k prompt + 800k cache + 100k completion = 2 + 0.8 + 2 = 4.8.
        let cost = stats.estimated_cost.unwrap();
        assert!((cost - 4.8).abs() < 0.001, "cost was {cost}");
        assert_eq!(summary.len(), 1);
        let account_cost = summary[0].estimated_cost.unwrap();
        assert!(
            (account_cost - 4.8).abs() < 0.001,
            "account cost was {account_cost}"
        );
    }

    #[test]
    fn usage_timeseries_groups_by_day_with_correct_cost_formula() {
        let store = UsageStore::open_in_memory();
        let mut first = event(
            "ts-a",
            1_764_636_000_000,
            "gpt-5.5",
            "alice@example.com",
            false,
            1_000_000,
        );
        first.cached_tokens = 800_000;
        first.output_tokens = 100_000;
        first.total_tokens = 1_100_000;
        let mut second = event(
            "ts-b",
            1_764_722_400_000,
            "gpt-5.5",
            "alice@example.com",
            true,
            500_000,
        );
        second.cached_tokens = 100_000;
        second.output_tokens = 50_000;
        second.total_tokens = 550_000;
        store.insert_events(&[first, second]);
        store.set_model_prices(&[ModelPrice {
            model: "gpt-5.5".to_string(),
            prompt_per_1m: 10.0,
            completion_per_1m: 20.0,
            cache_per_1m: 1.0,
            source: Some("manual".to_string()),
        }]);

        let points = store.usage_timeseries(&UsageQuery::default(), UsageChartBucket::Day);

        assert_eq!(points.len(), 2);
        assert_eq!(points[0].total_requests, 1);
        assert_eq!(points[0].success_requests, 1);
        assert_eq!(points[0].failed_requests, 0);
        assert_eq!(points[0].uncached_input_tokens, 200_000);
        assert_eq!(points[0].cached_tokens, 800_000);
        assert_eq!(points[0].output_tokens, 100_000);
        assert!((points[0].estimated_cost.unwrap() - 4.8).abs() < 0.001);
        assert_eq!(points[1].total_requests, 1);
        assert_eq!(points[1].success_requests, 0);
        assert_eq!(points[1].failed_requests, 1);
        assert_eq!(points[1].uncached_input_tokens, 400_000);
        assert!((points[1].estimated_cost.unwrap() - 5.1).abs() < 0.001);
    }

    #[test]
    fn usage_timeseries_groups_by_twenty_minute_windows() {
        let store = UsageStore::open_in_memory();
        let base = 1_764_633_600_000;
        store.insert_events(&[
            event(
                "ts-20-a",
                base + 60_000,
                "gpt-5.5",
                "alice@example.com",
                false,
                100,
            ),
            event(
                "ts-20-b",
                base + 19 * 60_000,
                "gpt-5.5",
                "alice@example.com",
                false,
                200,
            ),
            event(
                "ts-20-c",
                base + 20 * 60_000,
                "gpt-5.5",
                "alice@example.com",
                true,
                300,
            ),
            event(
                "ts-20-d",
                base + 41 * 60_000,
                "gpt-5.5",
                "alice@example.com",
                false,
                400,
            ),
        ]);

        let points = store.usage_timeseries(&UsageQuery::default(), UsageChartBucket::TwentyMinute);

        assert_eq!(points.len(), 3);
        assert_eq!(points[0].total_requests, 2);
        assert_eq!(points[0].success_requests, 2);
        assert_eq!(points[0].failed_requests, 0);
        assert_eq!(points[0].input_tokens, 300);
        assert_eq!(points[1].total_requests, 1);
        assert_eq!(points[1].success_requests, 0);
        assert_eq!(points[1].failed_requests, 1);
        assert_eq!(points[2].total_requests, 1);
        assert_eq!(points[2].input_tokens, 400);
    }

    #[test]
    fn usage_timeseries_keeps_bucket_start_on_real_event_time() {
        let store = UsageStore::open_in_memory();
        let first_ms = 1_764_636_123_456;
        let second_ms = first_ms + 3_600_000;
        store.insert_events(&[
            event(
                "ts-real-a",
                first_ms,
                "gpt-5.5",
                "alice@example.com",
                false,
                100,
            ),
            event(
                "ts-real-b",
                second_ms,
                "gpt-5.5",
                "alice@example.com",
                false,
                100,
            ),
        ]);

        let points = store.usage_timeseries(&UsageQuery::default(), UsageChartBucket::Day);

        assert_eq!(points.len(), 1);
        assert_eq!(points[0].bucket_start_ms, first_ms);
    }

    #[test]
    fn usage_model_breakdown_ranks_by_corrected_cost() {
        let store = UsageStore::open_in_memory();
        let mut expensive = event(
            "mb-a",
            1000,
            "gpt-5.5",
            "alice@example.com",
            false,
            1_000_000,
        );
        expensive.cached_tokens = 800_000;
        expensive.output_tokens = 100_000;
        expensive.total_tokens = 1_100_000;
        let mut cheap = event(
            "mb-b",
            2000,
            "gpt-5.4-mini",
            "alice@example.com",
            false,
            1_000_000,
        );
        cheap.cached_tokens = 900_000;
        cheap.output_tokens = 10_000;
        cheap.total_tokens = 1_010_000;
        store.insert_events(&[expensive, cheap]);
        store.set_model_prices(&[
            ModelPrice {
                model: "gpt-5.5".to_string(),
                prompt_per_1m: 10.0,
                completion_per_1m: 20.0,
                cache_per_1m: 1.0,
                source: Some("manual".to_string()),
            },
            ModelPrice {
                model: "gpt-5.4-mini".to_string(),
                prompt_per_1m: 1.0,
                completion_per_1m: 2.0,
                cache_per_1m: 0.1,
                source: Some("manual".to_string()),
            },
        ]);

        let rows = store.model_breakdown(&UsageQuery::default(), 10);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].model, "gpt-5.5");
        assert!((rows[0].estimated_cost.unwrap() - 4.8).abs() < 0.001);
        assert_eq!(rows[0].uncached_input_tokens, 200_000);
        assert!((rows[0].cache_hit_rate - 80.0).abs() < 0.001);
        assert_eq!(rows[1].model, "gpt-5.4-mini");
        assert!((rows[1].estimated_cost.unwrap() - 0.21).abs() < 0.001);
    }

    fn coded(hash: &str, ts: i64, source: &str, status: u16) -> UsageEvent {
        let mut e = event(hash, ts, "gpt-5.5", source, status >= 400, 10);
        e.status_code = Some(status);
        e
    }

    #[test]
    fn auth_health_classifies_by_real_status_code() {
        let store = UsageStore::open_in_memory();
        store.insert_events(&[
            // alice: only genuine 401s, no success -> recommend re-auth.
            coded("a1", 1000, "alice@example.com", 401),
            coded("a2", 2000, "alice@example.com", 401),
            coded("a3", 3000, "alice@example.com", 403),
            // bob: 500/429 heavy, no 401 -> NOT auth, no re-auth.
            coded("b1", 1000, "bob@example.com", 500),
            coded("b2", 2000, "bob@example.com", 500),
            coded("b3", 3000, "bob@example.com", 429),
            // carol: a couple 401s but a recent success -> NOT durable, no re-auth.
            coded("c1", 1000, "carol@example.com", 401),
            coded("c2", 2000, "carol@example.com", 401),
            coded("c3", 3000, "carol@example.com", 200),
        ]);
        let health: std::collections::HashMap<_, _> = store
            .account_auth_health(20)
            .into_iter()
            .map(|h| (h.account.clone(), h))
            .collect();

        let alice = &health["alice@example.com"];
        assert_eq!(alice.auth_failures, 3);
        assert_eq!(alice.successes, 0);
        assert!(
            alice.recommend_reauth,
            "all-401 no-success should suggest re-auth"
        );

        let bob = &health["bob@example.com"];
        assert_eq!(bob.auth_failures, 0);
        assert_eq!(bob.rate_limited, 1);
        assert_eq!(bob.server_errors, 2);
        assert!(
            !bob.recommend_reauth,
            "429/500-heavy must NOT suggest re-auth"
        );

        let carol = &health["carol@example.com"];
        assert_eq!(carol.auth_failures, 2);
        assert_eq!(carol.successes, 1);
        assert!(
            !carol.recommend_reauth,
            "a recent success means auth still works"
        );
    }
}
