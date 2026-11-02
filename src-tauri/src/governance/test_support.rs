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
    let policy = budget::BudgetPolicy { enabled: true, daily_tokens: 100_000, daily_microusd: 1_000_000, input_reservation: 1024, max_output_tokens: 64, ..Default::default() };
    save(db, budget::KEY, &policy).unwrap();
    let c = db.conn(); let c = c.lock().unwrap();
    crate::model_pricing::save_on(&c, &crate::model_pricing::PricingInput { id: "gov-model".into(), input_per_million: 1.0, output_per_million: 2.0, cache_read_per_million: None, cache_creation_per_million: None }).unwrap();
    policy
}

pub fn native_codex_requests() -> Vec<serde_json::Value> {
    serde_json::from_str(include_str!("fixtures/codex-native-requests.json")).unwrap()
}

pub fn native_budget(
    db: &Database,
    model: &str,
    input_reservation: i64,
    max_output_tokens: i64,
) -> budget::BudgetPolicy {
    let policy = budget::BudgetPolicy {
        enabled: true,
        daily_tokens: 2_000_000,
        daily_microusd: 4_000_000,
        input_reservation,
        max_output_tokens,
        ..Default::default()
    };
    save(db, budget::KEY, &policy).unwrap();
    crate::model_pricing::save_on(
        &db.conn().lock().unwrap(),
        &crate::model_pricing::PricingInput {
            id: model.into(),
            input_per_million: 1.0,
            output_per_million: 2.0,
            cache_read_per_million: None,
            cache_creation_per_million: None,
        },
    )
    .unwrap();
    policy
}
