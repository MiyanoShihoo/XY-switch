fn events(output: &str) -> Vec<Value> {
    output
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str(data).ok())
        .collect()
}

#[test]
fn responses_stream_completes_parallel_tools_text_reasoning_and_late_usage() {
    let chunks = vec![
        json!({"id":"chat_test","choices":[{"delta":{"role":"assistant","reasoning_content":"Need tools."}}]}),
        json!({"choices":[{"delta":{"content":"Checking"}}]}),
        json!({"choices":[{"delta":{"tool_calls":[
            {"index":1,"id":"call_b","function":{"name":"second","arguments":"{\"b\":"}},
            {"index":0,"id":"call_a","function":{"name":"first","arguments":"{\"a\":"}}
        ]}}]}),
        json!({"choices":[{"delta":{"tool_calls":[
            {"index":0,"function":{"arguments":"1}"}},
            {"index":1,"function":{"arguments":"2}"}}
        ]}}]}),
        json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
        json!({"choices":[],"usage":{"prompt_tokens":9,"completion_tokens":7}}),
    ];
    let mut frames: Vec<String> = chunks.into_iter().map(|v| format!("data: {v}")).collect();
    frames.push("data: [DONE]".to_string());
    let output = render_frames(
        UpstreamFormat::OpenAi,
        ClientFormat::Responses,
        &frames.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let events = events(&output);
    for (n, event) in events.iter().enumerate() {
        assert_eq!(event["sequence_number"], n);
    }
    assert_eq!(
        events
            .iter()
            .filter(|e| e["type"] == "response.completed")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e["type"] == "response.function_call_arguments.done")
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e["type"] == "response.output_item.done")
            .count(),
        4
    );
    let response = &events.last().unwrap()["response"];
    assert_eq!(response["usage"]["total_tokens"], 16);
    assert_eq!(response["output"][0]["summary"][0]["text"], "Need tools.");
    assert_eq!(response["output"][1]["content"][0]["text"], "Checking");
    assert_eq!(response["output"][2]["call_id"], "call_b");
    assert_eq!(response["output"][2]["arguments"], "{\"b\":2}");
    assert_eq!(response["output"][3]["call_id"], "call_a");
    assert_eq!(response["output"][3]["arguments"], "{\"a\":1}");
    let mut history = vec![json!({"role":"user","content":"check both"})];
    history.extend(response["output"].as_array().unwrap().clone());
    history.push(json!({"type":"function_call_output","call_id":"call_a","output":"one"}));
    history.push(json!({"type":"function_call_output","call_id":"call_b","output":"two"}));
    let next = crate::adapters::responses_to_openai_chat(&json!({"input":history})).unwrap();
    assert_eq!(next["messages"][1]["reasoning_content"], "Need tools.");
    assert_eq!(
        next["messages"][1]["tool_calls"].as_array().unwrap().len(),
        2
    );
    assert_eq!(next["messages"][2]["tool_call_id"], "call_a");
    assert_eq!(next["messages"][3]["tool_call_id"], "call_b");
}

#[tokio::test]
async fn responses_stream_handles_byte_fragmentation_and_truncation() {
    let text = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"你好\"}}]}\r\n\r\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\r\n\r\n",
        "data: [DONE]\r\n\r\n"
    );
    let chunks: Vec<Result<Bytes, reqwest::Error>> = text
        .as_bytes()
        .iter()
        .map(|b| Ok(Bytes::from(vec![*b])))
        .collect();
    let out: Vec<_> = transform(
        stream::iter(chunks),
        UpstreamFormat::OpenAi,
        ClientFormat::Responses,
        "kimi-test".into(),
    )
    .collect()
    .await;
    let output = out
        .into_iter()
        .map(|r| String::from_utf8(r.unwrap().to_vec()).unwrap())
        .collect::<String>();
    let e = events(&output);
    assert_eq!(e.last().unwrap()["type"], "response.incomplete");
    assert_eq!(
        e.last().unwrap()["response"]["output"][0]["content"][0]["text"],
        "你好"
    );
    assert!(!output.contains("response.completed"));
}

#[tokio::test]
async fn responses_stream_does_not_report_premature_eof_as_success() {
    for input in [
        "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
        "data: {\"error\":{\"message\":\"failed\"}}\n\n",
    ] {
        let chunks: Vec<Result<Bytes, reqwest::Error>> = vec![Ok(Bytes::from(input.to_string()))];
        let out: Vec<_> = transform(
            stream::iter(chunks),
            UpstreamFormat::OpenAi,
            ClientFormat::Responses,
            "test".into(),
        )
        .collect()
        .await;
        assert!(out.iter().any(Result::is_err));
        assert!(
            !out.iter()
                .filter_map(|r| r.as_ref().ok())
                .any(|b| String::from_utf8_lossy(b).contains("response.completed"))
        );
    }
}

#[test]
fn mixed_sse_line_endings_preserve_frame_order() {
    let mut bytes = b"data: first\n\ndata: second\r\n\r\ndata: third\n\n".to_vec();
    for expected in ["first", "second", "third"] {
        assert_eq!(
            parse_frame(&take_frame(&mut bytes).unwrap()).unwrap().1,
            expected
        );
    }
    assert!(bytes.is_empty());
}
