use super::*;
use serde_json::json;
use std::path::Path;

fn database() -> Database {
    Database::open(Path::new(":memory:")).unwrap()
}
fn price(id: &str) -> PricingInput {
    PricingInput {
        id: id.into(),
        input_per_million: 2.0,
        output_per_million: 4.0,
        cache_read_per_million: Some(1.0),
        cache_creation_per_million: Some(3.0),
    }
}
fn usage(id: &str) -> RequestUsage {
    RequestUsage {
        model: Some(id.into()),
        input_tokens: Some(1_000_000),
        output_tokens: Some(1_000_000),
        ..Default::default()
    }
}
fn estimate_for(db: &Database, usage: &RequestUsage) -> Option<PriceEstimate> {
    let arc = db.conn();
    let conn = arc.lock().unwrap();
    estimate(&conn, usage).unwrap()
}
fn catalog() -> Vec<u8> {
    serde_json::to_vec(&json!({"qa":{"models":{"model":{"id":"model","cost":{"input":2,"output":4,"cache_read":1,"cache_write":3}},"unpriced":{},"tiered":{"cost":{"input":1,"output":2,"context_over_200k":{"input":4}}}}}})).unwrap()
}
fn event(db: &Database) -> i64 {
    let arc = db.conn();
    let conn = arc.lock().unwrap();
    conn.execute("INSERT INTO request_events(provider_id,route,created_at) VALUES('openai','/v1/responses',1)", []).unwrap();
    conn.last_insert_rowid()
}

#[test]
fn custom_price_round_trip_and_remove_are_real_database_operations() {
    let db = database();
    assert!(list(&db).unwrap().is_empty());
    save(&db, &price("qa-model")).unwrap();
    let rows = list(&db).unwrap();
    assert_eq!(rows[0].pricing, price("qa-model"));
    assert_eq!(rows[0].source, "custom");
    assert!(rows[0].updated_at > 0);
    let mut changed = price("qa-model");
    changed.input_per_million = 9.0;
    save(&db, &changed).unwrap();
    assert_eq!(list(&db).unwrap().len(), 1);
    assert_eq!(list(&db).unwrap()[0].pricing.input_per_million, 9.0);
    remove_custom(&db, "qa-model").unwrap();
    assert!(list(&db).unwrap().is_empty());
    assert!(remove_custom(&db, "qa-model").is_err());
}

#[test]
fn pricing_persists_after_reopening_an_isolated_database() {
    let base = std::env::temp_dir().canonicalize().unwrap();
    let path = base.join(format!("xy-switch-pricing-{}.db", uuid::Uuid::new_v4()));
    {
        let db = Database::open(&path).unwrap();
        save(&db, &price("persisted")).unwrap();
    }
    {
        let reopened = Database::open(&path).unwrap();
        assert_eq!(list(&reopened).unwrap()[0].pricing, price("persisted"));
    }
    assert_eq!(path.parent(), Some(base.as_path()));
    assert_eq!(path.canonicalize().unwrap(), path);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn validation_rejects_nonfinite_negative_oversized_and_extra_fields() {
    let db = database();
    for bad in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -0.01,
        MAX_RATE + 1.0,
    ] {
        let mut p = price("bad");
        p.input_per_million = bad;
        assert!(save(&db, &p).is_err());
        let mut p = price("bad");
        p.cache_read_per_million = Some(bad);
        assert!(save(&db, &p).is_err());
    }
    for id in ["", "bad id", "bad\n", &"x".repeat(257)] {
        assert!(save(&db, &price(id)).is_err());
    }
    assert!(serde_json::from_value::<PricingInput>(
        json!({"id":"qa","inputPerMillion":1,"outputPerMillion":2,"source":"models.dev"})
    )
    .is_err());
    assert!(list(&db).unwrap().is_empty());
}

