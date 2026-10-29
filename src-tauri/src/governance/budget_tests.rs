use super::*;
use crate::governance::test_support as fixture;
fn body() -> Value { json!({"model":"gov-model","messages":[{"role":"user","content":"OK"}],"max_tokens":16}) }
#[test]
fn concurrent_admission_never_exceeds_tokens_or_cost() {
    for use_cost in [false, true] {
        let db = fixture::database(); let mut p = fixture::budget(&db);
        if use_cost { p.daily_microusd = 3 * (1024 + 16 * 2); } else { p.daily_tokens = 3 * (1024 + 16); }
        save(&db, KEY, &p).unwrap();
        let handles: Vec<_> = (0..32).map(|_| { let db = db.clone(); std::thread::spawn(move || reserve(&db, &fixture::provider("http://127.0.0.1:1"), &mut body(), "chat", "proxy").is_ok()) }).collect();
        assert_eq!(handles.into_iter().filter_map(|h| h.join().ok()).filter(|ok| *ok).count(), 3);
        let status = status(&db).unwrap();
        assert_eq!((status.attempts, status.reserved_tokens, status.reserved_microusd), (3, 3120, 3168));
    }
}
#[test]
fn reservations_survive_reopen_and_policy_changes() {
    let path = std::env::temp_dir().join(format!("xy-budget-{}.db", uuid::Uuid::new_v4()));
    {
        let db = Database::open(&path).unwrap(); let mut p = fixture::budget(&db);
        reserve(&db, &fixture::provider("http://127.0.0.1:1"), &mut body(), "chat", "proxy").unwrap();
        p.enabled = false; save(&db, KEY, &p).unwrap();
        assert_eq!(status(&db).unwrap().attempts, 1);
    }
    {
        let db = Database::open(&path).unwrap(); let mut p = fixture::budget(&db);
        p.daily_tokens = 1040; save(&db, KEY, &p).unwrap();
        assert!(reserve(&db, &fixture::provider("http://127.0.0.1:1"), &mut body(), "chat", "proxy").unwrap_err().contains("hard_token_budget_exhausted"));
        assert_eq!(status(&db).unwrap().attempts, 1);
    }
    std::fs::remove_file(path).unwrap();
}
#[test]
fn exact_custom_price_is_mandatory_and_cache_uses_highest_rate() {
    let db = fixture::database(); fixture::budget(&db);
    let p = fixture::provider("http://127.0.0.1:1");
    let mut missing = body(); missing["model"] = json!("gov-model-2026");
    assert!(reserve(&db, &p, &mut missing, "chat", "proxy").unwrap_err().contains("exact_custom_price"));
    { let c=db.conn(); let c=c.lock().unwrap(); c.execute("UPDATE model_pricing SET source='models.dev' WHERE id='gov-model'",[]).unwrap(); }
    assert!(reserve(&db, &p, &mut body(), "chat", "proxy").unwrap_err().contains("exact_custom_price"));
    { let c=db.conn(); let c=c.lock().unwrap(); c.execute("UPDATE model_pricing SET source='custom',cache_creation_per_million=4.0001 WHERE id='gov-model'",[]).unwrap(); }
    reserve(&db, &p, &mut body(), "chat", "proxy").unwrap();
    assert_eq!(status(&db).unwrap().reserved_microusd, 4129);
}
#[test]
fn payload_caps_all_supported_protocols_and_rejects_unbounded_inputs() {
    let p = BudgetPolicy { max_output_tokens: 32, ..Default::default() };
    for (format,key) in [("chat","max_completion_tokens"),("responses","max_output_tokens"),("anthropic","max_tokens")] {
        // Only protocol-native fields are accepted; use each protocol's own cap.
        let mut payload = json!({"model":"x",key:100});
        assert_eq!(bound_payload(&mut payload, format, &p).unwrap(),32);
        assert_eq!(payload[key],32);
    }
    for extra in [json!({"n":2}), json!({"service_tier":"priority"}), json!({"previous_response_id":"x"}), json!({"messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"https://example.invalid"}}]}]}), json!({"tools":[{"type":"web_search"}]}), json!({"max_tokens":0}), json!({"unknown_billable_feature":true}), json!({"extra_body":{"max_tokens":10000}})] {
        let mut payload = body(); payload.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        assert!(bound_payload(&mut payload, "chat", &p).is_err(), "{extra}");
    }
    let mut oversized = body(); oversized["messages"] = json!([{"role":"user","content":"x".repeat(5000)}]);
    assert!(bound_payload(&mut oversized, "chat", &p).unwrap_err().contains("input_reservation"));
    let mut thinking=json!({"model":"x","messages":[],"max_tokens":32,"thinking":{"type":"enabled","budget_tokens":32}});
    assert!(bound_payload(&mut thinking,"anthropic",&p).is_err());
    assert!(bound_payload(&mut json!({"contents":[]}),"gemini",&p).is_err());
}
#[test]
fn invalid_policy_and_disabled_synthetic_gate_fail_closed_without_reservation() {
    let db = fixture::database();
    let p = fixture::provider("http://127.0.0.1:1");
    assert!(reserve(&db, &p, &mut body(), "chat", "synthetic").unwrap_err().contains("synthetic_budget_required"));
    reserve(&db,&p,&mut body(),"chat","proxy").unwrap();
    db.set_setting(KEY,"{broken").unwrap();
    assert!(reserve(&db,&p,&mut body(),"chat","proxy").is_err());
    let c=db.conn(); let c=c.lock().unwrap();
    assert_eq!(c.query_row("SELECT count(*) FROM budget_ledger",[],|r|r.get::<_,i64>(0)).unwrap(),0);
}
