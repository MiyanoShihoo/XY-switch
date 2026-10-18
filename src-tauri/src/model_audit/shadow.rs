//! Shadow pairing keeps only bounded, expiring hashes in memory. Only numeric
//! pairs and an opaque session partition identifier may be persisted.
use super::LocalTokenizer;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, VecDeque};

const CACHE_CAPACITY: usize = 256;
const TTL_MS: u64 = 30 * 60 * 1000;
const MAX_ADDED_BYTES: usize = 8 * 1024;
// Two fitted coefficients need at least one residual degree of freedom.
const MIN_PAIRS: usize = 3;

/// Transient collector input: never store or log this structure. The collector
/// hashes canonical history/configuration and verifies strict plain-text roles.
#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShadowSnapshot {
    pub partition_key: String,
    pub history_key: String,
    pub parent_history_key: Option<String>,
    pub history_messages: u64,
    pub added_assistant: Option<String>,
    pub added_user: Option<String>,
    pub eligible: bool,
    pub exclusion_reason: Option<String>,
}

/// Completed plain-text response metadata; no cancellation or partial response
/// is eligible to become a baseline. Cache absence is not a known zero.
#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShadowCompleted {
    pub snapshot: ShadowSnapshot,
    pub input_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub complete: bool,
    pub failover: bool,
    pub assistant_sha256: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShadowPair {
    pub partition_id: String,
    pub x_local_tokens: u64,
    pub y_input_delta: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShadowOutcome {
    pub status: String,
    pub reason: Option<String>,
    pub pair: Option<ShadowPair>,
}

struct Entry {
    partition: String,
    group: String,
    history: String,
    messages: u64,
    input: u64,
    assistant_sha256: String,
    observed_ms: u64,
    consumed: bool,
    eligible: bool,
}

pub struct ShadowEngine {
    session_id: String,
    sequence: u64,
    entries: VecDeque<Entry>,
    last_ms: Option<u64>,
}

fn valid_sha(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn outcome(status: &str, reason: &str) -> ShadowOutcome {
    ShadowOutcome {
        status: status.into(),
        reason: Some(reason.into()),
        pair: None,
    }
}

impl ShadowEngine {
    /// session_id must be an opaque random identifier chosen by the runtime,
    /// never derived from a prompt, credential, endpoint or system message.
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            sequence: 0,
            entries: VecDeque::new(),
            last_ms: None,
        }
    }

    fn invalidate(&mut self, sample: &ShadowCompleted, tokenizer: &LocalTokenizer, now_ms: u64) {
        let snapshot = &sample.snapshot;
        if !valid_sha(&snapshot.partition_key) || !valid_sha(&snapshot.history_key) {
            return;
        }
        let partition = format!("{}:{}", snapshot.partition_key, tokenizer.sha256());
        let mut found = false;
        for entry in &mut self.entries {
            if entry.partition == partition && entry.history == snapshot.history_key {
                entry.eligible = false;
                entry.observed_ms = now_ms;
                found = true;
            }
        }
        if found {
            return;
        }
        let group = if let Some(entry) = self
            .entries
            .iter()
            .rev()
            .find(|entry| entry.partition == partition)
        {
            entry.group.clone()
        } else {
            self.sequence += 1;
            format!("{}-{}", self.session_id, self.sequence)
        };
        if self.entries.len() == CACHE_CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            partition,
            group,
            history: snapshot.history_key.clone(),
            messages: snapshot.history_messages,
            input: 0,
            assistant_sha256: String::new(),
            observed_ms: now_ms,
            consumed: false,
            eligible: false,
        });
    }

    pub fn observe(
        &mut self,
        sample: ShadowCompleted,
        tokenizer: &LocalTokenizer,
        now_ms: u64,
    ) -> ShadowOutcome {
        if self.last_ms.is_some_and(|previous| now_ms < previous) {
            self.entries.clear();
            self.last_ms = Some(now_ms);
            return outcome("excluded", "shadow.clock_regressed");
        }
        self.last_ms = Some(now_ms);
        self.entries
            .retain(|entry| now_ms.saturating_sub(entry.observed_ms) <= TTL_MS);
        let snapshot = &sample.snapshot;
        macro_rules! exclude {
            ($reason:expr) => {{
                self.invalidate(&sample, tokenizer, now_ms);
                return outcome("excluded", $reason);
            }};
        }
        if !snapshot.eligible {
            exclude!(
                snapshot
                    .exclusion_reason
                    .as_deref()
                    .unwrap_or("shadow.ineligible_history")
            );
        }
        if !sample.complete {
            exclude!("shadow.incomplete");
        }
        if sample.failover {
            exclude!("shadow.failover");
        }
        let Some(input) = sample
            .input_tokens
            .filter(|value| *value > 0 && *value <= (1u64 << 53))
        else {
            exclude!("shadow.input_unknown");
        };
        match (sample.cache_read_tokens, sample.cache_creation_tokens) {
            (Some(0), Some(0)) => (),
            (None, _) | (_, None) => exclude!("shadow.cache_unknown"),
            _ => exclude!("shadow.cache_present"),
        }
        let Some(response_hash) = sample
            .assistant_sha256
            .as_deref()
            .filter(|value| valid_sha(value))
        else {
            exclude!("shadow.response_unknown");
        };
        if !valid_sha(&snapshot.partition_key)
            || !valid_sha(&snapshot.history_key)
            || snapshot
                .parent_history_key
                .as_deref()
                .is_some_and(|value| !valid_sha(value))
        {
            exclude!("shadow.invalid_fingerprint");
        }
        if snapshot.history_messages == 0
            || snapshot.history_messages % 2 == 0
            || (snapshot.history_messages == 1
                && (snapshot.parent_history_key.is_some()
                    || snapshot.added_assistant.is_some()
                    || snapshot.added_user.is_some()))
            || (snapshot.history_messages > 1
                && (snapshot.parent_history_key.is_none()
                    || snapshot.added_assistant.is_none()
                    || snapshot.added_user.is_none()))
        {
            exclude!("shadow.history_shape");
        }
        if snapshot
            .added_assistant
            .as_ref()
            .map_or(0, String::len)
            .saturating_add(snapshot.added_user.as_ref().map_or(0, String::len))
            > MAX_ADDED_BYTES
        {
            exclude!("shadow.additions_too_large");
        }
        // The tokenizer source is part of the in-memory partition even if the
        // collector has omitted it from the configuration hash.
        let partition = format!("{}:{}", snapshot.partition_key, tokenizer.sha256());
        let group = if let Some(entry) = self
            .entries
            .iter()
            .rev()
            .find(|entry| entry.partition == partition)
        {
            entry.group.clone()
        } else {
            self.sequence += 1;
            format!("{}-{}", self.session_id, self.sequence)
        };
        let repeated = self
            .entries
            .iter()
            .any(|entry| entry.partition == partition && entry.history == snapshot.history_key);
        let mut result = outcome("baseline", "shadow.no_parent");
        if repeated {
            // Keep each completed response hash. A later request may resolve a
            // repeated prompt only if its actual assistant response is unique.
            result = outcome("excluded", "shadow.ambiguous_prefix");
        } else if let (Some(parent), Some(assistant), Some(user)) = (
            &snapshot.parent_history_key,
            &snapshot.added_assistant,
            &snapshot.added_user,
        ) {
            let expected_response = format!("{:x}", Sha256::digest(assistant.as_bytes()));
            let prefix_matches: Vec<_> = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    entry.partition == partition
                        && entry.history == *parent
                        && entry.messages.checked_add(2) == Some(snapshot.history_messages)
                })
                .map(|(index, _)| index)
                .collect();
            let matching: Vec<_> = prefix_matches
                .iter()
                .copied()
                .filter(|index| {
                    self.entries[*index]
                        .assistant_sha256
                        .eq_ignore_ascii_case(&expected_response)
                })
                .collect();
            if prefix_matches
                .iter()
                .any(|index| !self.entries[*index].eligible)
            {
                result = outcome("excluded", "shadow.parent_ineligible");
            } else if matching.len() > 1 {
                result = outcome("excluded", "shadow.ambiguous_prefix");
            } else if let Some(index) = matching.first().copied() {
                let previous = &self.entries[index];
                if previous.consumed {
                    result = outcome("excluded", "shadow.parent_reused");
                } else if input <= previous.input {
                    result = outcome("excluded", "shadow.non_positive_delta");
                } else {
                    // Count the two role contents separately. Artificially
                    // concatenating them could introduce cross-message merges.
                    match tokenizer
                        .count(assistant)
                        .and_then(|left| tokenizer.count(user).map(|right| left + right))
                    {
                        Ok(x) if x > 0 => {
                            result = ShadowOutcome {
                                status: "paired".into(),
                                reason: None,
                                pair: Some(ShadowPair {
                                    partition_id: group.clone(),
                                    x_local_tokens: x,
                                    y_input_delta: (input - previous.input) as i64,
                                }),
                            };
                            self.entries[index].consumed = true;
                        }
                        _ => result = outcome("excluded", "shadow.tokenizer_error"),
                    }
                }
            } else if !prefix_matches.is_empty() {
                result = outcome("excluded", "shadow.response_mismatch");
            }
        }
        if self.entries.len() == CACHE_CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            partition,
            group,
            history: snapshot.history_key.clone(),
            messages: snapshot.history_messages,
            input,
            assistant_sha256: response_hash.to_ascii_lowercase(),
            observed_ms: now_ms,
            consumed: false,
            eligible: true,
        });
        // `sample`, including the transient assistant/user text, drops here.
        result
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShadowRegressionReport {
    pub status: String,
    pub sample_count: usize,
    pub minimum_samples: usize,
    pub slope: Option<f64>,
    pub intercept: Option<f64>,
    pub slope_ci_low: Option<f64>,
    pub slope_ci_high: Option<f64>,
    pub residual_rmse: Option<f64>,
    pub x_span: Option<f64>,
    pub intercept_drift: Option<f64>,
    pub gaps: Vec<String>,
    pub limitations: Vec<String>,
}

