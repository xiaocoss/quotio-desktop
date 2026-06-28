//! 智能账号调度：按规则为每个服务商选出唯一目标账号，其余打 standby 标记
//! 临时禁用，让 CLIProxyAPI（只用未禁用账号）实际只能用目标账号。
//!
//! 规则「reset_soonest」（临近刷新优先）：额度最早刷新的账号优先——
//! 窗口余量是会过期的资源，先用快刷新的不浪费；闲置账号（无活跃窗口，
//! 一用就开全新窗口）留作储备排最后。打满、鉴权失败、用户手动禁用、
//! Codex 一键启动绑定的账号不参与调度。
//!
//! 调度覆盖所有服务商（≥2 个账号时自动生效），每个服务商独立选号。
//!
//! 设计要点：
//! - 调度器只动自己标记过的账号（`quotio_scheduler_standby`），
//!   用户手动禁用（`disabled` 无标记）永远不碰；
//! - fail-open：无可用账号 / 规则关闭 / 退出软件 → 恢复所有 standby 回池。

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use quotio_types::AccountQuota;
use serde_json::Value;

use crate::codex_launch;

/// 调度器临时禁用（待命）标记，写在账号 auth JSON 里，可还原。
pub(crate) const STANDBY_FIELD: &str = "quotio_scheduler_standby";
/// 账号健康隔离标记：401/403/auth_failed 时即使未启用智能调度也临时禁用。
pub(crate) const HEALTH_ISOLATED_FIELD: &str = "quotio_health_isolated";
/// 健康隔离原因标记："auth"(需重新授权)/"quota"(额度耗尽,等刷新)——供 UI 区分提示。
const HEALTH_ISOLATED_REASON_FIELD: &str = "quotio_health_isolated_reason";
/// Codex 一键启动绑定标记（codex_launch 写入），带它的账号调度器不碰。
const BOUND_FIELD: &str = "quotio_bound_login_only";

/// 池中一个 auth 文件的调度视角。
#[derive(Debug, Clone)]
pub struct PoolFile {
    /// 原始文件名（含 .json），auth 目录内的唯一身份。
    pub file_name: String,
    /// 清洗名（去 "{provider}-" 前缀和 ".json" 后缀），与 `AccountQuota.account_key` 对应。
    pub key: String,
    pub disabled: bool,
    /// 是否调度器自己标记的临时禁用。
    pub standby: bool,
    /// 是否因账号健康失败被临时隔离。
    pub health_isolated: bool,
    /// 健康隔离原因："auth" / "quota" / None(未隔离或升级前的旧文件)。
    pub health_isolated_reason: Option<String>,
    /// 是否被 Codex 一键启动绑定占用。
    pub bound: bool,
}

impl PoolFile {
    /// 用户手动禁用（不是调度器禁的）——调度器永远不碰。
    fn user_disabled(&self) -> bool {
        self.disabled && !self.standby && !self.health_isolated
    }
}

/// 一个参与排序的候选账号（池文件 + 配额数据合并后）。
#[derive(Debug, Clone)]
pub struct Candidate {
    pub file_name: String,
    pub key: String,
    pub label: String,
    /// 最近一次额度刷新时间（unix 秒）；None = 闲置（无窗口或已过期）。
    pub session_reset_at: Option<i64>,
    /// 总体剩余百分比（平手降权用）。
    pub weekly_remaining: f64,
    /// 是否可被选为目标（打满/鉴权失败/用户禁用/绑定占用 → false）。
    pub eligible: bool,
}

fn key_for_provider_file(file_name: &str, prefix: &str) -> String {
    let trimmed = file_name.strip_prefix(prefix).unwrap_or(file_name);
    trimmed.strip_suffix(".json").unwrap_or(trimmed).to_string()
}

/// Codex 向后兼容：扫描 Codex 账号文件。
pub fn read_pool(dir: &Path) -> Vec<PoolFile> {
    read_pool_for_provider(dir, "codex")
}

/// 扫描 auth 目录里属于指定服务商的账号文件。
/// 匹配条件：JSON `type` 字段等于 provider_id，或文件名以 `{provider_id}-` 开头。
pub fn read_pool_for_provider(dir: &Path, provider_id: &str) -> Vec<PoolFile> {
    let prefix = format!("{}-", provider_id);
    let mut pool = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return pool;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_string();
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let file_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if file_type != provider_id && !file_name.starts_with(&prefix) {
            continue;
        }
        let flag = |field: &str| {
            value
                .get(field)
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        };
        pool.push(PoolFile {
            key: key_for_provider_file(&file_name, &prefix),
            file_name,
            disabled: flag("disabled"),
            standby: flag(STANDBY_FIELD),
            health_isolated: flag(HEALTH_ISOLATED_FIELD),
            health_isolated_reason: value
                .get(HEALTH_ISOLATED_REASON_FIELD)
                .and_then(|v| v.as_str())
                .map(str::to_string),
            bound: flag(BOUND_FIELD),
        });
    }
    pool.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    pool
}

