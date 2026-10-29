use super::*;
#[test]
fn validates_actual_completion_not_a_200_or_substring() {
    let mut evidence = Evidence::default();
    for bad in [
        json!({"choices":[{"message":{"content":"NOT OK"},"finish_reason":"stop"}]}),
        json!({"choices":[{"message":{"content":"OK"},"finish_reason":"length"}]}),
    ] {
        assert!(verify("chat", &serde_json::to_vec(&bad).unwrap(), &mut evidence).is_err());
    }
    let good = json!({"model":"test-model","choices":[{"message":{"content":"OK"},"finish_reason":"stop"}],"usage":{"prompt_tokens":4,"completion_tokens":1}});
    assert!(verify("chat", &serde_json::to_vec(&good).unwrap(), &mut evidence).is_ok());
    assert_eq!(evidence.input_tokens, Some(4));
    assert_eq!(evidence.output_tokens, Some(1));
}
#[test]
fn stream_requires_terminal_event_and_finish_reason() {
    let content = "data: {\"choices\":[{\"delta\":{\"content\":\"OK\"}}]}\n\n";
    let finish = "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n";
    assert!(verify("stream", content.as_bytes(), &mut Evidence::default()).is_err());
    assert!(
        verify(
            "stream",
            format!("{content}{finish}data: [DONE]\n\n").as_bytes(),
            &mut Evidence::default()
        )
        .is_ok()
    );
}

#[test]
fn responses_null_error_is_success_but_real_error_is_rejected() {
    let mut body = json!({"error":null,"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"OK"}]}],"usage":{"input_tokens":4390,"output_tokens":5,"total_tokens":4395}});
    let mut evidence = Evidence::default();
    assert!(verify("responses", &serde_json::to_vec(&body).unwrap(), &mut evidence).is_ok());
    assert_eq!(evidence.input_tokens, Some(4390));
    for error in [json!({"message":"failure"}), json!({}), json!(false), json!("")] {
        body["error"] = error;
        assert_eq!(verify("responses", &serde_json::to_vec(&body).unwrap(), &mut Evidence::default()).unwrap_err(), "response_error_field");
    }
    body["error"] = Value::Null;
    body["status"] = json!("incomplete");
    assert!(verify("responses", &serde_json::to_vec(&body).unwrap(), &mut Evidence::default()).is_err());
}
#[test]
fn stream_null_error_does_not_mask_real_errors_or_missing_termination() {
    let content = "data: {\"error\":null,\"choices\":[{\"delta\":{\"content\":\"OK\"},\"finish_reason\":\"stop\"}]}\n\n";
    assert!(verify("stream", format!("{content}data: [DONE]\n\n").as_bytes(), &mut Evidence::default()).is_ok());
    assert!(verify("stream", content.as_bytes(), &mut Evidence::default()).is_err());
    assert_eq!(verify("stream", format!("{content}data: {{\"error\":{{\"message\":\"failure\"}}}}\n\ndata: [DONE]\n\n").as_bytes(), &mut Evidence::default()).unwrap_err(), "sse_error_event");
}
