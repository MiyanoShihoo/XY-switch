mod engine;
mod headers;
mod ledger;
mod store;
mod validation;
pub mod types;

pub use store::{list, recorded_events, save, set_enabled, simulate};

pub fn is_virtual_model(model: &str) -> bool {
    model.trim().starts_with(types::MODEL_PREFIX)
}

#[cfg(test)]
mod tests;