/// Ordinary least squares with HC3 heteroskedasticity-robust slope uncertainty.
/// Small samples use their actual residual degrees of freedom. Values are
/// rounded up; retaining t(18) above 18 degrees of freedom is conservative.
fn slope_critical_value(samples: usize) -> f64 {
    const T95: [f64; 18] = [
        12.707, 4.303, 3.183, 2.777, 2.572, 2.447, 2.365, 2.307, 2.263,
        2.229, 2.201, 2.179, 2.161, 2.145, 2.132, 2.120, 2.110, 2.101,
    ];
    T95[samples.saturating_sub(3).min(T95.len() - 1)]
}

/// The required evidence depends on residual noise and x variation through the
/// slope interval, not on a fixed production sample count or raw RMSE cutoff.
/// HC3 intervals remain approximate and require representative, independent data.
pub fn regress_shadow(pairs: &[ShadowPair]) -> ShadowRegressionReport {
    let mut report = ShadowRegressionReport {status:"untested".into(),sample_count:pairs.len(),minimum_samples:MIN_PAIRS,
        limitations:vec![
            "仅为被动统计线索，斜率偏离 1 不能证明虚报或确定模型身份。".into(),
            "本地分词器、角色模板、隐藏 system/prompt 改写仍可能解释差异；相同词表无法区分型号。".into(),
            "x 为新增 assistant 与 user 文本分别计数之和；固定角色开销由截距吸收，变化的模板开销可能影响斜率。".into(),
            "3 对仅是回归计算下限；判读还需 5 个不同 x、至少 32 的跨度，并按残差对应的斜率区间与时间稳定性判断，不固定要求 20 对。".into(),
            "小样本使用对应自由度的 t 临界值，但 HC3 区间仍是近似；噪声较大或区间不明确时继续采样。".into(),
            "HC3 置信区间仍依赖样本独立性与代表性；相邻对话可能相关，不输出客观可信度分数。".into(),
        ],..Default::default()};
    if pairs.len() < MIN_PAIRS {
        report.gaps.push("shadow.insufficient_pairs".into());
        return report;
    }
    if pairs.len() > 10000
        || pairs.iter().any(|pair| {
            pair.x_local_tokens == 0
                || pair.x_local_tokens > (1u64 << 53)
                || pair.y_input_delta <= 0
                || pair.y_input_delta as u64 > (1u64 << 53)
                || pair.partition_id.is_empty()
        })
    {
        report.gaps.push("shadow.invalid_pairs".into());
        return report;
    }
    if pairs
        .iter()
        .any(|pair| pair.partition_id != pairs[0].partition_id)
    {
        report.gaps.push("shadow.partition_mixed".into());
        return report;
    }
    let distinct: BTreeSet<_> = pairs.iter().map(|pair| pair.x_local_tokens).collect();
    let span = (*distinct.last().unwrap() - *distinct.first().unwrap()) as f64;
    report.x_span = Some(span);
    if distinct.len() < 5 || span < 32.0 {
        report.gaps.push("shadow.x_variance_insufficient".into());
        return report;
    }
    let n = pairs.len() as f64;
    let mean_x = pairs
        .iter()
        .map(|pair| pair.x_local_tokens as f64)
        .sum::<f64>()
        / n;
    let mean_y = pairs
        .iter()
        .map(|pair| pair.y_input_delta as f64)
        .sum::<f64>()
        / n;
    let sxx = pairs
        .iter()
        .map(|pair| (pair.x_local_tokens as f64 - mean_x).powi(2))
        .sum::<f64>();
    let sxy = pairs
        .iter()
        .map(|pair| (pair.x_local_tokens as f64 - mean_x) * (pair.y_input_delta as f64 - mean_y))
        .sum::<f64>();
    let slope = sxy / sxx;
    let intercept = mean_y - slope * mean_x;
    let residuals: Vec<_> = pairs
        .iter()
        .map(|pair| pair.y_input_delta as f64 - intercept - slope * pair.x_local_tokens as f64)
        .collect();
    let rmse = (residuals
        .iter()
        .map(|residual| residual.powi(2))
        .sum::<f64>()
        / (n - 2.0))
        .sqrt();
    let robust_variance = pairs
        .iter()
        .zip(&residuals)
        .map(|(pair, residual)| {
            let dx = pair.x_local_tokens as f64 - mean_x;
            let leverage = 1.0 / n + dx * dx / sxx;
            (dx * residual / (1.0 - leverage).max(1e-12)).powi(2)
        })
        .sum::<f64>()
        / (sxx * sxx);
    let critical_value = slope_critical_value(pairs.len());
    let margin = critical_value * robust_variance.sqrt();
    let half = pairs.len() / 2;
    let first = residuals[..half].iter().sum::<f64>() / half as f64;
    let second = residuals[half..].iter().sum::<f64>() / (pairs.len() - half) as f64;
    let drift = (second - first).abs();
    report.slope = Some(slope);
    report.intercept = Some(intercept);
    report.slope_ci_low = Some(slope - margin);
    report.slope_ci_high = Some(slope + margin);
    report.residual_rmse = Some(rmse);
    report.intercept_drift = Some(drift);
    let drift_bound =
        4.0f64.max(critical_value * rmse * (1.0 / half as f64 + 1.0 / (pairs.len() - half) as f64).sqrt());
    if drift > drift_bound {
        report.gaps.push("shadow.intercept_unstable".into());
    }
    if !report.gaps.is_empty() {
        return report;
    }
    if slope - margin > 1.05 || slope + margin < 0.95 {
        report.status = "suspicious".into();
    } else if slope - margin >= 0.95 && slope + margin <= 1.05 {
        report.status = "consistent".into();
    } else {
        report.gaps.push("shadow.slope_uncertain".into());
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_audit_shadow_requires_five_distinct_increments() {
        for count in [4, 5] {
            let pairs: Vec<ShadowPair> = (0..count * 6)
                .map(|index| {
                    let x = 40 + (index % count) * 40;
                    ShadowPair {
                        partition_id: "diversity-fixture".into(),
                        x_local_tokens: x,
                        y_input_delta: (x * 12 / 10 + 8) as i64,
                    }
                })
                .collect();
            let report = regress_shadow(&pairs);
            if count == 4 {
                assert_eq!(report.status, "untested");
                assert!(report.gaps.iter().any(|gap| gap == "shadow.x_variance_insufficient"));
                assert!(report.slope.is_none());
            } else {
                assert_eq!(report.status, "suspicious");
                assert!((report.slope.unwrap() - 1.2).abs() < 1e-9);
                assert!(report.gaps.is_empty());
            }
        }
    }
    fn hash(text: &str) -> String {
        format!("{:x}", Sha256::digest(text.as_bytes()))
    }
    fn sample(
        history: &str,
        parent: Option<&str>,
        messages: u64,
        input: u64,
        assistant: &str,
        user: &str,
        response: &str,
    ) -> ShadowCompleted {
        ShadowCompleted {
            snapshot: ShadowSnapshot {
                partition_key: hash("partition"),
                history_key: hash(history),
                parent_history_key: parent.map(hash),
                history_messages: messages,
                added_assistant: parent.map(|_| assistant.into()),
                added_user: parent.map(|_| user.into()),
                eligible: true,
                ..Default::default()
            },
            input_tokens: Some(input),
            cache_read_tokens: Some(0),
            cache_creation_tokens: Some(0),
            complete: true,
            assistant_sha256: Some(hash(response)),
            ..Default::default()
        }
    }
    #[test]
    fn model_audit_shadow_real_prefix_and_response_pair() {
        let mut engine = ShadowEngine::new("opaque-session".into());
        let tok = LocalTokenizer::byte_fixture();
        assert_eq!(
            engine
                .observe(sample("first", None, 1, 100, "", "", "answer"), &tok, 0)
                .status,
            "baseline"
        );
        let paired = engine.observe(
            sample(
                "next",
                Some("first"),
                3,
                120,
                "answer",
                "followup",
                "next answer",
            ),
            &tok,
            1,
        );
        let pair = paired.pair.unwrap();
        assert_eq!(pair.x_local_tokens, 14);
        assert_eq!(pair.y_input_delta, 20);
        assert!(pair.partition_id.starts_with("opaque-session-"));
        assert!(
            !serde_json::to_string(&pair)
                .unwrap()
                .contains(&hash("first"))
        );
        let edited = engine.observe(
            sample(
                "edited",
                Some("next"),
                5,
                150,
                "edited answer",
                "user",
                "out",
            ),
            &tok,
            2,
        );
        assert_eq!(edited.reason.as_deref(), Some("shadow.response_mismatch"));
    }
    #[test]
    fn model_audit_shadow_unknown_cache_partial_and_partition_do_not_seed() {
        let tok = LocalTokenizer::byte_fixture();
        for change in 0..5 {
            let mut engine = ShadowEngine::new("s".into());
            let mut first = sample("first", None, 1, 100, "", "", "answer");
            match change {
                0 => first.cache_read_tokens = None,
                1 => first.cache_creation_tokens = Some(1),
                2 => first.complete = false,
                3 => first.failover = true,
                _ => first.input_tokens = None,
            }
            assert_eq!(engine.observe(first, &tok, 0).status, "excluded");
            assert!(engine.entries.iter().all(|entry| !entry.eligible));
        }
        let mut engine = ShadowEngine::new("s".into());
        engine.observe(sample("first", None, 1, 100, "", "", "answer"), &tok, 0);
        let mut next = sample("next", Some("first"), 3, 120, "answer", "user", "out");
        next.snapshot.partition_key = hash("changed system/auth/config");
        assert_eq!(engine.observe(next, &tok, 1).status, "baseline");
    }
    #[test]
    fn model_audit_shadow_ambiguity_ttl_capacity_and_no_plaintext_cache() {
        let tok = LocalTokenizer::byte_fixture();
        let mut engine = ShadowEngine::new("s".into());
        engine.observe(sample("first", None, 1, 100, "", "", "answer"), &tok, 0);
        engine.observe(sample("first", None, 1, 100, "", "", "answer"), &tok, 1);
        assert_eq!(
            engine
                .observe(
                    sample("next", Some("first"), 3, 120, "answer", "user", "out"),
                    &tok,
                    2
                )
                .reason
                .as_deref(),
            Some("shadow.ambiguous_prefix")
        );
        let mut engine = ShadowEngine::new("s".into());
        engine.observe(sample("first", None, 1, 100, "", "", "answer"), &tok, 0);
        assert_eq!(
            engine
                .observe(
                    sample("next", Some("first"), 3, 120, "answer", "user", "out"),
                    &tok,
                    TTL_MS + 1
                )
                .status,
            "baseline"
        );
        for index in 0..300 {
            engine.observe(
                sample(&format!("h{index}"), None, 1, 100, "", "", "a"),
                &tok,
                TTL_MS + 2 + index,
            );
        }
        assert_eq!(engine.entries.len(), CACHE_CAPACITY);
    }
    #[test]
    fn model_audit_shadow_unique_response_resolves_repeated_history() {
        let tok = LocalTokenizer::byte_fixture();
        let mut engine = ShadowEngine::new("s".into());
        engine.observe(sample("first", None, 1, 100, "", "", "answer A"), &tok, 0);
        engine.observe(sample("first", None, 1, 100, "", "", "answer B"), &tok, 1);
        assert_eq!(
            engine
                .observe(
                    sample("next", Some("first"), 3, 120, "answer A", "user", "out"),
                    &tok,
                    2
                )
                .status,
            "paired"
        );
    }
    #[test]
    fn model_audit_shadow_cached_or_partial_duplicate_invalidates_old_seed() {
        let tok = LocalTokenizer::byte_fixture();
        for kind in 0..4 {
            let mut engine = ShadowEngine::new("s".into());
            engine.observe(sample("first", None, 1, 100, "", "", "answer"), &tok, 0);
            let mut duplicate = sample("first", None, 1, 100, "", "", "answer");
            match kind {
                0 => duplicate.cache_read_tokens = Some(10),
                1 => duplicate.complete = false,
                2 => duplicate.failover = true,
                _ => duplicate.input_tokens = None,
            }
            assert_eq!(engine.observe(duplicate, &tok, 1).status, "excluded");
            let current = engine.observe(
                sample("next", Some("first"), 3, 120, "answer", "user", "out"),
                &tok,
                2,
            );
            assert_eq!(current.reason.as_deref(), Some("shadow.parent_ineligible"));
        }
    }
    fn pairs(slope: f64) -> Vec<ShadowPair> {
        (0..24)
            .map(|index| {
                let x = 20 + index * 5;
                ShadowPair {
                    partition_id: "opaque".into(),
                    x_local_tokens: x,
                    y_input_delta: (slope * x as f64 + 7.0).round() as i64,
                }
            })
            .collect()
    }
    #[test]
    fn model_audit_shadow_regression_honest_and_multiplier() {
        let honest = regress_shadow(&pairs(1.0));
        assert_eq!(honest.status, "consistent");
        assert_eq!(honest.intercept, Some(7.0));
        let scaled = regress_shadow(&pairs(1.2));
        assert_eq!(scaled.status, "suspicious");
        assert!((scaled.slope.unwrap() - 1.2).abs() < 1e-10);
        assert!(
            scaled
                .limitations
                .iter()
                .any(|line| line.contains("不能证明"))
        );
    }
    #[test]
    fn model_audit_shadow_regression_gates() {
        assert_eq!(regress_shadow(&pairs(1.2)[..2]).status, "untested");
        let mut constant = pairs(1.0);
        for pair in &mut constant {
            pair.x_local_tokens = 10;
        }
        assert!(
            regress_shadow(&constant)
                .gaps
                .contains(&"shadow.x_variance_insufficient".into())
        );
        let mut mixed = pairs(1.0);
        mixed[0].partition_id = "other".into();
        assert_eq!(regress_shadow(&mixed).status, "untested");
        let mut noisy = pairs(1.0);
        for (index, pair) in noisy.iter_mut().enumerate() {
            if index % 2 == 0 {
                pair.y_input_delta += 50;
            }
        }
        assert!(
            regress_shadow(&noisy)
                .gaps
                .contains(&"shadow.slope_uncertain".into())
        );
        let too_many = vec![pairs(1.0)[0].clone(); 10001];
        assert!(
            regress_shadow(&too_many)
                .gaps
                .contains(&"shadow.invalid_pairs".into())
        );
    }

    #[test]
    fn model_audit_shadow_adaptive_clean_samples_do_not_wait_for_twenty() {
        for numerator in [10, 12] {
            let data: Vec<_> = (0..5).map(|index| {
                let x = 40 + index * 40;
                ShadowPair { partition_id: "clean-small".into(), x_local_tokens: x,
                    y_input_delta: (x * numerator / 10 + 8) as i64 }
            }).collect();
            let report = regress_shadow(&data);
            assert_eq!(report.minimum_samples, 3);
            assert_eq!(report.status, if numerator == 10 { "consistent" } else { "suspicious" });
            assert!(report.residual_rmse.unwrap() < 1e-9);
        }
        let insufficient_diversity = regress_shadow(&pairs(1.2)[..3]);
        assert_eq!(insufficient_diversity.status, "untested");
        assert!(insufficient_diversity.gaps.contains(&"shadow.x_variance_insufficient".into()));
    }

    #[test]
    fn model_audit_shadow_adaptive_noise_requires_more_evidence() {
        let observations = |count, numerator| -> Vec<ShadowPair> {
            let noise = [15, -20, 10, -15, 20, -10];
            (0..count).map(|index| {
                let x = 40 + (index % 6) as u64 * 40;
                ShadowPair {
                    partition_id: "noise-fixture".into(), x_local_tokens: x,
                    y_input_delta: (x * numerator / 10 + 8) as i64 + noise[index % 6],
                }
            }).collect()
        };
        assert_eq!(regress_shadow(&observations(6, 12)).status, "untested");
        let sufficient = regress_shadow(&observations(24, 12));
        assert_eq!(sufficient.status, "suspicious");
        assert!(sufficient.residual_rmse.unwrap() > 10.0);
        assert!(sufficient.slope_ci_low.unwrap() > 1.05);
        for count in [6, 12, 24] {
            assert_ne!(regress_shadow(&observations(count, 10)).status, "suspicious");
        }
    }

    #[test]
    fn model_audit_shadow_adaptive_small_sample_interval_uses_actual_degrees_of_freedom() {
        let noise = [0, 5, -5, 5, -5];
        let observations: Vec<_> = noise.iter().enumerate().map(|(index, noise)| {
            let x = 40 + index as u64 * 40;
            ShadowPair { partition_id: "small-noise".into(), x_local_tokens: x,
                y_input_delta: (x * 12 / 10 + 8) as i64 + noise }
        }).collect();
        let report = regress_shadow(&observations);
        assert_eq!(report.status, "untested");
        assert!(report.slope_ci_low.unwrap() < 1.05);
        let correct_margin = report.slope.unwrap() - report.slope_ci_low.unwrap();
        // Reusing t(18) would falsely turn this uncertain small sample into a verdict.
        assert!(report.slope.unwrap() - correct_margin * 2.101 / slope_critical_value(5) > 1.05);
    }
}