#[test]
fn catalog_preserves_provider_identity_and_missing_cache_prices() {
    let prices = parse_catalog(&catalog()).unwrap();
    assert_eq!(prices, vec![price("qa/model")]);
    let no_cache =
        parse_catalog(br#"{"vendor":{"models":{"free":{"cost":{"input":0,"output":0}}}}}"#)
            .unwrap();
    assert_eq!(no_cache[0].cache_read_per_million, None);
    let db = database();
    replace_synced(&db, &prices).unwrap();
    assert!(estimate_for(&db, &usage("model")).is_none());
    assert_eq!(
        estimate_for(&db, &usage("qa/model")).unwrap().source,
        "models.dev"
    );
    assert!(remove_custom(&db, "qa/model").is_err());
}

#[test]
fn malformed_or_empty_catalog_is_not_a_successful_zero_sync() {
    for bytes in [
        b"{}".as_slice(),
        b"[]",
        b"not JSON",
        br#"{"qa":{"models":{"m":{"cost":{"input":-1,"output":2}}}}}"#,
        br#"{"qa":{"models":{"m":{"cost":{"input":"1","output":2}}}}}"#,
    ] {
        assert!(parse_catalog(bytes).is_err());
    }
    let duplicate = json!({"qa":{"models":{"a":{"id":"same","cost":{"input":1,"output":2}},"b":{"id":"same","cost":{"input":1,"output":2}}}}});
    assert!(parse_catalog(&serde_json::to_vec(&duplicate).unwrap()).is_err());
    assert!(parse_catalog(&vec![b' '; MAX_DOWNLOAD_BYTES + 1]).is_err());
}

#[test]
fn sync_preserves_custom_overrides_and_atomically_replaces_stale_reference_rows() {
    let db = database();
    replace_synced(&db, &[price("qa/old"), price("qa/model")]).unwrap();
    let mut custom = price("qa/model");
    custom.input_per_million = 42.0;
    save(&db, &custom).unwrap();
    assert_eq!(
        replace_synced(&db, &[price("qa/model"), price("qa/new")]).unwrap(),
        1
    );
    let rows = list(&db).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].pricing, custom);
    assert_eq!(rows[0].source, "custom");
    assert!(rows.iter().all(|p| p.pricing.id != "qa/old"));
}

#[test]
fn sync_sql_failure_rolls_back_deletion_and_prior_inserts() {
    let db = database();
    replace_synced(&db, &[price("qa/original")]).unwrap();
    let original = list(&db).unwrap();
    db.conn().lock().unwrap().execute_batch("CREATE TRIGGER reject_qa_price BEFORE INSERT ON model_pricing WHEN NEW.id='qa/fail' BEGIN SELECT RAISE(ABORT,'qa failure'); END;").unwrap();
    assert!(replace_synced(&db, &[price("qa/first"), price("qa/fail")]).is_err());
    assert_eq!(list(&db).unwrap(), original);
}

#[test]
fn estimate_distinguishes_free_unknown_partial_and_invalid_cache_counts() {
    let db = database();
    save(&db, &price("qa-model")).unwrap();
    assert_eq!(
        estimate_for(&db, &usage("qa-model")).unwrap().total,
        Some(6.0)
    );
    assert!(estimate_for(&db, &usage("unregistered-model")).is_none());
    let mut partial = usage("qa-model");
    partial.output_tokens = None;
    assert_eq!(estimate_for(&db, &partial).unwrap().total, None);
    let mut invalid = usage("qa-model");
    invalid.cache_read_tokens = Some(2_000_000);
    assert_eq!(estimate_for(&db, &invalid).unwrap().total, None);
    invalid.cache_read_tokens = Some(-1);
    assert_eq!(estimate_for(&db, &invalid).unwrap().total, None);
    let free = PricingInput {
        id: "free".into(),
        input_per_million: 0.0,
        output_per_million: 0.0,
        cache_read_per_million: None,
        cache_creation_per_million: None,
    };
    save(&db, &free).unwrap();
    assert_eq!(estimate_for(&db, &usage("free")).unwrap().total, Some(0.0));
    let mut cached = usage("free");
    cached.cache_read_tokens = Some(1);
    assert_eq!(estimate_for(&db, &cached).unwrap().total, None);
}

