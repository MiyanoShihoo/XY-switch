//! Whitelisted export boundary for future user-initiated diagnostics; no automatic export.
use super::VerificationEvidence;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactedMetadata {
    pub observation_id: String,
    pub protocol: String,
    pub claim_consistency: String,
    pub identity_assurance: String,
    pub evidence_codes: Vec<String>,
    pub observed_at: i64,
    pub usage_status: String,
    pub total_tokens: Option<u64>,
    pub source: String,
    pub stream_completed: bool,
    pub http_status: Option<u16>,
}
impl From<&VerificationEvidence> for RedactedMetadata {
    fn from(e: &VerificationEvidence) -> Self {
        Self {
            observation_id: e.observation_id.clone(),
            protocol: e.protocol.clone(),
            claim_consistency: e.claim_consistency.clone(),
            identity_assurance: e.identity_assurance.clone(),
            evidence_codes: e.evidence_codes.clone(),
            observed_at: e.observed_at,
            usage_status: e.usage_status.clone(),
            total_tokens: e.total_tokens,
            source: e.source.clone(),
            stream_completed: e.stream_completed,
            http_status: e.http_status,
        }
    }
}
#[allow(dead_code)]
pub trait MetadataExporter: Send + Sync {
    /// Implementations receive no provider IDs, model aliases, URLs, keys, fingerprints or diagnostic IDs.
    fn write_redacted(&self, rows: &[RedactedMetadata]) -> Result<Vec<u8>, String>;
}
#[allow(dead_code)]
pub struct JsonMetadataExporter;
impl MetadataExporter for JsonMetadataExporter {
    fn write_redacted(&self, rows: &[RedactedMetadata]) -> Result<Vec<u8>, String> {
        serde_json::to_vec(rows).map_err(|_| "无法导出脱敏元数据".into())
    }
}
#[allow(dead_code)]
pub struct CsvMetadataExporter;
impl MetadataExporter for CsvMetadataExporter {
    fn write_redacted(&self, rows: &[RedactedMetadata]) -> Result<Vec<u8>, String> {
        fn field(s: &str) -> String {
            let prefix = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
                "'"
            } else {
                ""
            };
            format!("\"{}{}\"", prefix, s.replace('"', "\"\""))
        }
        let mut out = String::from(
            "observationId,protocol,claimConsistency,identityAssurance,evidenceCodes,observedAt,usageStatus,totalTokens,source,streamCompleted,httpStatus\r\n",
        );
        for e in rows {
            let row = [
                e.observation_id.clone(),
                e.protocol.clone(),
                e.claim_consistency.clone(),
                e.identity_assurance.clone(),
                e.evidence_codes.join(";"),
                e.observed_at.to_string(),
                e.usage_status.clone(),
                e.total_tokens.map(|n| n.to_string()).unwrap_or_default(),
                e.source.clone(),
                e.stream_completed.to_string(),
                e.http_status.map(|n| n.to_string()).unwrap_or_default(),
            ];
            out.push_str(&row.iter().map(|s| field(s)).collect::<Vec<_>>().join(","));
            out.push_str("\r\n");
        }
        Ok(out.into_bytes())
    }
}