/// 扫描 auth 目录，返回每个有 ≥2 个文件的服务商 ID 列表。
pub fn discover_schedulable_providers(dir: &Path) -> Vec<String> {
    let mut counts: HashMap<String, u32> = HashMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if let Some(ptype) = value.get("type").and_then(|v| v.as_str()) {
            *counts.entry(ptype.to_string()).or_insert(0) += 1;
        }
    }
    let mut providers: Vec<String> = counts
        .into_iter()
        .filter(|(_, count)| *count >= 2)
        .map(|(provider, _)| provider)
        .collect();
    providers.sort();
    providers
}

/// 池文件 + 配额数据 → 候选列表。配额缺失（没拉到）的账号视为不可选，
/// 信息不足时宁可不动它。
pub fn build_candidates(
    pool: &[PoolFile],
    quotas: &[AccountQuota],
    now_unix: i64,
    provider_id: &str,
) -> Vec<Candidate> {
    pool.iter()
        .map(|file| {
            let quota = quotas
                .iter()
                .find(|q| q.provider_id == provider_id && q.account_key == file.key);
            // 取所有模型中最早的未过期刷新时间。
            let session_reset_at = quota.and_then(|q| {
                q.models
                    .iter()
                    .filter_map(|m| m.reset_at_unix)
                    .filter(|reset| *reset > now_unix)
                    .min()
            });
            // 取所有模型中最低的剩余百分比。
            let weekly_remaining = quota
                .map(|q| {
                    q.models
                        .iter()
                        .map(|m| m.remaining_percent)
                        .reduce(f64::min)
                        .unwrap_or(100.0)
                })
                .unwrap_or(100.0);
            let auth_failed = quota
                .map(|q| q.status_message.as_deref() == Some("auth_failed"))
                .unwrap_or(false);
            let eligible = !file.bound
                && !file.user_disabled()
                && quota.map(|q| !q.is_forbidden).unwrap_or(false)
                && !auth_failed;
            Candidate {
                file_name: file.file_name.clone(),
                key: file.key.clone(),
                label: quota
                    .map(|q| q.account_label.clone())
                    .unwrap_or_else(|| file.key.clone()),
                session_reset_at,
                weekly_remaining,
                eligible,
            }
        })
        .collect()
}

/// 额度耗尽迟滞:余量(所有窗口的最小剩余 %)≤ 3% 的账号视为耗尽,踢出可选集
/// (记入 `exhausted`),直到余量回到 > 5%(通常是窗口刷新后)才放回。3%~5% 是
/// 迟滞缓冲,避免在阈值边界反复横跳。
///
/// 这补上了 codex 配额里 `is_forbidden` 的空档:它只在 `session_used >= 100` 或
/// API 明确 block 时才置位,而「只剩 3% 但 API 仍 allowed」的号发请求会 429——本
/// 逻辑据余量提前把它移出池子,换满血待命号。
pub fn apply_exhaustion_hysteresis(
    candidates: &mut [Candidate],
    exhausted: &mut std::collections::HashSet<String>,
) {
    const PARK_AT_OR_BELOW: f64 = 3.0;
    const RESUME_ABOVE: f64 = 5.0;
    for candidate in candidates.iter_mut() {
        if exhausted.contains(&candidate.key) {
            // 已被踢:只有明显恢复(> 5%,通常是窗口刷新后)才放回。
            if candidate.weekly_remaining > RESUME_ABOVE {
                exhausted.remove(&candidate.key);
            } else {
                candidate.eligible = false;
            }
        } else if candidate.weekly_remaining <= PARK_AT_OR_BELOW {
            // 新耗尽:踢出,等它自己刷新。
            exhausted.insert(candidate.key.clone());
            candidate.eligible = false;
        }
    }
    // 池里已不存在的账号从集合里清掉,防止无限增长。
    let present: std::collections::HashSet<&str> =
        candidates.iter().map(|c| c.key.as_str()).collect();
    exhausted.retain(|k| present.contains(k.as_str()));
}

/// 排序键：活跃窗口在前（按刷新时间升序），闲置垫后；平手看 Weekly 剩余多者优先。
fn rank(candidate: &Candidate) -> (u8, i64, i64, String) {
    match candidate.session_reset_at {
        Some(reset) => (
            0,
            reset,
            -(candidate.weekly_remaining.round() as i64),
            candidate.key.clone(),
        ),
        None => (
            1,
            i64::MAX,
            -(candidate.weekly_remaining.round() as i64),
            candidate.key.clone(),
        ),
    }
}

