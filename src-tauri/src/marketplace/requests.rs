use super::{MarketplaceError, WithdrawalReason, privacy};
use crate::plugins::PluginPackage;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubmitRequest {
    pub package: PluginPackage,
    pub replaces_submission_id: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VersionRequest {
    pub version: String,
    pub digest: String,
}

impl VersionRequest {
    pub(crate) fn validate(&self) -> Result<(), MarketplaceError> {
        crate::plugins::validation::parse_version(&self.version)
            .map_err(|_| MarketplaceError::InvalidRequest)?;
        crate::plugins::validation::validate_hex(&self.digest, 32)
            .map_err(|_| MarketplaceError::InvalidRequest)
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision { Approve, RequestChanges, Reject }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionRequest {
    pub version: String,
    pub digest: String,
    pub decision: ReviewDecision,
    #[serde(default)]
    pub note: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WithdrawRequest {
    pub version: String,
    pub digest: String,
    pub reason: WithdrawalReason,
    #[serde(default)]
    pub note: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReportRequest {
    pub reason: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ResolveReportRequest {
    pub resolution: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ListQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub status: Option<String>,
}

#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CatalogQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub client: Option<String>,
    pub kind: Option<String>,
    pub q: Option<String>,
}

pub(crate) fn page_bounds(limit: Option<u32>, offset: Option<u32>) -> Result<(u32, u32), MarketplaceError> {
    let limit = limit.unwrap_or(20);
    let offset = offset.unwrap_or(0);
    if !(1..=100).contains(&limit) || offset > 100_000 { return Err(MarketplaceError::InvalidRequest); }
    Ok((limit, offset))
}

impl CatalogQuery {
    pub(crate) fn validate(&self) -> Result<(), MarketplaceError> {
        page_bounds(self.limit, self.offset)?;
        if self.kind.as_deref().is_some_and(|kind| !matches!(kind, "mcp" | "skill" | "provider"))
            || self.client.as_deref().is_some_and(|client| !crate::plugins::SUPPORTED_CLIENTS.contains(&client)) {
            return Err(MarketplaceError::InvalidRequest);
        }
        if let Some(query) = &self.q {
            if query.len() > 160 { return Err(MarketplaceError::InvalidRequest); }
            privacy::inspect_text(query)?;
        }
        Ok(())
    }
}
