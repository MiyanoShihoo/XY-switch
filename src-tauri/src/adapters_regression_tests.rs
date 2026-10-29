#[test]
fn converts_codex_tool_choice_images_and_developer_messages() {
    let request = json!({"model":"kimi-test", "stream":true,"parallel_tool_calls":false,
        "input":[{"role":"developer","content":[{"type":"input_text","text":"Be accurate"}]},
        {"role":"user","content":[{"type":"input_text","text":"Describe"},{"type":"input_image","image_url":"https://example.test/image.png","detail":"low"}]}],
        "tools":[{"type":"function","name":"inspect","parameters":{"type":"object"},"strict":true}],
        "tool_choice":{"type":"function","name":"inspect"}});
    let chat = responses_to_openai_chat(&request).unwrap();
    assert_eq!(chat["messages"][0]["role"], "system");
    assert_eq!(
        chat["messages"][1]["content"][1]["image_url"]["url"],
        "https://example.test/image.png"
    );
    assert_eq!(chat["tools"][0]["function"]["strict"], true);
    assert_eq!(chat["tool_choice"]["function"]["name"], "inspect");
    assert_eq!(chat["parallel_tool_calls"], false);
    assert_eq!(chat["stream_options"]["include_usage"], true);
    assert!(chat.get("instructions").is_none());
}

#[test]
fn chat_bridge_rejects_unsupported_state_and_hosted_tools_explicitly() {
    assert!(
        responses_to_openai_chat(&json!({"input":"hi","previous_response_id":"resp_old"})).is_err()
    );
    assert!(
        responses_to_openai_chat(&json!({"input":"hi","tools":[{"type":"web_search"}]})).is_err()
    );
}

#[test]
fn nonstream_kimi_reasoning_survives_tool_result_replay() {
    let response = openai_chat_response_to_responses(
        json!({"id":"chat_kimi","choices":[{"message":{
        "role":"assistant","reasoning_content":"Inspect file first", "content":null,
        "tool_calls":[{"id":"call_read","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"README.md\"}"}}]},"finish_reason":"tool_calls"}]}),
    );
    let mut input = vec![json!({"role":"user","content":"Read the README"})];
    input.extend(response["output"].as_array().unwrap().clone());
    input
        .push(json!({"type":"function_call_output","call_id":"call_read","output":"Project docs"}));
    let replay = responses_to_openai_chat(&json!({"input":input})).unwrap();
    assert_eq!(
        replay["messages"][1]["reasoning_content"],
        "Inspect file first"
    );
    assert_eq!(replay["messages"][1]["tool_calls"][0]["id"], "call_read");
    assert_eq!(replay["messages"][2]["role"], "tool");
}

#[test]
fn nonstream_truncation_is_not_reported_as_completed() {
    for (reason, expected) in [
        ("length", "max_output_tokens"),
        ("content_filter", "content_filter"),
    ] {
        let response = openai_chat_response_to_responses(
            json!({"choices":[{"message":{"role":"assistant","content":"partial"},"finish_reason":reason}],"usage":{"prompt_tokens":5,"completion_tokens":3}}),
        );
        assert_eq!(response["status"], "incomplete");
        assert_eq!(response["incomplete_details"]["reason"], expected);
        assert_eq!(response["usage"]["total_tokens"], 8);
    }
}

#[test]
fn malformed_tool_wrappers_are_rejected_without_panicking() {
    for tool in [
        json!({"type":"function","name":"top","function":null}),
        json!({"type":"function","name":"top","function":{}}),
        json!({"type":"custom","name":"patch","function":42}),
    ] {
        assert!(responses_to_openai_chat(&json!({"input":"hello","tools":[tool]})).is_err());
    }
    assert!(responses_to_openai_chat(&json!({"input":"hello","tools":{}})).is_err());
}
