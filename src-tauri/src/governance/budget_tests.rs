use super::*;
use crate::governance::test_support as fixture;
fn body() -> Value {
    json!({"model":"gov-model","messages":[{"role":"user","content":"OK"}],"max_tokens":16})
}
#[test]
fn concurrent_admission_never_exceeds_tokens_or_cost() {
    for use_cost in [false, true] {
        let db = fixture::database();
        let mut p = fixture::budget(&db);
        if use_cost {
            p.daily_microusd = 3 * (1024 + 16 * 2);
        } else {
            p.daily_tokens = 3 * (1024 + 16);
        }
        save(&db, KEY, &p).unwrap();
        let handles: Vec<_> = (0..32)
            .map(|_| {
                let db = db.clone();
                std::thread::spawn(move || {
                    reserve(
                        &db,
                        &fixture::provider("http://127.0.0.1:1"),
                        &mut body(),
                        "chat",
                        "proxy",
                    )
                    .is_ok()
                })
            })
            .collect();
        assert_eq!(
            handles
                .into_iter()
                .filter_map(|h| h.join().ok())
                .filter(|ok| *ok)
                .count(),
            3
        );
        let status = status(&db).unwrap();
        assert_eq!(
            (
                status.attempts,
                status.reserved_tokens,
                status.reserved_microusd
            ),
            (3, 3120, 3168)
        );
    }
}
#[test]
fn reservations_survive_reopen_and_policy_changes() {
    let path = std::env::temp_dir().join(format!("xy-budget-{}.db", uuid::Uuid::new_v4()));
    {
        let db = Database::open(&path).unwrap();
        let mut p = fixture::budget(&db);
        reserve(
            &db,
            &fixture::provider("http://127.0.0.1:1"),
            &mut body(),
            "chat",
            "proxy",
        )
        .unwrap();
        p.enabled = false;
        save(&db, KEY, &p).unwrap();
        assert_eq!(status(&db).unwrap().attempts, 1);
    }
    {
        let db = Database::open(&path).unwrap();
        let mut p = fixture::budget(&db);
        p.daily_tokens = 1040;
        save(&db, KEY, &p).unwrap();
        assert!(
            reserve(
                &db,
                &fixture::provider("http://127.0.0.1:1"),
                &mut body(),
                "chat",
                "proxy"
            )
            .unwrap_err()
            .contains("hard_token_budget_exhausted")
        );
        assert_eq!(status(&db).unwrap().attempts, 1);
    }
    std::fs::remove_file(path).unwrap();
}
#[test]
fn exact_custom_price_is_mandatory_and_cache_uses_highest_rate() {
    let db = fixture::database();
    fixture::budget(&db);
    let p = fixture::provider("http://127.0.0.1:1");
    let mut missing = body();
    missing["model"] = json!("gov-model-2026");
    assert!(
        reserve(&db, &p, &mut missing, "chat", "proxy")
            .unwrap_err()
            .contains("exact_custom_price")
    );
    {
        let c = db.conn();
        let c = c.lock().unwrap();
        c.execute(
            "UPDATE model_pricing SET source='models.dev' WHERE id='gov-model'",
            [],
        )
        .unwrap();
    }
    assert!(
        reserve(&db, &p, &mut body(), "chat", "proxy")
            .unwrap_err()
            .contains("exact_custom_price")
    );
    {
        let c = db.conn();
        let c = c.lock().unwrap();
        c.execute("UPDATE model_pricing SET source='custom',cache_creation_per_million=4.0001 WHERE id='gov-model'",[]).unwrap();
    }
    reserve(&db, &p, &mut body(), "chat", "proxy").unwrap();
    assert_eq!(status(&db).unwrap().reserved_microusd, 4129);
}
#[test]
fn payload_caps_all_supported_protocols_and_rejects_unbounded_inputs() {
    let p = BudgetPolicy {
        max_output_tokens: 32,
        ..Default::default()
    };
    for (format, key) in [
        ("chat", "max_completion_tokens"),
        ("responses", "max_output_tokens"),
        ("anthropic", "max_tokens"),
    ] {
        // Only protocol-native fields are accepted; use each protocol's own cap.
        let mut payload = json!({"model":"x",key:100});
        assert_eq!(bound_payload(&mut payload, format, &p).unwrap(), 32);
        assert_eq!(payload[key], 32);
    }
    for extra in [
        json!({"n":2}),
        json!({"service_tier":"priority"}),
        json!({"previous_response_id":"x"}),
        json!({"messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"https://example.invalid"}}]}]}),
        json!({"tools":[{"type":"web_search"}]}),
        json!({"max_tokens":0}),
        json!({"unknown_billable_feature":true}),
        json!({"extra_body":{"max_tokens":10000}}),
    ] {
        let mut payload = body();
        payload
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert!(bound_payload(&mut payload, "chat", &p).is_err(), "{extra}");
    }
    let mut oversized = body();
    oversized["messages"] = json!([{"role":"user","content":"x".repeat(5000)}]);
    assert!(
        bound_payload(&mut oversized, "chat", &p)
            .unwrap_err()
            .contains("input_reservation")
    );
    let mut thinking = json!({"model":"x","messages":[],"max_tokens":32,"thinking":{"type":"enabled","budget_tokens":32}});
    assert!(bound_payload(&mut thinking, "anthropic", &p).is_err());
    assert!(bound_payload(&mut json!({"contents":[]}), "gemini", &p).is_err());
}
#[test]
fn invalid_policy_and_disabled_synthetic_gate_fail_closed_without_reservation() {
    let db = fixture::database();
    let p = fixture::provider("http://127.0.0.1:1");
    assert!(
        reserve(&db, &p, &mut body(), "chat", "synthetic")
            .unwrap_err()
            .contains("synthetic_budget_required")
    );
    reserve(&db, &p, &mut body(), "chat", "proxy").unwrap();
    db.set_setting(KEY, "{broken").unwrap();
    assert!(reserve(&db, &p, &mut body(), "chat", "proxy").is_err());
    let c = db.conn();
    let c = c.lock().unwrap();
    assert_eq!(
        c.query_row("SELECT count(*) FROM budget_ledger", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

fn native_payload() -> Value {
    json!({
        "model": "gov-model", "input": "OK", "max_output_tokens": 96,
        "include": ["reasoning.encrypted_content"],
        "tools": [
            {"type":"function","name":"echo","parameters":{"type":"object"}},
            {"type":"custom","name":"patch","format":{"type":"grammar","syntax":"lark","definition":"start: /.+/"}},
            {"type":"namespace","name":"local","tools":[{"type":"custom","name":"text","format":{"type":"text"}}]},
            {"type":"tool_search","execution":"client","parameters":{"type":"object"}}
        ],
        "tool_choice": "auto", "client_metadata": {"fixture":"budget"}
    })
}

#[test]
fn saved_codex_native_requests_admit_tools_results_and_resumed_history() {
    let policy = BudgetPolicy {
        input_reservation: 24_000,
        max_output_tokens: 64,
        ..Default::default()
    };
    let requests = fixture::native_codex_requests();
    assert_eq!(requests.len(), 4);
    for (index, original) in requests.into_iter().enumerate() {
        let mut payload = original.clone();
        assert_eq!(
            bound_payload(&mut payload, "responses", &policy).unwrap(),
            64,
            "request {index}"
        );
        let mut expected = original;
        expected["max_output_tokens"] = json!(64);
        assert_eq!(
            payload, expected,
            "native request {index} must preserve complete inline history and tools"
        );
    }
}

#[test]
fn native_schema_metadata_and_arguments_are_inline_data_not_remote_references() {
    let mut payload = native_payload();
    let schema = json!({"type":"object","properties":{
        "file_id":{"type":"string"},"image_url":{"type":"string"},
        "encrypted_content":{"type":"string"},"extra_body":{"type":"object"},
        "previous_response_id":{"type":"string"},"conversation":{"type":"string"}
    }});
    payload["tools"][0]["parameters"] = schema.clone();
    payload["text"] =
        json!({"format":{"type":"json_schema","name":"inline","schema":schema,"strict":true}});
    payload["metadata"] = json!({"previous_response_id":"ordinary-metadata","type":"input_image"});
    payload["client_metadata"] = json!({"file_id":"ordinary-metadata"});
    payload["input"] = json!([
        {"role":"user","content":"JSON field names are text"},
        {"type":"function_call","name":"echo","call_id":"c1","arguments":"{\"file_id\":\"inline\"}"},
        {"type":"function_call_output","call_id":"c1","output":"{\"encrypted_content\":\"inline\"}"}
    ]);
    let policy = BudgetPolicy {
        input_reservation: 8_000,
        ..Default::default()
    };
    assert_eq!(
        bound_payload(&mut payload, "responses", &policy).unwrap(),
        96
    );
}

#[test]
fn native_dynamic_tools_text_results_and_transparent_reasoning_are_bounded() {
    for encrypted in [Value::Null, json!("")] {
        let mut payload = native_payload();
        payload["input"] = json!([
            {"type":"additional_tools","role":"developer","tools":payload["tools"]},
            {"type":"tool_search_call","execution":"client","call_id":"search","arguments":{"query":"echo","file_id":"inline"}},
            {"type":"tool_search_output","execution":"client","call_id":"search","tools":[payload["tools"][2].clone()]},
            {"type":"custom_tool_call","name":"text","namespace":"local","call_id":"custom","input":"中文"},
            {"type":"custom_tool_call_output","call_id":"custom","output":[{"type":"input_text","text":"inline result"}]},
            {"type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"done","annotations":[]}]},
            {"type":"reasoning","summary":[{"type":"summary_text","text":"explicit history"}],"content":[{"type":"reasoning_text","text":"visible reasoning"}],"encrypted_content":encrypted}
        ]);
        payload["tool_choice"] = json!({"type":"allowed_tools","mode":"auto","tools":[
            {"type":"function","name":"echo"},{"type":"custom","namespace":"local","name":"text"},{"type":"tool_search"}
        ]});
        let policy = BudgetPolicy {
            input_reservation: 8_000,
            ..Default::default()
        };
        assert_eq!(
            bound_payload(&mut payload, "responses", &policy).unwrap(),
            96
        );
    }
}

#[test]
fn native_every_inline_definition_result_and_history_counts_at_utf8_boundary() {
    let data = "工具定义与结果".repeat(120);
    let changes = [
        json!({"tools":[{"type":"custom","name":"patch","format":{"type":"grammar","syntax":"lark","definition":data}}]}),
        json!({"tools":[{"type":"function","name":"echo","parameters":{"description":data}}]}),
        json!({"tools":[{"type":"tool_search","execution":"client","parameters":{"description":data}}]}),
        json!({"client_metadata":{"inline":data}}),
        json!({"input":[{"type":"function_call_output","call_id":"c1","output":data}]}),
        json!({"input":[{"type":"custom_tool_call_output","call_id":"c1","output":data}]}),
        json!({"input":[{"type":"additional_tools","role":"developer","tools":[{"type":"function","name":"echo","parameters":{"description":data}}]}]}),
        json!({"input":[{"type":"tool_search_output","execution":"client","call_id":"search","tools":[{"type":"namespace","name":"local","tools":[{"type":"function","name":"echo","parameters":{"description":data}}]}]}]}),
        json!({"input":[{"role":"assistant","content":data},{"role":"user","content":"resume"}]}),
        json!({"text":{"format":{"type":"json_schema","name":"reply","schema":{"description":data}}}}),
    ];
    for change in changes {
        let mut payload = native_payload();
        payload
            .as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        payload["max_output_tokens"] = json!(64);
        let bytes = serde_json::to_vec(&payload).unwrap().len() as i64 + 256;
        let mut policy = BudgetPolicy {
            input_reservation: bytes,
            max_output_tokens: 64,
            ..Default::default()
        };
        assert_eq!(
            bound_payload(&mut payload.clone(), "responses", &policy).unwrap(),
            64
        );
        policy.input_reservation -= 1;
        assert_eq!(
            bound_payload(&mut payload, "responses", &policy).unwrap_err(),
            "budget_input_reservation_exceeded"
        );
    }
}

#[test]
fn native_hosted_tools_and_unaccountable_history_fail_closed_in_every_protocol_location() {
    let policy = BudgetPolicy {
        input_reservation: 24_000,
        ..Default::default()
    };
    let changes = [
        (
            json!({"previous_response_id":"remote"}),
            "budget_unbounded_input",
        ),
        (json!({"conversation":"remote"}), "budget_unbounded_input"),
        (json!({"prompt":{"id":"stored"}}), "budget_unbounded_input"),
        (
            json!({"context_management":[{"type":"compaction"}]}),
            "budget_unbounded_input",
        ),
        (
            json!({"input":[{"type":"item_reference","id":"remote"}]}),
            "budget_unbounded_input",
        ),
        (
            json!({"input":[{"role":"user","content":[{"type":"input_image","image_url":"https://example.invalid"}]}]}),
            "budget_unbounded_input",
        ),
        (
            json!({"input":[{"type":"reasoning","summary":[],"encrypted_content":"opaque"}]}),
            "budget_unbounded_input",
        ),
        (
            json!({"input":[{"type":"compaction","encrypted_content":"opaque"}]}),
            "budget_unbounded_input",
        ),
        (
            json!({"input":[{"type":"function_call_output","call_id":"c1","output":[{"type":"input_file","file_id":"remote"}]}]}),
            "budget_unbounded_input",
        ),
        (
            json!({"tools":[{"type":"web_search"}]}),
            "budget_server_tools_blocked",
        ),
        (
            json!({"tools":[{"type":"tool_search"}]}),
            "budget_server_tools_blocked",
        ),
        (
            json!({"tools":[{"type":"tool_search","execution":"server"}]}),
            "budget_server_tools_blocked",
        ),
        (
            json!({"tools":[{"type":"namespace","name":"remote","tools":[{"type":"mcp","server_url":"https://example.invalid"}]}]}),
            "budget_server_tools_blocked",
        ),
        (
            json!({"input":[{"type":"additional_tools","role":"developer","tools":[{"type":"file_search"}]}]}),
            "budget_server_tools_blocked",
        ),
        (
            json!({"input":[{"type":"tool_search_call","execution":"server","arguments":{"query":"remote"}}]}),
            "budget_server_tools_blocked",
        ),
        (
            json!({"input":[{"type":"tool_search_output","call_id":"search","tools":[]}]}),
            "budget_server_tools_blocked",
        ),
        (
            json!({"input":[{"type":"tool_search_output","execution":"client","call_id":"search","tools":[{"type":"web_search"}]}]}),
            "budget_server_tools_blocked",
        ),
        (
            json!({"tool_choice":{"type":"web_search"}}),
            "budget_server_tools_blocked",
        ),
        (
            json!({"unknown_billable_feature":true}),
            "budget_unbounded_parameter",
        ),
        (
            json!({"tools":[{"type":"function","name":"echo","parameters":{},"unknown_billable_feature":true}]}),
            "budget_unbounded_parameter",
        ),
        (
            json!({"input":[{"role":"user","content":"OK","file_id":"remote"}]}),
            "budget_unbounded_parameter",
        ),
        (
            json!({"reasoning":{"effort":"medium","unknown_billable_feature":true}}),
            "budget_unbounded_parameter",
        ),
    ];
    for (change, reason) in changes {
        let mut payload = native_payload();
        payload
            .as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        assert_eq!(
            bound_payload(&mut payload, "responses", &policy).unwrap_err(),
            reason,
            "{change}"
        );
    }
}

#[test]
fn native_malformed_local_tools_and_text_history_do_not_reserve_budget() {
    let changes = [
        json!({"tools":[{"type":"function","name":"echo","parameters":"not a schema"}]}),
        json!({"tools":[{"type":"custom","name":"patch","format":{"type":"grammar","syntax":"unknown","definition":"x"}}]}),
        json!({"tools":[{"type":"namespace","name":"local","tools":{}}]}),
        json!({"input":[{"type":"custom_tool_call","name":"patch","call_id":"c1","input":{}}]}),
        json!({"input":[{"type":"function_call_output","call_id":"c1","output":{"text":"unsupported structured result"}}]}),
        json!({"input":[{"role":"user","content":[{"type":"input_text","text":123}]}]}),
        json!({"input":[{"type":"tool_search_call","execution":"client","arguments":null}]}),
        json!({"input":[{"type":"additional_tools","role":"user","tools":[]}]}),
        json!({"client_metadata":[]}),
    ];
    for change in changes {
        let db = fixture::database();
        let mut policy = fixture::budget(&db);
        policy.input_reservation = 24_000;
        save(&db, KEY, &policy).unwrap();
        let mut payload = native_payload();
        payload
            .as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        assert!(
            reserve(
                &db,
                &fixture::provider("http://127.0.0.1:1"),
                &mut payload,
                "responses",
                "proxy"
            )
            .is_err(),
            "{change}"
        );
        assert_eq!(status(&db).unwrap().attempts, 0);
        let connection = db.conn();
        let connection = connection.lock().unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM governance_events WHERE kind='budget_rejected'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
    }
}
