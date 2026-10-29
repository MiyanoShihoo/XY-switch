use crate::{
    database::{Provider, ProviderAdvancedConfig, ProviderModelMapping},
    model_pricing::PricingInput,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Favorite {
    pub provider_id: String,
    pub session_id: String,
}
impl Favorite {
    pub fn validate(&self) -> Result<(), String> {
        text(&self.provider_id, 160)?;
        text(&self.session_id, 512)?;
        if self.provider_id.contains(['/', '\\']) || self.session_id.contains(['/', '\\']) {
            return Err("Favorites contain identifiers only, not paths".into());
        }
        Ok(())
    }
    pub fn key(&self) -> String {
        serde_json::to_string(&(&self.provider_id, &self.session_id)).unwrap()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Mapping {
    pub model: String,
    pub display_name: String,
    pub context_window: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SyncProvider {
    pub id: String,
    pub name: String,
    pub provider_type: String,
    pub app_group: String,
    pub endpoint: String,
    pub model: String,
    pub upstream_format: String,
    pub is_full_url: bool,
    pub model_mappings: Vec<Mapping>,
}
impl SyncProvider {
    pub fn from_local(provider: &Provider, id: &str) -> Self {
        Self {
            id: id.into(),
            name: provider.name.clone(),
            provider_type: provider.provider_type.clone(),
            app_group: provider.app_group.clone(),
            endpoint: provider.endpoint.clone(),
            model: provider.model.clone(),
            upstream_format: provider.advanced.upstream_format.clone(),
            is_full_url: provider.advanced.is_full_url,
            model_mappings: provider
                .advanced
                .model_mappings
                .iter()
                .map(|m| Mapping {
                    model: m.model.clone(),
                    display_name: m.display_name.clone(),
                    context_window: m.context_window,
                })
                .collect(),
        }
    }
    pub fn target_changed(&self, local: &Self) -> bool {
        self.endpoint != local.endpoint
            || self.provider_type != local.provider_type
            || self.upstream_format != local.upstream_format
            || self.is_full_url != local.is_full_url
    }
    pub fn apply_fields(&self, provider: &mut Provider) {
        provider.name = self.name.clone();
        provider.provider_type = self.provider_type.clone();
        provider.app_group = self.app_group.clone();
        provider.endpoint = self.endpoint.clone();
        provider.model = self.model.clone();
        provider.advanced.upstream_format = self.upstream_format.clone();
        provider.advanced.is_full_url = self.is_full_url;
        provider.advanced.model_mappings = self
            .model_mappings
            .iter()
            .map(|m| ProviderModelMapping {
                model: m.model.clone(),
                display_name: m.display_name.clone(),
                context_window: m.context_window,
            })
            .collect();
    }
    pub fn new_disabled(&self) -> Provider {
        let mut provider = Provider {
            id: uuid::Uuid::new_v4().to_string(),
            name: String::new(),
            provider_type: String::new(),
            endpoint: String::new(),
            model: String::new(),
            app_group: "Codex".into(),
            enabled: false,
            sort_order: 0,
            advanced: ProviderAdvancedConfig::default(),
        };
        self.apply_fields(&mut provider);
        provider
    }
    pub fn validate(&self) -> Result<(), String> {
        if uuid::Uuid::parse_str(&self.id)
            .map(|id| id.to_string() != self.id)
            .unwrap_or(true)
        {
            return Err("Invalid sync identity".into());
        }
        text(&self.name, 256)?;
        text(&self.provider_type, 64)?;
        text(&self.model, 256)?;
        if !matches!(self.app_group.as_str(), "Codex" | "Claude" | "Gemini") {
            return Err("Invalid sync client group".into());
        }
        if self.provider_type == "local_usage" || self.endpoint.len() > 2048 {
            return Err("Unsupported sync provider".into());
        }
        text(&self.endpoint, 2048)?;
        let url = reqwest::Url::parse(&self.endpoint).map_err(|_| "Invalid provider endpoint")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(
                "Sync endpoints cannot contain credentials, query strings or fragments".into(),
            );
        }
        if self.model_mappings.len() > 200 {
            return Err("Too many sync model mappings".into());
        }
        for mapping in &self.model_mappings {
            text(&mapping.model, 256)?;
            if !mapping.display_name.is_empty() {
                text(&mapping.display_name, 256)?;
            }
        }
        crate::database::validate_config_records(&[self.new_disabled()], &[])
            .map_err(|_| "Invalid sync model mapping or upstream format")?;
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Payload {
    pub schema: u32,
    pub providers: Vec<SyncProvider>,
    pub favorites: Vec<Favorite>,
    pub prices: Vec<PricingInput>,
}
impl Payload {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != 1
            || self.providers.len() > 1000
            || self.favorites.len() > 10000
            || self.prices.len() > crate::model_pricing::MAX_PRICES
        {
            return Err("Unsupported sync schema or record count".into());
        }
        let mut ids = BTreeSet::new();
        for provider in &self.providers {
            provider.validate()?;
            if !ids.insert(provider.id.clone()) {
                return Err("Duplicate sync provider".into());
            }
        }
        ids.clear();
        for favorite in &self.favorites {
            favorite.validate()?;
            if !ids.insert(favorite.key()) {
                return Err("Duplicate sync favorite".into());
            }
        }
        ids.clear();
        for price in &self.prices {
            price.validate()?;
            if !ids.insert(price.id.clone()) {
                return Err("Duplicate sync price".into());
            }
        }
        if serde_json::to_vec(self)
            .map_err(|_| "Invalid sync data")?
            .len()
            > super::crypto::MAX_PLAINTEXT
        {
            return Err("Sync payload exceeds 8 MiB".into());
        }
        Ok(())
    }
}
fn text(value: &str, max: usize) -> Result<(), String> {
    if value.is_empty()
        || value.trim() != value
        || value.len() > max
        || value.chars().any(char::is_control)
    {
        return Err("Invalid sync identifier or label".into());
    }
    Ok(())
}