/// 纯函数规则引擎：从候选里选目标（返回文件名）。
///
/// 滞回：当前账号仍可用时，未满最小保持时间不换；主动切换要求候选的
/// 刷新时间比当前早 `switch_margin_secs` 以上（当前闲置而候选有活跃窗口除外）。
/// 当前账号打满/出错（不再 eligible）→ 立即切最优。无可用账号 → None。
pub fn pick_target(
    candidates: &[Candidate],
    current: Option<(&str, Duration)>,
    min_hold: Duration,
    switch_margin_secs: i64,
) -> Option<String> {
    let eligible: Vec<&Candidate> = candidates.iter().filter(|c| c.eligible).collect();
    let best = eligible.iter().min_by_key(|c| rank(c))?;

    if let Some((current_file, held)) = current {
        if let Some(current) = eligible.iter().find(|c| c.file_name == current_file) {
            if best.file_name == current.file_name {
                return Some(current.file_name.clone());
            }
            if held < min_hold {
                return Some(current.file_name.clone());
            }
            return match (best.session_reset_at, current.session_reset_at) {
                // 候选要明显更早刷新才值得切。
                (Some(best_reset), Some(current_reset))
                    if best_reset + switch_margin_secs <= current_reset =>
                {
                    Some(best.file_name.clone())
                }
                // 当前已是闲置（窗口过期/未开）而候选有活跃窗口：切。
                (Some(_), None) => Some(best.file_name.clone()),
                _ => Some(current.file_name.clone()),
            };
        }
    }
    Some(best.file_name.clone())
}

/// 守门执行：让池子里只有 `target_file` 可用。
/// 其余启用中的账号打 standby 禁用；目标若被 standby 禁着则还原。
/// 绑定占用和用户手动禁用的账号不碰。返回 (是否有改动, 当前待命数)。
pub fn apply_target_in(
    dir: &Path,
    pool: &[PoolFile],
    target_file: &str,
) -> (bool, u32) {
    let mut changed = false;
    let mut standby_count = 0_u32;
    for file in pool {
        if file.bound || file.user_disabled() {
            continue;
        }
        let path = dir.join(&file.file_name);
        if file.file_name == target_file {
            if file.standby {
                changed |= set_standby(&path, false).is_ok();
            }
        } else if !file.disabled {
            if set_standby(&path, true).is_ok() {
                changed = true;
                standby_count += 1;
            }
        } else if file.standby {
            standby_count += 1;
        }
    }
    (changed, standby_count)
}

/// 恢复指定服务商的所有 standby 账号回池。
pub fn release_provider_in(dir: &Path, provider_id: &str) -> bool {
    let mut changed = false;
    for file in read_pool_for_provider(dir, provider_id) {
        if file.standby {
            changed |= set_standby(&dir.join(&file.file_name), false).is_ok();
        }
    }
    changed
}

/// fail-open / 关闭调度 / 退出软件：恢复所有服务商的 standby 账号回池。
pub fn release_all_in(dir: &Path) -> bool {
    let mut changed = false;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if value
            .get(STANDBY_FIELD)
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            changed |= set_standby(&path, false).is_ok();
        }
    }
    changed
}

/// 只有 codex 会在探测软失败时仍把账号以「空 models」占位列进 quotas(避免账号在 UI 里
/// 凭空消失);其余 provider 探测失败一律返回 `None`。因此对非-codex 而言,账号能以
/// `is_forbidden=false` 出现在 quotas 本身就证明探测成功、账号健康——空 models 也足以确证、
/// 解除隔离;对 codex 则必须有 model 数据才算确证(空 models 可能只是软失败占位)。
fn provider_lists_blank_on_probe_failure(provider_id: &str) -> bool {
    provider_id == "codex"
}

/// 根据最新配额健康状态隔离/恢复账号。
///
/// 这层不依赖智能调度：即使调度规则关闭，明确 403/鉴权失败的账号也会临时
/// `disabled=true`，避免代理默认选择器继续打到坏号；健康恢复后再自动放回。
pub fn reconcile_health_isolation_in(dir: &Path, quotas: &[AccountQuota]) -> bool {
    let mut providers: Vec<String> = quotas.iter().map(|quota| quota.provider_id.clone()).collect();
    providers.sort();
    providers.dedup();

    let mut changed = false;
    for provider_id in providers {
        let pool = read_pool_for_provider(dir, &provider_id);
        for file in pool {
            if file.bound || file.user_disabled() {
                continue;
            }
            let Some(quota) = quotas
                .iter()
                .find(|quota| quota.provider_id == provider_id && quota.account_key == file.key)
            else {
                continue;
            };
            let should_isolate = quota.is_forbidden || quota.is_auth_failure();
            // 「健康已知」才允许解除隔离,避免一次失败探测就误把坏号放回。codex 的空 models
            // 可能只是软失败占位,不算确证;非-codex 能出现在 quotas 即代表探测成功(失败会返
            // None 缺席),所以它们空 models 也算确证健康——否则恢复后无用量窗口的号会永远卡死。
            let health_known = should_isolate
                || !quota.models.is_empty()
                || !provider_lists_blank_on_probe_failure(&provider_id);
            if !health_known {
                continue;
            }
            let path = dir.join(&file.file_name);
            if should_isolate {
                // 区分隔离原因:鉴权失效要提示用户重新登录;额度耗尽只需等窗口刷新自动恢复。
                // 对已隔离的旧文件(reason 缺失)或原因变化也补写,保证 UI 标签始终正确。
                let reason = if quota.is_auth_failure() { "auth" } else { "quota" };
                let needs_write = !file.health_isolated
                    || !file.disabled
                    || file.health_isolated_reason.as_deref() != Some(reason);
                if needs_write {
                    changed |= set_health_isolated(&path, true, Some(reason)).is_ok();
                }
            } else if file.health_isolated {
                changed |= set_health_isolated(&path, false, None).is_ok();
            }
        }
    }
    changed
}