#[test]
fn anthropic_input_excludes_caches_while_openai_input_includes_them() {
    let db = database();
    save(&db, &price("qa-model")).unwrap();
    let mut u = usage("qa-model");
    u.cache_read_tokens = Some(500_000);
    u.cache_creation_tokens = Some(250_000);
    let inclusive = estimate_for(&db, &u).unwrap();
    assert_eq!(inclusive.total, Some(5.75));
    u.audit = Some(crate::model_audit::audit(crate::model_audit::Observation {
        protocol: Some("anthropic".into()),
        ..Default::default()
    }));
    let exclusive = estimate_for(&db, &u).unwrap();
    assert_eq!(exclusive.input, Some(2.0));
    assert_eq!(exclusive.total, Some(7.25));
}

#[test]
fn broken_or_unfinished_stream_does_not_claim_a_complete_bill() {
    let db = database();
    save(&db, &price("qa-model")).unwrap();
    let mut u = usage("qa-model");
    u.audit = Some(crate::model_audit::audit(crate::model_audit::Observation {
        streaming: true,
        usage_final: false,
        ..Default::default()
    }));
    assert_eq!(estimate_for(&db, &u).unwrap().total, None);
    u.audit.as_mut().unwrap().observation.usage_final = true;
    assert_eq!(estimate_for(&db, &u).unwrap().total, Some(6.0));
    u.audit.as_mut().unwrap().observation.parse_invalid = true;
    assert_eq!(estimate_for(&db, &u).unwrap().total, None);
}

#[test]
fn new_request_uses_custom_price_but_later_price_edits_do_not_rewrite_history() {
    let db = database();
    save(&db, &price("qa-model")).unwrap();
    let id = event(&db);
    db.update_request_usage(id, &usage("qa-model")).unwrap();
    let mut changed = price("qa-model");
    changed.input_per_million = 100.0;
    save(&db, &changed).unwrap();
    let arc = db.conn();
    let conn = arc.lock().unwrap();
    let (cost, input, output, source): (f64,f64,f64,String) = conn.query_row("SELECT cost,input_cost,output_cost,request_events.price_source FROM request_events JOIN request_price_snapshots ON event_id=id WHERE id=?1", [id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap();
    assert_eq!((cost, input, output), (6.0, 2.0, 4.0));
    assert_eq!(source, "custom");
}

#[test]
fn failed_snapshot_does_not_commit_a_cost_without_its_breakdown() {
    let db = database();
    save(&db, &price("qa-model")).unwrap();
    let id = event(&db);
    db.conn().lock().unwrap().execute_batch("CREATE TRIGGER reject_qa_snapshot BEFORE INSERT ON request_price_snapshots BEGIN SELECT RAISE(ABORT,'qa snapshot failure'); END;").unwrap();
    assert!(db.update_request_usage(id, &usage("qa-model")).is_err());
    let cost: Option<f64> = db
        .conn()
        .lock()
        .unwrap()
        .query_row("SELECT cost FROM request_events WHERE id=?1", [id], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(cost, None);
}

async fn server(
    status: u16,
    body: Vec<u8>,
    content_length: Option<&str>,
) -> (String, tokio::task::JoinHandle<()>) {
    use axum::{body::Body, http::Response, routing::get, Router};
    let length = content_length.map(str::to_owned);
    let app = Router::new().route(
        "/api.json",
        get(move || {
            let body = body.clone();
            let length = length.clone();
            async move {
                let mut response = Response::builder()
                    .status(status)
                    .header("content-type", "application/json");
                if let Some(length) = length {
                    response = response.header("content-length", length);
                }
                if status == 302 {
                    response = response.header("location", "/must-not-follow");
                }
                response.body(Body::from(body)).unwrap()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/api.json", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, task)
}

#[tokio::test]
async fn public_catalog_sync_transport_is_bounded_and_rejects_errors_and_redirects() {
    let client = crate::network::client_builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let db = database();
    save(&db, &price("keep-custom")).unwrap();
    for (status, body, succeeds) in [
        (200, catalog(), true),
        (401, b"unauthorized".to_vec(), false),
        (429, b"limited".to_vec(), false),
        (500, b"failed".to_vec(), false),
        (302, Vec::new(), false),
        (200, b"not JSON".to_vec(), false),
    ] {
        let (url, task) = server(status, body, None).await;
        let result = download_catalog(&client, &url).await;
        task.abort();
        assert_eq!(result.is_ok(), succeeds, "status {status}: {result:?}");
        assert_eq!(list(&db).unwrap()[0].pricing, price("keep-custom"));
    }
}
