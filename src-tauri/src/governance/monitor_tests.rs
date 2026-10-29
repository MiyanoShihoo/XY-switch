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
