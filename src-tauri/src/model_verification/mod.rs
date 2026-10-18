//! Opt-in metadata observation and strictly budgeted, separately opt-in rechecks.
mod engine;
#[cfg(test)]
mod decision_tests;
pub mod export;
mod runtime;
mod store;
#[cfg(test)]
mod tests;
mod types;
pub use engine::usage_total;
pub use runtime::{VerificationRuntime, now_ms};
pub use types::*;

use crate::{credential_store, database::Database, proxy};
use tauri::State;

#[tauri::command]
pub fn set_model_verification_settings(
    settings: VerificationSettings,
    db: State<'_, Database>,
) -> Result<VerificationSettings, String> {
    db.verification.set_settings(settings)
}
#[tauri::command]
pub fn model_verification_status(
    provider_id: String,
    model: Option<String>,
    db: State<'_, Database>,
) -> Result<VerificationStatus, String> {
    let provider = db.get_provider(&provider_id)?.ok_or("供应商不存在")?;
    let model = model.as_deref().unwrap_or(&provider.model);
    let mut context_error = None;
    let capture = if db.verification.settings()?.enabled {
        let key = match credential_store::get_api_key(&provider.id) {
            Ok(key) => key.map(zeroize::Zeroizing::new),
            Err(_) => {
                context_error = Some("无法读取当前凭据；路由证据不可用，仍可关闭验证。".into());
                None
            }
        };
        key.and_then(|key| {
            proxy::response_model_protocol(&provider)
                .ok()
                .and_then(|protocol| db.verification.capture(&provider, &key, protocol, model))
        })
    } else {
        None
    };
    let mut status = db
        .verification
        .status(&provider.id, model, capture.as_ref())?;
    status.context_error = context_error;
    Ok(status)
}
#[tauri::command]
pub fn confirm_model_verification_cost(
    provider_id: String,
    model: String,
    confirmed: bool,
    db: State<'_, Database>,
) -> Result<(), String> {
    if !db.verification.settings()?.enabled {
        return Err("请先开启模型验证".into());
    }
    let provider = db.get_provider(&provider_id)?.ok_or("供应商不存在")?;
    let key = zeroize::Zeroizing::new(
        credential_store::get_api_key(&provider.id)?.ok_or("尚未保存 API Key")?,
    );
    let capture = db
        .verification
        .capture(
            &provider,
            &key,
            proxy::response_model_protocol(&provider)?,
            &model,
        )
        .ok_or("请先开启验证并启用此路由")?;
    // Confirmation changes configuration only; it never sends an inference request.
    db.verification.approve(&capture, confirmed)
}