/// 用户手动「启用」账号时,清掉所有 Quotio 临时禁用标记(调度待命 standby + 健康隔离
/// health_isolated)并把账号放回池子(disabled=false),让启用立刻、彻底生效。
///
/// 关键:不能只清 health_isolated 再按残留的 standby 回填 disabled——否则一个同时被
/// 「待命」+「健康隔离」的账号,用户点「启用」会被残留的 standby 又写回 disabled=true,
/// 启用静默失效。绑定(bound)账号保持禁用;调度器下一轮会按需重新决策。
pub fn clear_temp_disable_markers_for_file_in(dir: &Path, name: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if !file_name.eq_ignore_ascii_case(name) {
            continue;
        }
        return clear_temp_disable_markers(&entry.path()).is_ok();
    }
    false
}

fn clear_temp_disable_markers(path: &Path) -> Result<(), String> {
    let mut value = codex_launch::read_proxy_account_from(path)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| format!("账号文件不是 JSON 对象: {}", path.display()))?;
    object.remove(STANDBY_FIELD);
    object.remove(HEALTH_ISOLATED_FIELD);
    object.remove(HEALTH_ISOLATED_REASON_FIELD);
    // 绑定账号必须保持禁用;其余放回池子。
    let bound = object.get(BOUND_FIELD).and_then(|v| v.as_bool()).unwrap_or(false);
    object.insert("disabled".to_string(), Value::Bool(bound));
    codex_launch::write_proxy_account_to(path, &value)
}

/// 写 standby 状态：true → `disabled=true` + 标记；false → `disabled=false` + 去标记。
/// 只在调度器确认该文件可动（非 bound、非用户禁用）后调用。
fn set_standby(path: &Path, standby: bool) -> Result<(), String> {
    let mut value = codex_launch::read_proxy_account_from(path)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| format!("账号文件不是 JSON 对象: {}", path.display()))?;
    if standby {
        object.insert("disabled".to_string(), Value::Bool(true));
        object.insert(STANDBY_FIELD.to_string(), Value::Bool(true));
    } else {
        let health_isolated = object
            .get(HEALTH_ISOLATED_FIELD)
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        object.insert("disabled".to_string(), Value::Bool(health_isolated));
        object.remove(STANDBY_FIELD);
    }
    codex_launch::write_proxy_account_to(path, &value)
}

