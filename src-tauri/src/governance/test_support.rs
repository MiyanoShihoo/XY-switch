use super::*;
use crate::database::{Provider, ProviderAdvancedConfig};
pub fn database() -> Database {
    let db = Database::open(std::path::Path::new(":memory:")).unwrap();
    for provider in db.list_providers().unwrap() { db.delete_provider(&provider.id).unwrap(); }
    db
}
pub fn provider(endpoint: &str) -> Provider {
    Provider { id: "gov-test".into(), name: "Governance test".into(), provider_type: "openai_compatible".into(), endpoint: endpoint.into(), model: "gov-model".into(), app_group: "Codex".into(), enabled: true, sort_order: 0, advanced: ProviderAdvancedConfig { upstream_format: "openai_chat".into(), ..Default::default() } }
}
pub fn budget(db: &Database) -> budget::BudgetPolicy {
    let policy = budget::BudgetPolicy { enabled: true, daily_tokens: 100_000, daily_microusd: 1_000_000, input_reservation: 1024, max_output_tokens: 64 };
    save(db, budget::KEY, &policy).unwrap();
    let c = db.conn(); let c = c.lock().unwrap();
    crate::model_pricing::save_on(&c, &crate::model_pricing::PricingInput { id: "gov-model".into(), input_per_million: 1.0, output_per_million: 2.0, cache_read_per_million: None, cache_creation_per_million: None }).unwrap();
    policy
}
