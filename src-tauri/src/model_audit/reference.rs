use super::tokenizer::{Tokenizer, read_bounded};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const CORPUS_ID: &str = "xy-local-differential-v1";
// These are inputs, never an answer table. Counts and Unicode character lengths
// are computed from the user-supplied tokenizer on every generation.
const CORPUS: &[(&str, &str)] = &[
    ("digits", "3141592653589793238462643383279"),
    ("han_common", "人工智能正在改变世界我们需要可靠的工具"),
    ("han_rare", "龘靐齉爨麤灪籲龗魑魅魍魉"),
    ("spaces", "                                "),
    ("latin", "The quick brown fox jumps over the lazy dog."),
    (
        "code",
        "fn main() { let value = 42; println!(\"{}\", value); }",
    ),
    ("cyrillic", "Проверка точности токенизации текста"),
    ("arabic", "اختبار دقة تقسيم النص إلى رموز"),
    ("thai", "ภาษาไทยมีเครื่องหมายประกอบและสระ"),
    ("emoji", "👨‍👩‍👧‍👦👍🏽🙂🚀🌍"),
    ("mixed_case", "HelloWorld XMLHttpRequest OpenAI HTTPServer"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceRequest {
    pub tokenizer_path: String,
    pub model: String,
    #[serde(default)]
    pub pretokenizer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FingerprintGroup {
    pub id: String,
    pub chars: u64,
    pub d1: i64,
    pub d2: i64,
    pub stable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceFingerprint {
    pub schema_version: u32,
    pub corpus_id: String,
    pub model: String,
    pub source_sha256: String,
    pub tokenizer_kind: String,
    pub pretokenizer: String,
    pub authority: String,
    pub groups: Vec<FingerprintGroup>,
    pub limitations: Vec<String>,
}

pub fn generate_reference(request: ReferenceRequest) -> Result<ReferenceFingerprint, String> {
    if request.model.trim().is_empty() || request.model.len() > 256 {
        return Err("Provide a nonempty model label of at most 256 bytes".into());
    }
    let tokenizer = Tokenizer::load(&request.tokenizer_path, request.pretokenizer.as_deref())?;
    let filler = "Local offline tokenizer reference. Same prefix for all three rungs.";
    let t0 = tokenizer.count(filler)? as i64;
    let mut groups = Vec::new();
    for (id, text) in CORPUS {
        // Both increments append the exact same fragment, separator included.
        let fragment = format!("\n{text}");
        let first = format!("{filler}{fragment}");
        let second = format!("{first}{fragment}");
        let t1 = tokenizer.count(&first)? as i64;
        let t2 = tokenizer.count(&second)? as i64;
        let d1 = t1 - t0;
        let d2 = t2 - t1;
        groups.push(FingerprintGroup {
            id: (*id).into(),
            chars: text.chars().count() as u64,
            d1,
            d2,
            stable: d1 > 0 && d1 == d2,
        });
    }
    let pretokenizer = tokenizer.pretokenizer().to_owned();
    Ok(ReferenceFingerprint {
        schema_version: 1,
        corpus_id: CORPUS_ID.into(),
        model: request.model,
        source_sha256: tokenizer.sha256,
        tokenizer_kind: tokenizer.kind,
        pretokenizer,
        authority: "tokenizer_file".into(),
        groups,
        limitations: vec![
            "本地文件 SHA256 只证明文件内容，不证明下载来源或模型身份。".into(),
            "手写预分词状态机使用 Unicode 类别；仍需与官方分词器或直连实测逐组核对。".into(),
            "相同词表可由多个模型共用；全组一致只能定位词表等价类。".into(),
            "指纹仅包含结构统计；没有保存业务请求或响应正文，也没有发送网络请求。".into(),
        ],
    })
}

pub fn load_reference(path: &str) -> Result<ReferenceFingerprint, String> {
    let bytes = read_bounded(path, 2 * 1024 * 1024)?;
    let fingerprint: ReferenceFingerprint = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Invalid reference JSON: {error}"))?;
    validate(&fingerprint)?;
    Ok(fingerprint)
}

fn validate(fingerprint: &ReferenceFingerprint) -> Result<(), String> {
    if fingerprint.schema_version != 1 || fingerprint.corpus_id != CORPUS_ID {
        return Err(
            "Unsupported reference schema/corpus; regenerate with this corpus version".into(),
        );
    }
    if fingerprint.model.trim().is_empty() || fingerprint.model.len() > 256 {
        return Err("Reference model label is missing or too long".into());
    }
    if fingerprint.source_sha256.len() != 64
        || !fingerprint
            .source_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("Reference must identify its source with a SHA256 hex digest".into());
    }
    if !matches!(
        fingerprint.authority.as_str(),
        "tokenizer_file" | "measured"
    ) {
        return Err("Unsupported reference authority (tokenizer_file or measured required)".into());
    }
    let corpus: BTreeMap<_, _> = CORPUS
        .iter()
        .map(|(id, text)| (*id, text.chars().count() as u64))
        .collect();
    let mut seen = BTreeSet::new();
    if fingerprint.groups.len() > CORPUS.len() {
        return Err("Too many reference groups".into());
    }
    for group in &fingerprint.groups {
        if !seen.insert(&group.id) || corpus.get(group.id.as_str()) != Some(&group.chars) {
            return Err("Unknown/duplicate corpus group or incorrect character count".into());
        }
        if group.d1 < 0 || group.d2 < 0 || (group.stable && (group.d1 == 0 || group.d1 != group.d2))
        {
            return Err(
                "A stable group must contain two observed, equal positive differences".into(),
            );
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RankingConfig {
    pub p_hit: f64,
    pub outside_edge: f64,
    pub family_cap: f64,
    pub minimum_groups: usize,
}
impl Default for RankingConfig {
    fn default() -> Self {
        Self {
            p_hit: 0.97,
            outside_edge: 0.1,
            family_cap: 3.0,
            minimum_groups: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankingRequest {
    pub observation: ReferenceFingerprint,
    pub candidates: Vec<ReferenceFingerprint>,
    #[serde(default)]
    pub config: Option<RankingConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateProbability {
    pub model: String,
    pub probability: f64,
    pub comparable: usize,
    pub matched: usize,
    pub outside: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankingReport {
    pub status: String,
    pub candidates: Vec<CandidateProbability>,
    pub compared_groups: Vec<String>,
    pub limitations: Vec<String>,
}
pub type RankingResult = RankingReport;

fn soft_cap(sum: f64, cap: f64) -> f64 {
    cap * (sum / cap).tanh()
}

fn validate_config(config: &RankingConfig) -> Result<(), String> {
    if !config.p_hit.is_finite()
        || !(0.5 < config.p_hit && config.p_hit < 1.0)
        || !config.outside_edge.is_finite()
        || !config.family_cap.is_finite()
        || config.family_cap <= 0.0
        || config.minimum_groups == 0
        || config.minimum_groups > CORPUS.len()
    {
        return Err("Invalid ranking configuration: finite pHit in (0.5,1), positive familyCap, minimumGroups 1..11 required".into());
    }
    let one_miss = ((1.0 - config.p_hit) / config.p_hit).ln();
    // Applying the same family cap changes the lower bound. Validate the
    // stronger condition too, preserving 'one mismatch => outside ranks first'.
    let lower = soft_cap(one_miss, config.family_cap).exp();
    if !(lower < config.outside_edge && config.outside_edge < 1.0) {
        return Err(format!(
            "outsideEdge must be in ({lower:.6}, 1) after the tokenizer-family soft cap"
        ));
    }
    Ok(())
}

pub fn rank_references(request: RankingRequest) -> Result<RankingReport, String> {
    let config = request.config.unwrap_or_default();
    validate_config(&config)?;
    validate(&request.observation)?;
    if request.candidates.len() > 128 {
        return Err("At most 128 reference candidates are supported".into());
    }
    let mut candidate_names = BTreeSet::new();
    for candidate in &request.candidates {
        validate(candidate)?;
        if !candidate_names.insert(&candidate.model) {
            return Err("Duplicate candidate model labels are not allowed".into());
        }
    }
    let stable = |fingerprint: &ReferenceFingerprint| -> BTreeMap<String, i64> {
        fingerprint
            .groups
            .iter()
            .filter(|group| group.stable && group.d1 == group.d2 && group.d1 > 0)
            .map(|group| (group.id.clone(), group.d1))
            .collect()
    };
    let observed = stable(&request.observation);
    let references: Vec<_> = request.candidates.iter().map(stable).collect();
    // A shared comparison set avoids rewarding a candidate for absent groups.
    let compared_groups: Vec<_> = observed
        .keys()
        .filter(|group| {
            references
                .iter()
                .all(|candidate| candidate.contains_key(*group))
        })
        .cloned()
        .collect();
    let mut report = RankingReport {
        status: "untested".into(),
        candidates: Vec::new(),
        compared_groups,
        limitations: vec![
            "概率只在当前候选、等权先验和似然参数下成立，不是模型身份的客观概率。".into(),
            "共享词表的型号无法区分；全组一致只能定位词表等价类，库外候选始终保留。".into(),
            "相关分词组按单一证据族使用 c*tanh(sum/c) 软封顶，避免重复累加造成过度确信。".into(),
            "仅比较所有候选共同具备的稳定分词组；文件指纹未经官方核对，不能据此认定欺诈。".into(),
        ],
    };
    if references.is_empty() || report.compared_groups.len() < config.minimum_groups {
        report.limitations.insert(
            0,
            "未测到：候选库为空或共同稳定分词组不足；这不等于没问题。".into(),
        );
        return Ok(report);
    }
    let n = report.compared_groups.len();
    let one_miss = ((1.0 - config.p_hit) / config.p_hit).ln();
    let mut weights = Vec::new();
    for (candidate, values) in request.candidates.iter().zip(&references) {
        let matched = report
            .compared_groups
            .iter()
            .filter(|group| observed.get(*group) == values.get(*group))
            .count();
        // Subtracting n*ln(p_hit) from all log likelihoods is a common offset;
        // outside is p_hit^n*outside_edge before this normalization.
        let weight = soft_cap((n - matched) as f64 * one_miss, config.family_cap).exp();
        weights.push(weight);
        report.candidates.push(CandidateProbability {
            model: candidate.model.clone(),
            probability: weight,
            comparable: n,
            matched,
            outside: false,
        });
    }
    weights.push(config.outside_edge);
    report.candidates.push(CandidateProbability {
        model: "（库外：不属于任何库内候选词表）".into(),
        probability: config.outside_edge,
        comparable: n,
        matched: 0,
        outside: true,
    });
    let sum: f64 = weights.iter().sum();
    for candidate in &mut report.candidates {
        candidate.probability /= sum;
    }
    report.candidates.sort_by(|left, right| {
        right
            .probability
            .total_cmp(&left.probability)
            .then(left.model.cmp(&right.model))
    });
    report.status = "ranked".into();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fingerprint(model: &str) -> ReferenceFingerprint {
        ReferenceFingerprint {
            schema_version: 1,
            corpus_id: CORPUS_ID.into(),
            model: model.into(),
            source_sha256: "a".repeat(64),
            tokenizer_kind: "test".into(),
            pretokenizer: "gpt2".into(),
            authority: "measured".into(),
            groups: CORPUS
                .iter()
                .map(|(id, text)| FingerprintGroup {
                    id: (*id).into(),
                    chars: text.chars().count() as u64,
                    d1: 8,
                    d2: 8,
                    stable: true,
                })
                .collect(),
            limitations: Vec::new(),
        }
    }
    #[test]
    fn model_audit_ranking_equivalence_and_outside() {
        let observation = fingerprint("observed");
        let result = rank_references(RankingRequest {
            observation: observation.clone(),
            candidates: vec![fingerprint("large"), fingerprint("small")],
            config: None,
        })
        .unwrap();
        assert_eq!(result.status, "ranked");
        assert_eq!(
            result.candidates[0].probability,
            result.candidates[1].probability
        );
        assert!(
            result
                .candidates
                .iter()
                .any(|candidate| candidate.outside && candidate.probability > 0.0)
        );
        assert!(
            (result
                .candidates
                .iter()
                .map(|candidate| candidate.probability)
                .sum::<f64>()
                - 1.0)
                .abs()
                < 1e-12
        );
        let mut wrong = fingerprint("wrong");
        wrong.groups[0].d1 += 1;
        wrong.groups[0].d2 += 1;
        let result = rank_references(RankingRequest {
            observation,
            candidates: vec![wrong],
            config: None,
        })
        .unwrap();
        assert!(result.candidates[0].outside);
    }
    #[test]
    fn model_audit_ranking_missing_and_unstable_are_untested() {
        let mut candidate = fingerprint("sparse");
        candidate.groups.truncate(2);
        let result = rank_references(RankingRequest {
            observation: fingerprint("observed"),
            candidates: vec![candidate],
            config: None,
        })
        .unwrap();
        assert_eq!(result.status, "untested");
        assert!(result.candidates.is_empty());
        let mut invalid = fingerprint("unstable");
        invalid.groups[0].d2 += 1;
        assert!(validate(&invalid).is_err());
    }
    #[test]
    fn model_audit_family_soft_cap_and_degenerate_config() {
        assert!((soft_cap(-20.0, 3.0) + 3.0).abs() < 0.001);
        for value in [0.0, 1.0, f64::NAN, f64::INFINITY] {
            assert!(
                validate_config(&RankingConfig {
                    outside_edge: value,
                    ..Default::default()
                })
                .is_err()
            );
        }
        assert!(
            validate_config(&RankingConfig {
                family_cap: 0.01,
                ..Default::default()
            })
            .is_err()
        );
        assert!(validate_config(&RankingConfig::default()).is_ok());
    }
    #[test]
    fn model_audit_reference_generation_load_and_hash_roundtrip() {
        use std::{
            fs,
            time::{SystemTime, UNIX_EPOCH},
        };
        let directory = std::env::temp_dir().join(format!(
            "xy-audit-core-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        // Explicit miniature byte vocabulary: no downloaded/hardcoded answers.
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let rows = (0u16..=255)
            .map(|byte| {
                format!(
                    "{}{}== {}\n",
                    alphabet[(byte >> 2) as usize] as char,
                    alphabet[((byte & 3) << 4) as usize] as char,
                    byte
                )
            })
            .collect::<String>();
        let tokenizer_path = directory.join("bytes.tiktoken");
        fs::write(&tokenizer_path, rows.as_bytes()).unwrap();
        let generated = generate_reference(ReferenceRequest {
            tokenizer_path: tokenizer_path.to_string_lossy().into_owned(),
            model: "byte-fixture".into(),
            pretokenizer: Some("cl100k".into()),
        })
        .unwrap();
        assert_eq!(generated.groups.len(), CORPUS.len());
        for group in &generated.groups {
            let text = CORPUS.iter().find(|(id, _)| *id == group.id).unwrap().1;
            assert_eq!(group.d1, (text.len() + 1) as i64);
            assert_eq!(group.d1, group.d2);
            assert!(group.stable);
        }
        use sha2::{Digest, Sha256};
        assert_eq!(
            generated.source_sha256,
            format!("{:x}", Sha256::digest(rows.as_bytes()))
        );
        let reference_path = directory.join("reference.json");
        fs::write(&reference_path, serde_json::to_vec(&generated).unwrap()).unwrap();
        assert_eq!(
            load_reference(&reference_path.to_string_lossy())
                .unwrap()
                .source_sha256,
            generated.source_sha256
        );
        // Exercise the actual HuggingFace byte alphabet and file loader too.
        // With no merges, every byte is a token under either container format.
        let mut next = 256u32;
        let mut vocab = serde_json::Map::new();
        for byte in 0u16..=255 {
            let codepoint = if matches!(byte, 33..=126 | 161..=172 | 174..=255) {
                u32::from(byte)
            } else {
                let value = next;
                next += 1;
                value
            };
            vocab.insert(
                char::from_u32(codepoint).unwrap().to_string(),
                serde_json::json!(byte),
            );
        }
        let hf_path = directory.join("tokenizer.json");
        fs::write(
            &hf_path,
            serde_json::to_vec(&serde_json::json!({
                "normalizer":null,
                "pre_tokenizer":{"type":"ByteLevel","add_prefix_space":false,"use_regex":true},
                "model":{"type":"BPE","vocab":vocab,"merges":[]}
            }))
            .unwrap(),
        )
        .unwrap();
        let hf = generate_reference(ReferenceRequest {
            tokenizer_path: hf_path.to_string_lossy().into_owned(),
            model: "hf-byte-fixture".into(),
            pretokenizer: None,
        })
        .unwrap();
        assert_eq!(hf.tokenizer_kind, "hf_bpe");
        assert_eq!(
            hf.groups.iter().map(|group| group.d1).collect::<Vec<_>>(),
            generated
                .groups
                .iter()
                .map(|group| group.d1)
                .collect::<Vec<_>>()
        );
        fs::remove_file(tokenizer_path).unwrap();
        fs::remove_file(reference_path).unwrap();
        fs::remove_file(hf_path).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
