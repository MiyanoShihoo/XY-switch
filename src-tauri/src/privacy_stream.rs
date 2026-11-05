use super::{PrivacyReport, ResponsePrivacy, MAX_RESPONSE_BYTES, response_code};
use crate::{database::Database, streaming::ClientFormat};
use axum::body::Bytes;
use futures_util::{Stream, StreamExt, stream};
use serde_json::{Value, json};
use std::{collections::{BTreeMap, VecDeque}, io, pin::Pin, time::Duration};
use zeroize::{Zeroize, Zeroizing};

const MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;
const MAX_PENDING_FRAMES: usize = 65_536;
const HEARTBEAT: &[u8] = b": xy-privacy\n\n";
const INITIAL_HEARTBEAT_DELAY: Duration = Duration::from_secs(1);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

struct Piece { path: String, lane: String, original: Zeroizing<String> }
struct Frame { headers: Vec<(String, String)>, value: Option<Value>, pieces: Vec<Piece>, original: Zeroizing<Vec<u8>>, changed: bool, done: bool }
struct Lane { text: Zeroizing<String>, protected: bool, json_required: bool, closed: bool }

struct ProtectedStream<S> {
    upstream: Pin<Box<S>>,
    context: ResponsePrivacy,
    format: ClientFormat,
    database: Database,
    event_id: Option<i64>,
    buffer: Vec<u8>,
    frames: Vec<Frame>,
    lanes: BTreeMap<String, Lane>,
    output: VecDeque<Bytes>,
    report: PrivacyReport,
    failure: Option<&'static str>,
    total: usize,
    terminal: bool,
    gemini_finishes: BTreeMap<u64, bool>,
    eof: bool,
    heartbeat_at: tokio::time::Instant,
}