/// 写账号健康隔离状态：true → `disabled=true` + 标记 + 原因；false → 去标记/原因并按 standby 保持禁用。
fn set_health_isolated(path: &Path, isolated: bool, reason: Option<&str>) -> Result<(), String> {
    let mut value = codex_launch::read_proxy_account_from(path)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| format!("账号文件不是 JSON 对象: {}", path.display()))?;
    if isolated {
        object.insert("disabled".to_string(), Value::Bool(true));
        object.insert(HEALTH_ISOLATED_FIELD.to_string(), Value::Bool(true));
        match reason {
            Some(reason) => {
                object.insert(
                    HEALTH_ISOLATED_REASON_FIELD.to_string(),
                    Value::String(reason.to_string()),
                );
            }
            None => {
                object.remove(HEALTH_ISOLATED_REASON_FIELD);
            }
        }
    } else {
        let standby = object
            .get(STANDBY_FIELD)
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        object.insert("disabled".to_string(), Value::Bool(standby));
        object.remove(HEALTH_ISOLATED_FIELD);
        object.remove(HEALTH_ISOLATED_REASON_FIELD);
    }
    codex_launch::write_proxy_account_to(path, &value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quotio_types::QuotaModelUsage;

    fn key_for_file(file_name: &str) -> String {
        key_for_provider_file(file_name, "codex-")
    }

    fn candidate(
        key: &str,
        session_reset_at: Option<i64>,
        weekly_remaining: f64,
        eligible: bool,
    ) -> Candidate {
        Candidate {
            file_name: format!("codex-{key}.json"),
            key: key.to_string(),
            label: format!("{key}@example.com"),
            session_reset_at,
            weekly_remaining,
            eligible,
        }
    }

    const HOLD: Duration = Duration::from_secs(600);
    const MARGIN: i64 = 900;

    #[test]
    fn key_matches_quota_clean_filename_rule() {
        assert_eq!(key_for_file("codex-abc.json"), "abc");
        assert_eq!(key_for_file("abc.json"), "abc");
        assert_eq!(key_for_file("codex-a-b.json"), "a-b");
    }

    #[test]
    fn picks_soonest_reset_and_parks_idle_accounts_last() {
        let candidates = vec![
            candidate("idle", None, 100.0, true),
            candidate("late", Some(10_000), 50.0, true),
            candidate("soon", Some(2_000), 10.0, true),
        ];
        let target = pick_target(&candidates, None, HOLD, MARGIN);
        assert_eq!(target.as_deref(), Some("codex-soon.json"));
    }

    #[test]
    fn exhaustion_hysteresis_parks_at_3pct_and_resumes_above_5pct() {
        let mut exhausted = std::collections::HashSet::new();

        // soon 余量 3%(快刷新但快耗尽)→ 踢出;pick_target 改选满血的 late。
        let mut candidates = vec![
            candidate("soon", Some(2_000), 3.0, true),
            candidate("late", Some(10_000), 50.0, true),
        ];
        apply_exhaustion_hysteresis(&mut candidates, &mut exhausted);
        assert!(!candidates[0].eligible, "soon 3% 应被踢出可选");
        assert!(candidates[1].eligible);
        assert!(exhausted.contains("soon"));
        assert_eq!(
            pick_target(&candidates, None, HOLD, MARGIN).as_deref(),
            Some("codex-late.json"),
            "耗尽号被踢后改选满血号,不再死磕"
        );

        // 迟滞:soon 小幅回到 4%(仍 < 5%)→ 不放回。
        let mut candidates = vec![candidate("soon", Some(2_000), 4.0, true)];
        apply_exhaustion_hysteresis(&mut candidates, &mut exhausted);
        assert!(!candidates[0].eligible, "4% 在 3~5% 迟滞缓冲内,不放回");
        assert!(exhausted.contains("soon"));

        // 刷新到 99%(> 5%)→ 放回可选。
        let mut candidates = vec![candidate("soon", Some(2_000), 99.0, true)];
        apply_exhaustion_hysteresis(&mut candidates, &mut exhausted);
        assert!(candidates[0].eligible, "恢复 > 5% 放回");
        assert!(!exhausted.contains("soon"));
    }

    #[test]
    fn skips_ineligible_and_falls_back_to_idle_reserve() {
        let candidates = vec![
            candidate("forbidden", Some(1_000), 80.0, false),
            candidate("idle-a", None, 30.0, true),
            candidate("idle-b", None, 90.0, true),
        ];
        // 闲置储备里选 Weekly 剩余多的。
        let target = pick_target(&candidates, None, HOLD, MARGIN);
        assert_eq!(target.as_deref(), Some("codex-idle-b.json"));
    }

    #[test]
    fn returns_none_when_no_account_is_eligible() {
        let candidates = vec![candidate("a", Some(1_000), 0.0, false)];
        assert_eq!(pick_target(&candidates, None, HOLD, MARGIN), None);
    }

    #[test]
    fn keeps_current_within_min_hold() {
        let candidates = vec![
            candidate("cur", Some(9_000), 50.0, true),
            candidate("soon", Some(1_000), 50.0, true),
        ];
        let target = pick_target(
            &candidates,
            Some(("codex-cur.json", Duration::from_secs(60))),
            HOLD,
            MARGIN,
        );
        assert_eq!(target.as_deref(), Some("codex-cur.json"));
    }

    #[test]
    fn switches_only_when_candidate_is_better_by_margin() {
        let candidates = vec![
            candidate("cur", Some(3_000), 50.0, true),
            candidate("soon", Some(2_500), 50.0, true),
        ];
        // 仅早 500s < 900s 门槛：不切。
        let held = Some(("codex-cur.json", Duration::from_secs(3_600)));
        assert_eq!(
            pick_target(&candidates, held, HOLD, MARGIN).as_deref(),
            Some("codex-cur.json")
        );
        // 早 1200s ≥ 门槛：切。
        let candidates = vec![
            candidate("cur", Some(3_000), 50.0, true),
            candidate("soon", Some(1_800), 50.0, true),
        ];
        assert_eq!(
            pick_target(&candidates, held, HOLD, MARGIN).as_deref(),
            Some("codex-soon.json")
        );
    }

    #[test]
    fn switches_immediately_when_current_becomes_ineligible() {
        let candidates = vec![
            candidate("cur", Some(1_000), 50.0, false), // 打满
            candidate("next", Some(5_000), 50.0, true),
        ];
        let target = pick_target(
            &candidates,
            Some(("codex-cur.json", Duration::from_secs(10))),
            HOLD,
            MARGIN,
        );
        assert_eq!(target.as_deref(), Some("codex-next.json"));
    }

    #[test]
    fn switches_off_idle_current_when_active_window_appears() {
        // 当前账号窗口已过期（闲置），候选有活跃窗口：过保持期后立即切。
        let candidates = vec![
            candidate("cur", None, 50.0, true),
            candidate("active", Some(9_999), 50.0, true),
        ];
        let target = pick_target(
            &candidates,
            Some(("codex-cur.json", Duration::from_secs(3_600))),
            HOLD,
            MARGIN,
        );
        assert_eq!(target.as_deref(), Some("codex-active.json"));
    }

    #[test]
    fn build_candidates_joins_quota_and_normalizes_expired_window() {
        let pool = vec![
            PoolFile {
                file_name: "codex-a.json".into(),
                key: "a".into(),
                disabled: false,
                standby: false,
                health_isolated: false,
                health_isolated_reason: None,
                bound: false,
            },
            PoolFile {
                file_name: "codex-b.json".into(),
                key: "b".into(),
                disabled: true, // 用户手动禁用
                standby: false,
                health_isolated: false,
                health_isolated_reason: None,
                bound: false,
            },
        ];
        let quota = |key: &str, reset: Option<i64>, forbidden: bool| AccountQuota {
            provider_id: "codex".into(),
            account_label: format!("{key}@x.com"),
            account_key: key.into(),
            is_forbidden: forbidden,
            status_message: None,
            models: vec![QuotaModelUsage {
                model: "Session".into(),
                used_percent: 10.0,
                remaining_percent: 90.0,
                reset_at: None,
                reset_at_unix: reset,
            }],
        };
        let now = 5_000;
        let quotas = vec![quota("a", Some(3_000), false), quota("b", Some(9_000), false)];
        let candidates = build_candidates(&pool, &quotas, now, "codex");

        // a 的窗口 3000 < now=5000 → 过期视为闲置。
        assert_eq!(candidates[0].session_reset_at, None);
        assert!(candidates[0].eligible);
        // b 用户手动禁用 → 不可选。
        assert!(!candidates[1].eligible);
    }

    #[test]
    fn apply_and_release_roundtrip_respects_user_disabled_and_bound() {
        let dir = std::env::temp_dir().join(format!(
            "ql_scheduler_apply_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, body: &str| std::fs::write(dir.join(name), body).unwrap();
        let base = r#""type":"codex","access_token":"a","id_token":"i","refresh_token":"r""#;
        write("codex-target.json", &format!("{{{base}}}"));
        write("codex-other.json", &format!("{{{base}}}"));
        write(
            "codex-user-off.json",
            &format!("{{{base},\"disabled\":true}}"),
        );
        write(
            "codex-bound.json",
            &format!("{{{base},\"disabled\":true,\"quotio_bound_login_only\":true}}"),
        );

        let pool = read_pool(&dir);
        assert_eq!(pool.len(), 4);
        let (changed, standby_count) = apply_target_in(&dir, &pool, "codex-target.json");
        assert!(changed);
        assert_eq!(standby_count, 1);

        let pool = read_pool(&dir);
        let by_name = |name: &str| pool.iter().find(|f| f.file_name == name).unwrap();
        assert!(!by_name("codex-target.json").disabled);
        assert!(by_name("codex-other.json").disabled);
        assert!(by_name("codex-other.json").standby);
        // 用户手动禁用、绑定占用：原样不动。
        assert!(by_name("codex-user-off.json").disabled);
        assert!(!by_name("codex-user-off.json").standby);
        assert!(by_name("codex-bound.json").disabled);
        assert!(!by_name("codex-bound.json").standby);

        assert!(release_all_in(&dir));
        let pool = read_pool(&dir);
        let by_name = |name: &str| pool.iter().find(|f| f.file_name == name).unwrap();
        assert!(!by_name("codex-other.json").disabled);
        assert!(!by_name("codex-other.json").standby);
        assert!(by_name("codex-user-off.json").disabled);
        assert!(by_name("codex-bound.json").disabled);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn health_isolation_parks_forbidden_accounts_and_restores_recovered() {
        let dir = std::env::temp_dir().join(format!(
            "ql_scheduler_health_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let base = r#""type":"codex","access_token":"a","id_token":"i","refresh_token":"r""#;
        std::fs::write(dir.join("codex-bad.json"), format!("{{{base}}}")).unwrap();
        std::fs::write(dir.join("codex-good.json"), format!("{{{base}}}")).unwrap();
        std::fs::write(
            dir.join("codex-user-off.json"),
            format!("{{{base},\"disabled\":true}}"),
        )
        .unwrap();

        let quota = |key: &str, forbidden: bool, status_message: Option<&str>| AccountQuota {
            provider_id: "codex".into(),
            account_label: format!("{key}@x.com"),
            account_key: key.into(),
            is_forbidden: forbidden,
            status_message: status_message.map(str::to_string),
            models: vec![QuotaModelUsage {
                model: "Session".into(),
                used_percent: 10.0,
                remaining_percent: 90.0,
                reset_at: None,
                reset_at_unix: Some(9_000),
            }],
        };

        let changed = reconcile_health_isolation_in(
            &dir,
            &[
                quota("bad", true, None),
                quota("good", false, None),
                quota("user-off", true, None),
            ],
        );
        assert!(changed, "forbidden account should be isolated");

        let pool = read_pool(&dir);
        let by_name = |name: &str| pool.iter().find(|f| f.file_name == name).unwrap();
        assert!(by_name("codex-bad.json").disabled);
        assert!(by_name("codex-bad.json").health_isolated);
        assert!(!by_name("codex-good.json").disabled);
        assert!(!by_name("codex-good.json").health_isolated);
        assert!(by_name("codex-user-off.json").disabled);
        assert!(!by_name("codex-user-off.json").health_isolated);

        let changed = reconcile_health_isolation_in(
            &dir,
            &[
                quota("bad", false, None),
                quota("good", false, None),
                quota("user-off", true, None),
            ],
        );
        assert!(changed, "recovered account should be restored");
        let pool = read_pool(&dir);
        let by_name = |name: &str| pool.iter().find(|f| f.file_name == name).unwrap();
        assert!(!by_name("codex-bad.json").disabled);
        assert!(!by_name("codex-bad.json").health_isolated);
        assert!(by_name("codex-user-off.json").disabled);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn health_isolation_keeps_isolated_account_on_blank_probe() {
        let dir = std::env::temp_dir().join(format!(
            "ql_scheduler_health_blank_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let base = r#""type":"codex","access_token":"a","id_token":"i","refresh_token":"r""#;
        std::fs::write(
            dir.join("codex-bad.json"),
            format!("{{{base},\"disabled\":true,\"quotio_health_isolated\":true}}"),
        )
        .unwrap();

        let changed = reconcile_health_isolation_in(
            &dir,
            &[AccountQuota {
                provider_id: "codex".into(),
                account_label: "bad@x.com".into(),
                account_key: "bad".into(),
                is_forbidden: false,
                status_message: None,
                models: Vec::new(),
            }],
        );
        assert!(!changed, "blank probe should not clear a health isolation marker");

        let pool = read_pool(&dir);
        let bad = pool.iter().find(|f| f.file_name == "codex-bad.json").unwrap();
        assert!(bad.disabled);
        assert!(bad.health_isolated);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn manual_enable_clears_temp_markers_but_keeps_bound_disabled() {
        let dir = std::env::temp_dir().join(format!(
            "ql_scheduler_manual_enable_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let base = r#""type":"codex","access_token":"a","id_token":"i","refresh_token":"r""#;
        // (1) 同时挂 standby + health_isolated(真实可发生:先被调度待命,后又因 403 被
        //     健康隔离)。用户点「启用」必须一次清干净、disabled=false —— HIGH ① 回归点。
        std::fs::write(
            dir.join("codex-MixedCase.json"),
            format!(
                "{{{base},\"disabled\":true,\"quotio_scheduler_standby\":true,\"quotio_health_isolated\":true}}"
            ),
        )
        .unwrap();
        // (2) 绑定账号:启用调用不应把它放回(保持 disabled=true)。
        std::fs::write(
            dir.join("codex-bound.json"),
            format!("{{{base},\"disabled\":true,\"quotio_bound_login_only\":true}}"),
        )
        .unwrap();

        // 文件名大小写不敏感匹配。
        assert!(clear_temp_disable_markers_for_file_in(&dir, "codex-mixedcase.json"));
        assert!(clear_temp_disable_markers_for_file_in(&dir, "codex-bound.json"));

        let pool = read_pool(&dir);
        let account = pool
            .iter()
            .find(|file| file.file_name == "codex-MixedCase.json")
            .unwrap();
        assert!(!account.disabled, "手动启用必须真正生效(disabled=false)");
        assert!(!account.standby, "standby 标记必须清掉");
        assert!(!account.health_isolated, "health_isolated 标记必须清掉");

        let bound = pool
            .iter()
            .find(|file| file.file_name == "codex-bound.json")
            .unwrap();
        assert!(bound.disabled, "绑定账号必须保持禁用");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn health_isolation_repairs_marker_with_disabled_cleared() {
        let dir = std::env::temp_dir().join(format!(
            "ql_scheduler_health_repair_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let base = r#""type":"codex","access_token":"a","id_token":"i","refresh_token":"r""#;
        std::fs::write(
            dir.join("codex-bad.json"),
            format!("{{{base},\"disabled\":false,\"quotio_health_isolated\":true}}"),
        )
        .unwrap();

        let changed = reconcile_health_isolation_in(
            &dir,
            &[AccountQuota {
                provider_id: "codex".into(),
                account_label: "bad@x.com".into(),
                account_key: "bad".into(),
                is_forbidden: true,
                status_message: None,
                models: vec![QuotaModelUsage {
                    model: "Session".into(),
                    used_percent: 100.0,
                    remaining_percent: 0.0,
                    reset_at: None,
                    reset_at_unix: Some(9_000),
                }],
            }],
        );
        assert!(changed, "health marker with disabled=false should be repaired");

        let pool = read_pool(&dir);
        let bad = pool.iter().find(|f| f.file_name == "codex-bad.json").unwrap();
        assert!(bad.disabled);
        assert!(bad.health_isolated);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn health_isolation_records_auth_vs_quota_reason() {
        let dir = std::env::temp_dir().join(format!(
            "ql_scheduler_reason_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let base = r#""type":"codex","access_token":"a","id_token":"i","refresh_token":"r""#;
        std::fs::write(dir.join("codex-authbad.json"), format!("{{{base}}}")).unwrap();
        std::fs::write(dir.join("codex-quotabad.json"), format!("{{{base}}}")).unwrap();

        let q = |key: &str, forbidden: bool, msg: Option<&str>, models: Vec<QuotaModelUsage>| {
            AccountQuota {
                provider_id: "codex".into(),
                account_label: format!("{key}@x.com"),
                account_key: key.into(),
                is_forbidden: forbidden,
                status_message: msg.map(str::to_string),
                models,
            }
        };
        let model = || {
            vec![QuotaModelUsage {
                model: "Session".into(),
                used_percent: 100.0,
                remaining_percent: 0.0,
                reset_at: None,
                reset_at_unix: Some(9_000),
            }]
        };

        // authbad: codex 401 不可恢复 → is_forbidden=false + "auth_failed" + 空 models。
        // quotabad: 额度打满 → is_forbidden=true,无鉴权哨兵。
        assert!(reconcile_health_isolation_in(
            &dir,
            &[
                q("authbad", false, Some("auth_failed"), Vec::new()),
                q("quotabad", true, None, model()),
            ],
        ));
        let pool = read_pool(&dir);
        let by = |name: &str| pool.iter().find(|f| f.file_name == name).unwrap().clone();
        assert!(by("codex-authbad.json").health_isolated);
        assert_eq!(
            by("codex-authbad.json").health_isolated_reason.as_deref(),
            Some("auth"),
            "鉴权失效隔离原因应为 auth"
        );
        assert!(by("codex-quotabad.json").health_isolated);
        assert_eq!(
            by("codex-quotabad.json").health_isolated_reason.as_deref(),
            Some("quota"),
            "额度耗尽隔离原因应为 quota"
        );

        // 原因变化:quotabad 之后令牌也失效 → reason 应被改写为 auth。
        assert!(reconcile_health_isolation_in(
            &dir,
            &[q("quotabad", false, Some("auth_failed"), Vec::new())],
        ));
        let pool = read_pool(&dir);
        let quotabad = pool
            .iter()
            .find(|f| f.file_name == "codex-quotabad.json")
            .unwrap();
        assert_eq!(quotabad.health_isolated_reason.as_deref(), Some("auth"));

        // 恢复:authbad 健康了 → 隔离标记与原因一并清掉。
        assert!(reconcile_health_isolation_in(
            &dir,
            &[q("authbad", false, None, model())],
        ));
        let pool = read_pool(&dir);
        let authbad = pool
            .iter()
            .find(|f| f.file_name == "codex-authbad.json")
            .unwrap();
        assert!(!authbad.health_isolated);
        assert!(authbad.health_isolated_reason.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn is_auth_failure_matches_provider_sentinels() {
        let q = |msg: Option<&str>, forbidden: bool| AccountQuota {
            provider_id: "x".into(),
            account_label: "x".into(),
            account_key: "x".into(),
            is_forbidden: forbidden,
            status_message: msg.map(str::to_string),
            models: Vec::new(),
        };
        assert!(q(Some("auth_failed"), false).is_auth_failure());
        assert!(q(Some("需要重新授权"), true).is_auth_failure());
        assert!(q(Some("需要重新登录"), true).is_auth_failure());
        assert!(q(Some("密钥无效"), true).is_auth_failure());
        // 额度耗尽 / 健康 / plan 串都不是鉴权失败。
        assert!(!q(None, true).is_auth_failure());
        assert!(!q(Some("plan: pro | resets: 3"), true).is_auth_failure());
        assert!(!q(None, false).is_auth_failure());
    }

    #[test]
    fn non_codex_recovers_on_blank_healthy_probe_but_codex_does_not() {
        let dir = std::env::temp_dir().join(format!(
            "ql_scheduler_blank_recover_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // 两个号当前都被健康隔离。
        std::fs::write(
            dir.join("claude-acc.json"),
            r#"{"type":"claude","access_token":"a","disabled":true,"quotio_health_isolated":true,"quotio_health_isolated_reason":"auth"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("codex-acc.json"),
            r#"{"type":"codex","access_token":"a","id_token":"i","refresh_token":"r","disabled":true,"quotio_health_isolated":true,"quotio_health_isolated_reason":"quota"}"#,
        )
        .unwrap();

        // 两者都返回「健康但空 models」的探测结果(is_forbidden=false、非鉴权、无额度条)。
        let blank = |provider: &str, key: &str| AccountQuota {
            provider_id: provider.into(),
            account_label: format!("{key}@x.com"),
            account_key: key.into(),
            is_forbidden: false,
            status_message: None,
            models: Vec::new(),
        };
        let changed =
            reconcile_health_isolation_in(&dir, &[blank("claude", "acc"), blank("codex", "acc")]);
        assert!(changed, "claude 应被解除隔离 → changed");

        // claude:非-codex,出现在 quotas 即证明探测成功 → 空白也算健康 → 解除隔离。
        let claude = read_pool_for_provider(&dir, "claude");
        let c = claude.iter().find(|f| f.file_name == "claude-acc.json").unwrap();
        assert!(!c.disabled, "claude 恢复后(空白健康)必须解除隔离,不再卡死");
        assert!(!c.health_isolated);

        // codex:空 models 可能只是软失败占位,不据此解除隔离 → 保持隔离。
        let codex = read_pool(&dir);
        let x = codex.iter().find(|f| f.file_name == "codex-acc.json").unwrap();
        assert!(x.disabled, "codex 空白探测不解除隔离(可能只是软失败)");
        assert!(x.health_isolated);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