fn error(code: &'static str) -> io::Error { io::Error::other(response_code(code)) }

impl<S> ProtectedStream<S> {
    fn persist(&mut self) -> Result<(), io::Error> {
        let report = std::mem::take(&mut self.report);
        self.database.update_response_privacy(self.event_id, &report, self.failure)
            .map_err(|_| error("privacy_scan_failed"))
    }

    fn inspect(&mut self, value: Value) -> Result<Value, io::Error> {
        let checked = self.context.inspect(value).map_err(error)?;
        self.report.merge(&checked.report);
        if let Some(code) = checked.blocked_code { return Err(error(code)); }
        Ok(checked.redacted)
    }

    fn piece(&mut self, frame: &mut Frame, path: String, lane: String, protected: bool, json_required: bool) -> Result<(), io::Error> {
        let Some(value) = frame.value.as_mut().and_then(|value| value.pointer_mut(&path)) else { return Ok(()); };
        if value.is_null() { return Ok(()); }
        let text = value.as_str().ok_or_else(|| error("privacy_scan_failed"))?;
        let state = self.lanes.entry(lane.clone()).or_insert_with(|| Lane {
            text: Zeroizing::new(String::new()), protected, json_required, closed: false,
        });
        if state.closed || state.text.len().saturating_add(text.len()) > MAX_RESPONSE_BYTES {
            return Err(error("privacy_scan_limit"));
        }
        state.text.push_str(text);
        let original = Zeroizing::new(text.to_owned());
        *value = Value::String(String::new());
        frame.pieces.push(Piece { path, lane, original });
        Ok(())
    }

    fn close(&mut self, prefix: &str) {
        for (key, lane) in &mut self.lanes { if key.starts_with(prefix) { lane.closed = true; } }
    }

    fn prepare(&mut self, mut frame: Frame) -> Result<(), io::Error> {
        if self.frames.len().saturating_add(self.output.len()) >= MAX_PENDING_FRAMES {
            return Err(error("privacy_scan_limit"));
        }
        let Some(value) = frame.value.as_ref() else {
            if frame.done { self.close(""); self.terminal = true; }
            self.frames.push(frame); return self.flush();
        };
        if self.terminal { return Err(error("privacy_scan_failed")); }
        let kind = value["type"].as_str().or_else(|| frame.headers.iter()
            .find(|(key, _)| key == "event").map(|(_, value)| value.as_str())).unwrap_or("").to_owned();
        match self.format {
            ClientFormat::Chat => {
                let choices = value["choices"].as_array().cloned().unwrap_or_default();
                for (position, choice) in choices.iter().enumerate() {
                    let index = choice["index"].as_u64().unwrap_or(position as u64);
                    let lane = format!("choice:{index}:");
                    for key in ["content", "reasoning_content", "reasoning", "refusal"] {
                        self.piece(&mut frame, format!("/choices/{position}/delta/{key}"), format!("{lane}{key}"), false, false)?;
                    }
                    if let Some(calls) = choice["delta"]["tool_calls"].as_array() {
                        for (tool_position, call) in calls.iter().enumerate() {
                            let tool_index = call["index"].as_u64().unwrap_or(tool_position as u64);
                            for (key, json_required) in [("id", false), ("function/name", false), ("function/arguments", true)] {
                                self.piece(&mut frame, format!("/choices/{position}/delta/tool_calls/{tool_position}/{key}"),
                                    format!("{lane}tool:{tool_index}:{key}"), true, json_required)?;
                            }
                        }
                    }
                    if let Some(function) = choice["delta"].get("function_call") {
                        if !function.is_null() {
                            for (key, json_required) in [("name", false), ("arguments", true)] {
                                self.piece(&mut frame, format!("/choices/{position}/delta/function_call/{key}"),
                                    format!("{lane}legacy:{key}"), true, json_required)?;
                            }
                        }
                    }
                    if choice.get("finish_reason").is_some_and(|reason| !reason.is_null()) { self.close(&lane); }
                }
            }
            ClientFormat::Anthropic => {
                let index = value["index"].as_u64().unwrap_or(0);
                let lane = format!("block:{index}:");
                match kind.as_str() {
                    "content_block_start" => {
                        match value["content_block"]["type"].as_str().unwrap_or("") {
                            "text" => self.piece(&mut frame, "/content_block/text".into(), format!("{lane}text"), false, false)?,
                            "thinking" => {
                                self.piece(&mut frame, "/content_block/thinking".into(), format!("{lane}thinking"), false, false)?;
                                self.piece(&mut frame, "/content_block/signature".into(), format!("{lane}signature"), true, false)?;
                            }
                            _ => {}
                        }
                    }
                    "content_block_delta" => {
                        match value["delta"]["type"].as_str().unwrap_or("") {
                            "text_delta" => self.piece(&mut frame, "/delta/text".into(), format!("{lane}text"), false, false)?,
                            "thinking_delta" => self.piece(&mut frame, "/delta/thinking".into(), format!("{lane}thinking"), false, false)?,
                            "input_json_delta" => self.piece(&mut frame, "/delta/partial_json".into(), format!("{lane}arguments"), true, true)?,
                            "signature_delta" => self.piece(&mut frame, "/delta/signature".into(), format!("{lane}signature"), true, false)?,
                            _ => return Err(error("privacy_scan_failed")),
                        }
                    }
                    "content_block_stop" => self.close(&lane),
                    "message_stop" => { self.close(""); self.terminal = true; }
                    _ => {},
                }
            }
            ClientFormat::Responses => {
                let item = value["item_id"].as_str().map(str::to_owned)
                    .or_else(|| value["item"]["id"].as_str().map(str::to_owned))
                    .unwrap_or_else(|| value["output_index"].to_string());
                let lane = format!("item:{item}:");
                let content_index = value["content_index"].as_u64().unwrap_or(0);
                match kind.as_str() {
                    "response.output_text.delta" | "response.refusal.delta" | "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                        self.piece(&mut frame, "/delta".into(), format!("{lane}{content_index}:{kind}"), false, false)?;
                    }
                    "response.function_call_arguments.delta" | "response.custom_tool_call_input.delta" => {
                        self.piece(&mut frame, "/delta".into(), format!("{lane}arguments"), true, kind == "response.function_call_arguments.delta")?;
                    }
                    "response.output_text.done" | "response.refusal.done" | "response.reasoning_summary_text.done" | "response.reasoning_text.done" => {
                        let delta_kind = format!("{}.delta", kind.trim_end_matches(".done"));
                        self.close(&format!("{lane}{content_index}:{delta_kind}"));
                    }
                    "response.function_call_arguments.done" | "response.custom_tool_call_input.done" => self.close(&format!("{lane}arguments")),
                    "response.output_item.done" => self.close(&lane),
                    "response.completed" | "response.failed" | "response.incomplete" => { self.close(""); self.terminal = true; }
                    _ if kind.ends_with(".delta") => return Err(error("privacy_scan_failed")),
                    _ => {},
                }
            }
            ClientFormat::Gemini => {
                let candidates = value["candidates"].as_array().cloned().unwrap_or_default();
                for (position, candidate) in candidates.iter().enumerate() {
                    let index = candidate["index"].as_u64().unwrap_or(position as u64);
                    self.gemini_finishes.entry(index).or_insert(false);
                    let lane = format!("candidate:{index}:");
                    if let Some(parts) = candidate["content"]["parts"].as_array() {
                        for (part, value) in parts.iter().enumerate() {
                            let thought = value["thought"].as_bool().unwrap_or(false);
                            self.piece(&mut frame, format!("/candidates/{position}/content/parts/{part}/text"), format!("{lane}text:{thought}"), false, false)?;
                        }
                    }
                    if candidate["finishReason"].as_str().is_some_and(|reason| !reason.is_empty()) {
                        self.close(&lane);
                        self.gemini_finishes.insert(index, true);
                    }
                }
            }
        }
        let checked = self.context.inspect(frame.value.take().unwrap()).map_err(error)?;
        frame.changed = checked.report.secret_redactions != 0 || checked.report.personal_redactions != 0;
        self.report.merge(&checked.report);
        if let Some(code) = checked.blocked_code { return Err(error(code)); }
        frame.value = Some(checked.redacted);
        self.frames.push(frame);
        self.flush()
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        if self.lanes.values().any(|lane| !lane.closed && (lane.protected || !self.context.text_boundary_ready(&lane.text))) { return Ok(()); }
        let lanes = std::mem::take(&mut self.lanes);
        let mut clean = BTreeMap::new();
        for (key, lane) in lanes {
            if lane.json_required && !lane.text.is_empty() && serde_json::from_str::<Value>(&lane.text).is_err() {
                return Err(error("privacy_scan_failed"));
            }
            let checked = if lane.protected {
                self.inspect(json!({"tool_calls":[{"arguments":lane.text.as_str()}]}))?["tool_calls"][0]["arguments"].clone()
            } else { self.inspect(Value::String(lane.text.to_string()))? };
            let changed = checked.as_str() != Some(lane.text.as_str());
            clean.insert(key, changed.then_some(checked));
        }
        for mut frame in std::mem::take(&mut self.frames) {
            for piece in frame.pieces {
                let value = match clean.get_mut(&piece.lane).ok_or_else(|| error("privacy_scan_failed"))? {
                    None => Value::String(piece.original.to_string()),
                    Some(replacement) => {
                        frame.changed = true;
                        std::mem::replace(replacement, Value::String(String::new()))
                    }
                };
                *frame.value.as_mut().and_then(|frame| frame.pointer_mut(&piece.path)).ok_or_else(|| error("privacy_scan_failed"))? = value;
            }
            if !frame.changed {
                self.output.push_back(Bytes::copy_from_slice(&frame.original));
                continue;
            }
            let mut text = String::new();
            for (key, value) in frame.headers { text.push_str(&format!("{key}: {value}\n")); }
            text.push_str("data: ");
            text.push_str(&frame.value.map(|value| value.to_string()).unwrap_or_else(|| "[DONE]".into()));
            text.push_str("\n\n");
            self.output.push_back(Bytes::from(text));
        }
        self.persist()
    }

    fn frame(&mut self, bytes: &[u8]) -> Result<(), io::Error> {
        let text = std::str::from_utf8(bytes).map_err(|_| error("privacy_scan_failed"))?;
        let mut headers = Vec::new();
        let mut data = Vec::new();
        for line in text.lines() {
            if line.is_empty() { continue; }
            let (key, value) = line.split_once(':').unwrap_or((line, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            if key == "data" { data.push(value); }
            else {
                let checked = self.inspect(json!({"metadata":{key:value}}))?;
                let value = checked["metadata"][key].as_str().ok_or_else(|| error("privacy_scan_failed"))?;
                headers.push((key.to_owned(), value.to_owned()));
            }
        }
        let has_data = !data.is_empty();
        let data = data.join("\n");
        let done = has_data && data == "[DONE]";
        let value = if !has_data || done { None } else {
            Some(serde_json::from_str::<Value>(&data).map_err(|_| error("privacy_scan_failed"))?)
        };
        self.prepare(Frame { headers, value, pieces: Vec::new(), original: Zeroizing::new(bytes.to_vec()), changed: false, done })
    }

    fn push(&mut self, bytes: &[u8]) -> Result<(), io::Error> {
        self.total = self.total.saturating_add(bytes.len());
        if self.total > MAX_RESPONSE_BYTES { return Err(error("privacy_scan_limit")); }
        self.buffer.extend_from_slice(bytes);
        loop {
            let delimiter = self.buffer.windows(4).position(|window| window == b"\r\n\r\n").map(|index| (index, 4))
                .into_iter().chain(self.buffer.windows(2).position(|window| window == b"\n\n").map(|index| (index, 2)))
                .min_by_key(|(index, _)| *index);
            let Some((index, length)) = delimiter else { break; };
            if index > MAX_FRAME_BYTES { return Err(error("privacy_scan_limit")); }
            let frame = Zeroizing::new(self.buffer[..index + length].to_vec());
            self.buffer.drain(..index + length);
            self.frame(&frame)?;
        }
        if self.buffer.len() > MAX_FRAME_BYTES { return Err(error("privacy_scan_limit")); }
        Ok(())
    }
}

impl<S> Drop for ProtectedStream<S> {
    fn drop(&mut self) { let _ = self.persist(); self.buffer.zeroize(); }
}

pub(crate) fn protect<S, E>(upstream: S, format: ClientFormat, context: ResponsePrivacy, database: Database, event_id: Option<i64>)
    -> impl Stream<Item = Result<Bytes, io::Error>> + Send
where S: Stream<Item = Result<Bytes, E>> + Send + 'static, E: std::fmt::Display + Send + 'static {
    let state = ProtectedStream { upstream: Box::pin(upstream), context, format, database, event_id, buffer: Vec::new(), frames: Vec::new(),
        lanes: BTreeMap::new(), output: VecDeque::new(), report: Default::default(), failure: None, total: 0,
        terminal: false, gemini_finishes: BTreeMap::new(), eof: false, heartbeat_at: tokio::time::Instant::now() + INITIAL_HEARTBEAT_DELAY };
    stream::try_unfold(state, |mut state| async move {
        loop {
            if let Some(bytes) = state.output.pop_front() {
                state.heartbeat_at = tokio::time::Instant::now() + HEARTBEAT_INTERVAL;
                return Ok(Some((bytes, state)));
            }
            if state.eof { return Ok(None); }
            let next = tokio::select! {
                biased;
                _ = tokio::time::sleep_until(state.heartbeat_at) => {
                    state.heartbeat_at = tokio::time::Instant::now() + HEARTBEAT_INTERVAL;
                    return Ok(Some((Bytes::from_static(HEARTBEAT), state)));
                }
                next = state.upstream.next() => next,
            };
            let result = match next {
                Some(Ok(bytes)) => state.push(&bytes),
                Some(Err(_)) => return Err(io::Error::other("stream_interrupted")),
                None => {
                    state.eof = true;
                    let pending = if !state.buffer.is_empty() {
                        let bytes = Zeroizing::new(std::mem::take(&mut state.buffer));
                        state.frame(&bytes)
                    } else { Ok(()) };
                    pending.and_then(|_| {
                        if matches!(state.format, ClientFormat::Gemini) && !state.gemini_finishes.is_empty()
                            && state.gemini_finishes.values().all(|finished| *finished) { state.terminal = true; }
                        if !state.terminal { Err(error("privacy_scan_failed")) } else { state.close(""); state.flush() }
                    })
                }
            };
            if let Err(failure) = result {
                let code = match failure.to_string().as_str() {
                    "privacy_response_sensitive_structure" => "privacy_response_sensitive_structure",
                    "privacy_response_attachment_uninspected" => "privacy_response_attachment_uninspected",
                    "privacy_response_scan_limit" => "privacy_response_scan_limit",
                    _ => "privacy_response_scan_failed",
                };
                state.failure = Some(code);
                state.persist()?;
                return Err(io::Error::other(code));
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::privacy::PrivacySettings;

    fn event(value: Value) -> Vec<u8> { format!("data: {value}\n\n").into_bytes() }

    fn chat(text: &str) -> Vec<u8> {
        event(json!({"choices":[{"index":0,"delta":{"content":text},"finish_reason":null}]}))
    }

    fn chat_end() -> Vec<u8> {
        let mut bytes = event(json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}));
        bytes.extend_from_slice(b"data: [DONE]\n\n");
        bytes
    }

    #[tokio::test]
    async fn privacy_stream_safe_native_wire_keeps_all_frames_and_fragment_boundaries() {
        for format in [ClientFormat::Chat, ClientFormat::Responses, ClientFormat::Anthropic, ClientFormat::Gemini] {
            let mut wire = b": keepalive\r\nretry: 1000\r\nx-safe-extension: ordinary\r\n\r\n".to_vec();
            let events = match format {
                ClientFormat::Chat => vec![chat("ordinary"), chat(" text"), chat_end()],
                ClientFormat::Responses => vec![event(json!({"type":"response.output_text.delta","item_id":"item-1","delta":"ordinary"})),
                    event(json!({"type":"response.output_text.delta","item_id":"item-1","delta":" text"})), event(json!({"type":"response.completed"}))],
                ClientFormat::Anthropic => vec![event(json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ordinary"}})),
                    event(json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" text"}})), event(json!({"type":"message_stop"}))],
                ClientFormat::Gemini => vec![event(json!({"candidates":[{"index":0,"content":{"parts":[{"text":"ordinary"}]}}]})),
                    event(json!({"candidates":[{"index":0,"content":{"parts":[{"text":" text"}]},"finishReason":"STOP"}]}))],
            };
            for bytes in events {
                wire.extend_from_slice(String::from_utf8(bytes).unwrap().replace("data: {", "data:  {").replace('\n', "\r\n").as_bytes());
            }
            wire.extend_from_slice(b": final keepalive\n\n");
            let chunks = wire.chunks(7).map(<[u8]>::to_vec).collect();
            let (output, failure) = checked(format, chunks).await;
            assert_eq!(failure, None);
            assert_eq!(output.as_bytes(), wire);
        }
    }

    #[tokio::test]
    async fn privacy_stream_comments_and_unknown_fields_cannot_bypass_inspection() {
        for line in [": unit-upstream-key", "x-extension: unit-upstream-key", "unit-upstream-key: ordinary", "id: unit-upstream-key"] {
            let mut wire = format!("{line}\r\n\r\n").into_bytes();
            wire.extend_from_slice(&chat_end());
            let (output, failure) = checked(ClientFormat::Chat, wire.chunks(7).map(<[u8]>::to_vec).collect()).await;
            assert_eq!(failure.as_deref(), Some("privacy_response_sensitive_structure"));
            assert!(!output.contains("unit-upstream-key"));
        }
    }

    #[tokio::test]
    async fn privacy_stream_waiting_heartbeat_never_delivers_unchecked_fragments() {
        let upstream = stream::iter([Ok::<_, io::Error>(Bytes::from(chat("unit-up")))]).chain(stream::pending());
        let context = ResponsePrivacy::new(PrivacySettings::default(), &["unit-upstream-key"]);
        let mut output = Box::pin(protect(upstream, ClientFormat::Chat, context, crate::governance::test_support::database(), None));
        let bytes = tokio::time::timeout(Duration::from_secs(2), output.next()).await.unwrap().unwrap().unwrap();
        assert_eq!(bytes.as_ref(), HEARTBEAT);
        assert!(tokio::time::timeout(Duration::from_millis(30), output.next()).await.is_err());
    }

    async fn checked(format: ClientFormat, chunks: Vec<Vec<u8>>) -> (String, Option<String>) {
        let context = ResponsePrivacy::new(PrivacySettings::default(), &["unit-upstream-key"]);
        let upstream = stream::iter(chunks.into_iter().map(|chunk| Ok::<_, io::Error>(Bytes::from(chunk))));
        let mut protected = Box::pin(protect(upstream, format, context, crate::governance::test_support::database(), None));
        let mut output = String::new();
        while let Some(result) = protected.next().await {
            match result {
                Ok(bytes) => output.push_str(std::str::from_utf8(&bytes).unwrap()),
                Err(error) => return (output, Some(error.to_string())),
            }
        }
        (output, None)
    }

    fn values(output: &str) -> Vec<Value> {
        output.lines().filter_map(|line| line.strip_prefix("data: "))
            .filter(|data| *data != "[DONE]").map(|data| serde_json::from_str(data).unwrap()).collect()
    }

    #[tokio::test]
    async fn privacy_stream_chat_scans_network_and_event_boundaries() {
        let bytes = [chat("你好 unit-up"), chat("stream-key private@"), chat("example.test 13712345678 "), chat_end()].concat();
        let (output, failure) = checked(ClientFormat::Chat, bytes.chunks(1).map(<[u8]>::to_vec).collect()).await;
        assert_eq!(failure, None);
        for private in ["unit-upstream-key", "private@example.test", "13712345678"] { assert!(!output.contains(private)); }
        let text: String = values(&output).iter().filter_map(|value| value.pointer("/choices/0/delta/content").and_then(Value::as_str)).collect();
        assert_eq!(text, "你好 [REDACTED:SECRET] [REDACTED:EMAIL] [REDACTED:PHONE] ");
        assert!(output.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn privacy_stream_multiple_choices_keep_their_text() {
        let first = event(json!({"choices":[{"index":0,"delta":{"content":"left "}},{"index":1,"delta":{"content":"right "}}]}));
        let (output, failure) = checked(ClientFormat::Chat, vec![first, chat_end()]).await;
        assert_eq!(failure, None);
        let values = values(&output);
        assert_eq!(values[0]["choices"][0]["delta"]["content"], "left ");
        assert_eq!(values[0]["choices"][1]["delta"]["content"], "right ");
    }

    #[tokio::test]
    async fn privacy_stream_safe_tool_json_and_identity_survive_fragmentation() {
        let first = event(json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call-1","type":"function","function":{"name":"inspect","arguments":"{\"path\":"}}]}}]}));
        let second = event(json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"C:/project/main.rs\"}"}}]}}]}));
        let (output, failure) = checked(ClientFormat::Chat, vec![first, second, chat_end()]).await;
        assert_eq!(failure, None);
        let values = values(&output);
        assert_eq!(values[0]["choices"][0]["delta"]["tool_calls"][0]["id"], "call-1");
        assert_eq!(values[0]["choices"][0]["delta"]["tool_calls"][0]["function"]["name"], "inspect");
        let arguments: String = values.iter().filter_map(|value| value.pointer("/choices/0/delta/tool_calls/0/function/arguments").and_then(Value::as_str)).collect();
        assert_eq!(serde_json::from_str::<Value>(&arguments).unwrap(), json!({"path":"C:/project/main.rs"}));
    }

    #[tokio::test]
    async fn privacy_stream_escaped_sensitive_tool_fields_are_blocked() {
        let chunks = ["{\"pass\\u0077", "ord\":\"opaque-private\"}"].into_iter().map(|arguments|
            event(json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":arguments}}]}}]})))
            .chain([chat_end()]).collect();
        let (output, failure) = checked(ClientFormat::Chat, chunks).await;
        assert_eq!(failure.as_deref(), Some("privacy_response_sensitive_structure"));
        assert!(!output.contains("opaque-private"));
    }

    #[tokio::test]
    async fn privacy_stream_responses_content_done_keeps_other_lanes_open() {
        let delta = |index, text| event(json!({"type":"response.output_text.delta","item_id":"item-1","content_index":index,"delta":text}));
        let done = |index| event(json!({"type":"response.output_text.done","item_id":"item-1","content_index":index}));
        let chunks = vec![delta(0, "plain"), delta(1, "unit-up"), done(0), delta(1, "stream-key "), done(1), event(json!({"type":"response.completed","response":{"usage":{"input_tokens":4,"output_tokens":1}}}))];
        let (output, failure) = checked(ClientFormat::Responses, chunks).await;
        assert_eq!(failure, None);
        assert!(!output.contains("unit-upstream-key"));
        let values = values(&output);
        let text: String = values.iter().filter(|value| value["content_index"] == 1).filter_map(|value| value["delta"].as_str()).collect();
        assert_eq!(text, "[REDACTED:SECRET] ");
    }

    #[tokio::test]
    async fn privacy_stream_anthropic_signatures_wait_for_the_complete_block() {
        for (signature, expected) in [("safe-signature", None), ("unit-upstream-key", Some("privacy_response_sensitive_structure"))] {
            let chunks = vec![event(json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"ordinary thought","signature":""}})),
                event(json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":&signature[..5]}})),
                event(json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":&signature[5..]}})),
                event(json!({"type":"content_block_stop","index":0})), event(json!({"type":"message_stop"}))];
            let (output, failure) = checked(ClientFormat::Anthropic, chunks).await;
            assert_eq!(failure.as_deref(), expected);
            if expected.is_none() {
                let signature_text: String = values(&output).iter().filter_map(|value| value.pointer("/delta/signature").and_then(Value::as_str)).collect();
                assert_eq!(signature_text, signature);
                assert!(output.contains("ordinary thought"));
            } else { assert!(!output.contains(signature)); }
        }
    }

    #[tokio::test]
    async fn privacy_stream_gemini_parts_share_scanning_and_keep_usage_after_finish() {
        let chunks = vec![event(json!({"candidates":[{"index":0,"content":{"parts":[{"text":"unit-up"},{"text":"stream-key "}]}}]})),
            event(json!({"candidates":[{"index":0,"finishReason":"STOP"}]})),
            event(json!({"usageMetadata":{"promptTokenCount":4,"candidatesTokenCount":1,"totalTokenCount":5}}))];
        let (output, failure) = checked(ClientFormat::Gemini, chunks).await;
        assert_eq!(failure, None);
        assert!(!output.contains("unit-upstream-key"));
        assert!(output.contains("[REDACTED:SECRET]"));
        assert_eq!(values(&output).last().unwrap()["usageMetadata"]["totalTokenCount"], 5);
    }

    #[test]
    fn privacy_stream_private_keys_require_complete_matching_end_markers() {
        let context = ResponsePrivacy::new(PrivacySettings::default(), &[]);
        let first = "-----BEGIN PRIVATE KEY-----\nfirst\n-----END PRIVATE KEY-----\n";
        for tail in ["-----BEGIN RSA PRIVATE KEY-----\nsecond\n-----END ", "-----BEGIN RSA PRIVATE KEY-----\nsecond\n-----END PRIVATE KEY-----\n"] {
            assert!(!context.text_boundary_ready(&format!("{first}{tail}")));
        }
        assert!(context.text_boundary_ready(&format!("{first}-----BEGIN RSA PRIVATE KEY-----\nsecond\n-----END RSA PRIVATE KEY-----\n")));
    }

    #[tokio::test]
    async fn privacy_stream_invalid_eof_unknown_deltas_and_size_limits_fail_closed() {
        for (format, chunks, expected) in [
            (ClientFormat::Responses, vec![b"data: {\"type\":\"response.unknown.delta\",\"delta\":\"unit-upstream-key\"}".to_vec()], "privacy_response_scan_failed"),
            (ClientFormat::Responses, vec![event(json!({"type":"response.audio.delta","delta":"unit-upstream-key"}))], "privacy_response_scan_failed"),
            (ClientFormat::Chat, vec![chat("unit-up")], "privacy_response_scan_failed"),
            (ClientFormat::Chat, vec![vec![b'a'; MAX_FRAME_BYTES + 1]], "privacy_response_scan_limit"),
        ] {
            let (output, failure) = checked(format, chunks).await;
            assert_eq!(failure.as_deref(), Some(expected));
            assert!(!output.contains("unit-upstream-key"));
        }
    }

    #[tokio::test]
    async fn privacy_stream_anthropic_event_headers_can_supply_event_type() {
        let make = |kind: &str, value: Value| format!("event: {kind}\ndata: {value}\n\n").into_bytes();
        let chunks = vec![make("content_block_delta", json!({"index":0,"delta":{"type":"text_delta","text":"ordinary text"}})),
            make("content_block_stop", json!({"index":0})), make("message_stop", json!({}))];
        let (output, failure) = checked(ClientFormat::Anthropic, chunks).await;
        assert_eq!(failure, None);
        assert!(output.contains("ordinary text"));
        assert!(output.contains("event: message_stop"));
    }
}
