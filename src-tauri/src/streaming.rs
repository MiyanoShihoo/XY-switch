use axum::body::Bytes;
use futures_util::{Stream, StreamExt, stream};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    io,
    pin::Pin,
};

const MAX_SSE_FRAME_BYTES: usize = 2 * 1024 * 1024;

/// Observes upstream bytes before conversion, including native protocol passthrough.
/// It never changes a frame, retains no response text, and persists partial observations
/// on transport failure or client cancellation as well as normal EOF.
pub(crate) mod audit_capture {
    use super::*;
    use crate::model_audit::{Observation, StreamAccumulator};
    use crate::shadow_audit_runtime::ShadowCapture;
    use sha2::{Digest, Sha256};
    use std::time::Instant;

    pub(crate) struct Collector {
        pub observation: Observation,
        format: UpstreamFormat,
        stream: StreamAccumulator,
        finishes: BTreeMap<u64, bool>,
        terminal: bool,
        invalid: bool,
        cache_read: Option<u64>,
        cache_write: Option<u64>,
        saw_content: bool,
        first_frame_ms: Option<u64>,
        empty_tool_inputs: BTreeMap<u64, u64>,
        output_sha: Sha256,
        shadow_output_plain: bool,
        final_usage_seen: bool,
    }

    fn number(value: &Value, paths: &[&str]) -> Option<u64> {
        paths
            .iter()
            .find_map(|path| value.pointer(path).and_then(Value::as_u64))
    }

    fn cache_number(value: &Value, paths: &[&str]) -> Option<u64> {
        // Gateways may report multiple aliases. Every reported hit matters;
        // a zero in a higher-priority field must never hide another positive.
        // No recognized value remains unknown, rather than becoming zero.
        paths
            .iter()
            .filter_map(|path| value.pointer(path).and_then(Value::as_u64))
            .max()
    }

    impl Collector {
        pub(crate) fn new(format: UpstreamFormat, mut observation: Observation) -> Self {
            // A fresh upstream response must never inherit a previous declaration.
            observation.upstream_model = None;
            observation.upstream_models.clear();
            observation.model_evidence_truncated = false;
            observation.visible_output_bytes = Some(0);
            observation.tool_payload_bytes = Some(0);
            observation.text_only = true;
            observation.complete = false;
            observation.protocol = Some(match format { UpstreamFormat::OpenAi => "chat", UpstreamFormat::Responses => "responses", UpstreamFormat::Anthropic => "anthropic", UpstreamFormat::Gemini => "gemini" }.into());
            Self {
                observation,
                format,
                stream: StreamAccumulator::default(),
                finishes: BTreeMap::new(),
                terminal: false,
                invalid: false,
                cache_read: None,
                cache_write: None,
                saw_content: false,
                first_frame_ms: None,
                empty_tool_inputs: BTreeMap::new(),
                output_sha: Sha256::new(),
                shadow_output_plain: true,
                final_usage_seen: false,
            }
        }

        pub(crate) fn capture_headers(&mut self, headers: &reqwest::header::HeaderMap) {
            if self.observation.verification_capture.as_ref().is_some_and(|c| c.store_diagnostic_ids) {
                self.observation.provider_request_id = ["x-request-id", "request-id", "x-goog-request-id"]
                    .iter().find_map(|key| headers.get(*key).and_then(|v| v.to_str().ok()).and_then(crate::model_verification::safe_id));
            }
        }

        fn metadata(&mut self, value: &Value) {
            let kind = value["type"].as_str().unwrap_or("");
            let shape = if value["choices"].is_array() { Some("chat") }
                else if value["candidates"].is_array() || value["usageMetadata"].is_object() { Some("gemini") }
                else if value["output"].is_array() || kind.starts_with("response.") { Some("responses") }
                else if value["content"].is_array() || matches!(kind, "message_start" | "message_delta" | "message_stop" | "content_block_start" | "content_block_delta") { Some("anthropic") }
                else { None };
            if let Some(shape) = shape {
                let shape = if self.observation.protocol_shape.as_deref().is_some_and(|old| old != shape) { "mixed" } else { shape };
                self.observation.protocol_shape = Some(shape.into());
            }
            if self.observation.verification_capture.as_ref().is_some_and(|c| c.store_diagnostic_ids) {
                if let Some(id) = value.get("id").or_else(|| value.get("responseId")).and_then(Value::as_str).and_then(crate::model_verification::safe_id) {
                    self.observation.response_id = Some(id);
                }
            }
        }

        fn finish_reason(&mut self, reason: Option<&str>) {
            if let Some(reason) = reason.and_then(crate::model_verification::safe_id) {
                self.observation.finish_reason = Some(reason);
            }
        }

        fn model(&mut self, value: &Value) {
            self.metadata(value);
            if let Some(model) = value
                .get("model")
                .or_else(|| value.get("modelVersion"))
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
            {
                if self
                    .observation
                    .upstream_model
                    .as_deref()
                    .is_some_and(|previous| previous != model)
                {
                    self.shadow_output_plain = false;
                }
                // Only typed, bounded model identifiers may enter diagnostic storage.
                if crate::model_verification::safe_id(model).is_none() {
                    self.observation.model_evidence_truncated = true;
                    self.shadow_output_plain = false;
                    return;
                }
                if !self
                    .observation
                    .upstream_models
                    .iter()
                    .any(|seen| seen == model)
                {
                    if self.observation.upstream_models.len() < 16 {
                        self.observation.upstream_models.push(model.to_owned());
                    } else {
                        self.observation.model_evidence_truncated = true;
                    }
                }
                self.observation.upstream_model = Some(model.to_owned());
            }
        }

        fn usage(&mut self, usage: &Value) {
            if let Some(n) = number(
                usage,
                &["/prompt_tokens", "/input_tokens", "/promptTokenCount"],
            ) {
                self.observation.input_tokens = Some(n);
            }
            if let Some(n) = number(
                usage,
                &[
                    "/completion_tokens",
                    "/output_tokens",
                    "/candidatesTokenCount",
                ],
            ) {
                self.observation.output_tokens = Some(n);
            }
            if let Some(n) = number(usage, &["/total_tokens", "/totalTokenCount"]) {
                self.observation.total_tokens = Some(n);
            }
            if let Some(n) = number(
                usage,
                &[
                    "/completion_tokens_details/reasoning_tokens",
                    "/output_tokens_details/reasoning_tokens",
                    "/reasoning_tokens",
                    "/thoughtsTokenCount",
                ],
            ) {
                self.observation.reasoning_tokens = Some(n);
                // Gemini reports visible candidates and hidden thoughts separately.
                if matches!(self.format, UpstreamFormat::Gemini) {
                    self.observation.output_tokens = usage
                        .get("candidatesTokenCount")
                        .and_then(Value::as_u64)
                        .and_then(|visible| visible.checked_add(n));
                }
            }
            let cache_read = cache_number(
                usage,
                &[
                    "/cache_read_input_tokens",
                    "/prompt_tokens_details/cached_tokens",
                    "/input_tokens_details/cached_tokens",
                    "/cachedContentTokenCount",
                    "/prompt_cache_hit_tokens",
                ],
            );
            // A later zero must not erase a cache hit observed earlier in a stream.
            self.cache_read = self.cache_read.into_iter().chain(cache_read).max();
            let cache_write = cache_number(
                usage,
                &[
                    "/cache_creation_input_tokens",
                    "/input_tokens_details/cache_write_tokens",
                    "/prompt_tokens_details/cache_write_tokens",
                ],
            );
            self.cache_write = self.cache_write.into_iter().chain(cache_write).max();
            // Additional tool-use prompt accounting is not comparable to ordinary prompt usage.
            if number(usage, &["/toolUsePromptTokenCount"]).is_some_and(|n| n > 0) {
                self.observation.total_tokens = None;
                self.shadow_output_plain = false;
            }
        }

        fn text(&mut self, value: &Value, elapsed_ms: Option<u64>) {
            if let Some(text) = value.as_str() {
                self.output_sha.update(text.as_bytes());
                self.saw_content = true;
                self.observation.visible_output_bytes = self
                    .observation
                    .visible_output_bytes
                    .and_then(|n| n.checked_add(text.len() as u64));
                if let Some(ms) = elapsed_ms {
                    self.stream.observe(text, ms);
                }
            } else if !value.is_null() {
                self.observation.text_only = false;
            }
        }

        fn tool(&mut self, value: &Value) {
            self.shadow_output_plain = false;
            let bytes = if let Some(s) = value.as_str() {
                s.len()
            } else if value.is_null() {
                0
            } else {
                value.to_string().len()
            };
            self.saw_content = true;
            self.observation.tool_payload_bytes = self
                .observation
                .tool_payload_bytes
                .and_then(|n| n.checked_add(bytes as u64));
        }

        fn chat_message(&mut self, message: &Value, elapsed_ms: Option<u64>) {
            // Match the converter's empty-placeholder handling while retaining
            // ordinary usage/output accounting for explicit reasoning replies.
            if message["reasoning_content"]
                .as_str()
                .is_some_and(|text| !text.is_empty())
            {
                self.shadow_output_plain = false;
            }
            if message["role"]
                .as_str()
                .is_some_and(|role| role != "assistant")
            {
                self.shadow_output_plain = false;
            }
            let content = &message["content"];
            if let Some(parts) = content.as_array() {
                for part in parts {
                    match part["type"].as_str() {
                        Some("text" | "output_text") => self.text(&part["text"], elapsed_ms),
                        _ => self.observation.text_only = false,
                    }
                }
            } else {
                self.text(content, elapsed_ms);
            }
            self.text(&message["refusal"], elapsed_ms);
            for call in message["tool_calls"].as_array().into_iter().flatten() {
                if call
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind != "function")
                {
                    self.observation.text_only = false;
                }
                self.tool(&call["function"]["name"]);
                self.tool(&call["function"]["arguments"]);
            }
            if let Some(call) = message.get("function_call") {
                self.tool(&call["name"]);
                self.tool(&call["arguments"]);
            }
            if message.get("audio").is_some_and(|v| !v.is_null()) {
                self.observation.text_only = false;
            }
        }

        fn blocks(&mut self, blocks: &Value) {
            for block in blocks.as_array().into_iter().flatten() {
                match block["type"].as_str() {
                    Some("text" | "output_text") => self.text(&block["text"], None),
                    Some("refusal") => self.text(&block["refusal"], None),
                    Some("tool_use") => {
                        self.tool(&block["name"]);
                        self.tool(&block["input"]);
                    }
                    Some("thinking" | "redacted_thinking") => self.shadow_output_plain = false,
                    _ => self.observation.text_only = false,
                }
            }
        }

        fn responses_output(&mut self, output: &Value) {
            for item in output.as_array().into_iter().flatten() {
                match item["type"].as_str() {
                    Some("message") => {
                        if item["role"]
                            .as_str()
                            .is_some_and(|role| role != "assistant")
                            || item["status"]
                                .as_str()
                                .is_some_and(|status| status != "completed")
                        {
                            self.shadow_output_plain = false;
                        }
                        self.blocks(&item["content"]);
                    }
                    Some("function_call") => {
                        self.tool(&item["name"]);
                        self.tool(&item["arguments"]);
                    }
                    Some("reasoning") => self.shadow_output_plain = false,
                    _ => self.observation.text_only = false,
                }
            }
        }

        fn gemini(&mut self, value: &Value, elapsed_ms: Option<u64>) {
            self.model(value);
            self.usage(&value["usageMetadata"]);
            for (position, candidate) in value["candidates"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                let index = candidate["index"].as_u64().unwrap_or(position as u64);
                self.finishes.entry(index).or_insert(false);
                for part in candidate["content"]["parts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    if part["thought"] == true {
                        self.shadow_output_plain = false;
                        continue;
                    }
                    if part.get("text").is_some() {
                        self.text(&part["text"], elapsed_ms);
                    } else if let Some(call) = part.get("functionCall") {
                        self.tool(&call["name"]);
                        self.tool(&call["args"]);
                    } else {
                        self.observation.text_only = false;
                    }
                }
                if let Some(reason) = candidate["finishReason"].as_str() {
                    self.finish_reason(Some(reason));
                    self.finishes.insert(index, reason == "STOP");
                    self.terminal = true;
                }
            }
        }

        #[cfg(test)]
        pub(crate) fn non_stream(self, value: &Value) -> Observation {
            self.capture_non_stream(value).observation
        }

        pub(crate) fn capture_non_stream(mut self, value: &Value) -> Self {
            if value.get("error").is_some_and(|v| !v.is_null()) {
                self.invalid = true;
                self.complete(false);
                return self;
            }
            self.model(value);
            self.usage(&value["usage"]);
            if value.get("error").is_some_and(|v| !v.is_null()) {
                self.invalid = true;
            }
            match self.format {
                UpstreamFormat::OpenAi => {
                    for (i, choice) in value["choices"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .enumerate()
                    {
                        self.chat_message(&choice["message"], None);
                        self.finish_reason(choice["finish_reason"].as_str());
                        if choice["finish_reason"] != "stop" {
                            self.shadow_output_plain = false;
                        }
                        self.finishes.insert(
                            i as u64,
                            matches!(
                                choice["finish_reason"].as_str(),
                                Some("stop" | "tool_calls" | "function_call")
                            ),
                        );
                    }
                    self.terminal = true;
                }
                UpstreamFormat::Responses => {
                    self.responses_output(&value["output"]);
                    self.finish_reason(value["status"].as_str());
                    self.terminal = value["status"] == "completed";
                }
                UpstreamFormat::Anthropic => {
                    self.finish_reason(value["stop_reason"].as_str());
                    if value["stop_reason"] == "tool_use" {
                        self.shadow_output_plain = false;
                    }
                    self.blocks(&value["content"]);
                    self.terminal = matches!(
                        value["stop_reason"].as_str(),
                        Some("end_turn" | "tool_use" | "stop_sequence")
                    );
                }
                UpstreamFormat::Gemini => self.gemini(value, None),
            }
            self.final_usage_seen = self.observation.input_tokens.is_some() && self.observation.output_tokens.is_some();
            self.complete(false);
            self
        }

        pub(crate) fn publish_shadow(
            &self,
            database: &crate::database::Database,
            event_id: Option<i64>,
            capture: Option<ShadowCapture>,
        ) {
            let Some(capture) = capture else {
                return;
            };
            let plain_complete = self.shadow_plain_complete();
            let cache_creation = if matches!(self.format, UpstreamFormat::Anthropic) {
                self.cache_write
            } else {
                self.cache_write.or(self.cache_read.map(|_| 0))
            };
            database.shadow.submit(
                capture,
                event_id,
                self.observation.input_tokens,
                self.cache_read,
                cache_creation,
                plain_complete,
                false,
                plain_complete.then(|| format!("{:x}", self.output_sha.clone().finalize())),
                self.observation.upstream_model.as_deref(),
            );
        }

        fn shadow_plain_complete(&self) -> bool {
            self.shadow_output_plain
                && self.observation.complete
                && self.observation.text_only
                && self.observation.tool_payload_bytes == Some(0)
                && self.finishes.len() <= 1
        }

        fn frame(&mut self, frame: &[u8], elapsed_ms: u64) {
            let Ok((event, data)) = parse_frame(frame) else {
                self.invalid = true;
                return;
            };
            if data.is_empty() {
                return;
            }
            self.first_frame_ms.get_or_insert(elapsed_ms);
            if data == "[DONE]" {
                if matches!(self.format, UpstreamFormat::OpenAi) {
                    self.terminal = true;
                }
                return;
            }
            let Ok(value) = serde_json::from_str::<Value>(&data) else {
                self.invalid = true;
                return;
            };
            if value.get("error").is_some_and(|v| !v.is_null()) {
                self.invalid = true;
                return;
            }
            self.model(&value);
            self.usage(&value["usage"]);
            let kind = value["type"].as_str().or(event.as_deref()).unwrap_or("");
            match self.format {
                UpstreamFormat::OpenAi => {
                    for (i, choice) in value["choices"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .enumerate()
                    {
                        let index = choice["index"].as_u64().unwrap_or(i as u64);
                        self.finishes.entry(index).or_insert(false);
                        self.chat_message(&choice["delta"], Some(elapsed_ms));
                        if let Some(reason) = choice["finish_reason"].as_str() {
                            self.finish_reason(Some(reason));
                            if reason != "stop" {
                                self.shadow_output_plain = false;
                            }
                            self.finishes.insert(
                                index,
                                matches!(reason, "stop" | "tool_calls" | "function_call"),
                            );
                        }
                    }
                }
                UpstreamFormat::Responses => {
                    self.model(&value["response"]);
                    self.usage(&value["response"]["usage"]);
                    match kind {
                        "response.output_text.delta" => {
                            self.text(&value["delta"], Some(elapsed_ms))
                        }
                        "response.refusal.delta" => self.text(&value["delta"], Some(elapsed_ms)),
                        "response.function_call_arguments.delta" => self.tool(&value["delta"]),
                        "response.output_item.added" => match value["item"]["type"].as_str() {
                            Some("function_call") => {
                                self.tool(&value["item"]["name"]);
                                self.tool(&value["item"]["arguments"]);
                            }
                            Some("message") => {}
                            Some("reasoning") => self.shadow_output_plain = false,
                            _ => self.observation.text_only = false,
                        },
                        "response.completed" | "response.done" => {
                            self.finish_reason(Some("completed"));
                            self.terminal = value["response"]["status"]
                                .as_str()
                                .is_none_or(|s| s == "completed");
                            // Final snapshots validate coverage, but must never be counted twice.
                            if value["response"]["output"].is_array() {
                                let mut snapshot = Self::new(self.format, Observation::default());
                                snapshot.responses_output(&value["response"]["output"]);
                                if !snapshot.shadow_output_plain
                                    || snapshot.output_sha.clone().finalize()
                                        != self.output_sha.clone().finalize()
                                {
                                    self.shadow_output_plain = false;
                                }
                                if snapshot.observation.visible_output_bytes
                                    != self.observation.visible_output_bytes
                                    || snapshot.observation.tool_payload_bytes
                                        != self.observation.tool_payload_bytes
                                    || !snapshot.observation.text_only
                                {
                                    self.observation.text_only = false;
                                }
                            }
                        }
                        "response.failed" | "response.incomplete" | "error" => self.invalid = true,
                        _ => {}
                    }
                }
                UpstreamFormat::Anthropic => match kind {
                    "message_start" => {
                        self.model(&value["message"]);
                        self.usage(&value["message"]["usage"]);
                    }
                    "content_block_start" => {
                        let block = &value["content_block"];
                        match block["type"].as_str() {
                            Some("text") => self.text(&block["text"], Some(elapsed_ms)),
                            Some("tool_use") => {
                                self.tool(&block["name"]);
                                self.tool(&block["input"]);
                                // Count empty arguments unless subsequent JSON deltas
                                // replace the protocol's initial object placeholder.
                                if block["input"].as_object().is_some_and(|v| v.is_empty()) {
                                    self.empty_tool_inputs
                                        .insert(value["index"].as_u64().unwrap_or(0), 2);
                                }
                            }
                            Some("thinking" | "redacted_thinking") => {
                                self.shadow_output_plain = false
                            }
                            _ => self.observation.text_only = false,
                        }
                    }
                    "content_block_delta" => match value["delta"]["type"].as_str() {
                        Some("text_delta") => self.text(&value["delta"]["text"], Some(elapsed_ms)),
                        Some("input_json_delta") => {
                            if value["delta"]["partial_json"]
                                .as_str()
                                .is_some_and(|value| !value.is_empty())
                                && let Some(placeholder) = self
                                    .empty_tool_inputs
                                    .remove(&value["index"].as_u64().unwrap_or(0))
                            {
                                self.observation.tool_payload_bytes = self
                                    .observation
                                    .tool_payload_bytes
                                    .and_then(|n| n.checked_sub(placeholder));
                            }
                            self.tool(&value["delta"]["partial_json"]);
                        }
                        Some("thinking_delta" | "signature_delta") => {
                            self.shadow_output_plain = false
                        }
                        _ => self.observation.text_only = false,
                    },
                    "message_delta" => {
                        self.finish_reason(value["delta"]["stop_reason"].as_str());
                        if value["delta"]["stop_reason"] == "tool_use" {
                            self.shadow_output_plain = false;
                        }
                        self.finishes.insert(
                            0,
                            matches!(
                                value["delta"]["stop_reason"].as_str(),
                                Some("end_turn" | "tool_use" | "stop_sequence")
                            ),
                        );
                    }
                    "message_stop" => self.terminal = true,
                    "error" => self.invalid = true,
                    _ => {}
                },
                UpstreamFormat::Gemini => self.gemini(&value, Some(elapsed_ms)),
            }
            let final_usage = match self.format {
                UpstreamFormat::Anthropic => kind == "message_delta" && value["usage"]["output_tokens"].is_u64(),
                UpstreamFormat::Responses => matches!(kind, "response.completed" | "response.done") && value["response"]["usage"].is_object(),
                UpstreamFormat::OpenAi => !self.finishes.is_empty() && self.finishes.values().all(|v| *v) && value["usage"].is_object(),
                UpstreamFormat::Gemini => self.terminal && value["usageMetadata"].is_object(),
            };
            self.final_usage_seen |= final_usage;
        }

        /// Shared incremental decoder for passthrough requests and single-shot probes.
        pub(crate) fn push_sse(&mut self, buffer: &mut Vec<u8>, bytes: &[u8], elapsed_ms: u64) {
            if self.invalid {
                return;
            }
            buffer.extend_from_slice(bytes);
            while let Some(frame) = take_frame(buffer) {
                if frame.len() > MAX_SSE_FRAME_BYTES {
                    self.invalid = true;
                    break;
                }
                self.frame(&frame, elapsed_ms);
            }
            if buffer.len() > MAX_SSE_FRAME_BYTES {
                self.invalid = true;
            }
            if self.invalid {
                buffer.clear();
            }
        }

        pub(crate) fn finish_stream(&mut self, clean_eof: bool) {
            if !clean_eof {
                self.invalid = true;
            }
            self.complete(true);
            let mut stream = std::mem::take(&mut self.stream).finish();
            stream.ttfb_ms = self.first_frame_ms;
            self.observation.stream = Some(stream);
        }

        pub(crate) fn request_usage(&self, fallback_model: &str) -> crate::database::RequestUsage {
            let observation = self.observation.clone();
            let model = observation
                .upstream_model
                .clone()
                .unwrap_or_else(|| fallback_model.to_owned());
            let input = observation.input_tokens.and_then(|n| i64::try_from(n).ok());
            let output = observation
                .output_tokens
                .and_then(|n| i64::try_from(n).ok());
            let cached = self.cache_read.and_then(|n| i64::try_from(n).ok());
            let cost = crate::session_usage_codex::estimate_codex_cost(
                &model,
                input.unwrap_or(0),
                output.unwrap_or(0),
                cached.unwrap_or(0),
            );
            crate::database::RequestUsage {
                model: Some(model),
                input_tokens: input,
                output_tokens: output,
                cache_read_tokens: cached,
                cache_creation_tokens: self.cache_write.and_then(|n| i64::try_from(n).ok()),
                cost: (cost > 0.0).then_some(cost),
                price_source: (cost > 0.0).then(|| "standard".to_owned()),
                price_version: (cost > 0.0).then(|| "v1".to_owned()),
                audit: Some(crate::model_audit::audit(observation)),
            }
        }

        fn complete(&mut self, streaming: bool) {
            self.observation.streaming = streaming;
            self.observation.parse_invalid = self.invalid;
            self.observation.cache_read_tokens = self.cache_read;
            self.observation.cache_creation_tokens = self.cache_write;
            let requires_finish =
                matches!(self.format, UpstreamFormat::OpenAi | UpstreamFormat::Gemini)
                    || (streaming && matches!(self.format, UpstreamFormat::Anthropic));
            self.observation.complete = self.terminal
                && !self.invalid
                && (!requires_finish
                    || (!self.finishes.is_empty() && self.finishes.values().all(|v| *v)));
            self.observation.usage_final = self.observation.complete && self.final_usage_seen;
            if !self.saw_content {
                self.observation.text_only = false;
            }
            if self.finishes.len() > 1 {
                self.observation.max_tokens = None;
            }
        }
    }

    struct Observed<S> {
        upstream: Pin<Box<S>>,
        collector: Collector,
        buffer: Vec<u8>,
        started: Instant,
        database: crate::database::Database,
        event_id: Option<i64>,
        model: String,
        persisted: bool,
        eof: bool,
        shadow: Option<ShadowCapture>,
    }

    impl<S> Observed<S> {
        fn persist(&mut self) {
            if self.persisted {
                return;
            }
            self.persisted = true;
            self.collector
                .finish_stream(self.eof && self.buffer.is_empty());
            self.collector
                .publish_shadow(&self.database, self.event_id, self.shadow.take());
            let usage = self.collector.request_usage(&self.model);
            if let Some(id) = self.event_id {
                if let Err(error) = self.database.update_request_usage(id, &usage) {
                    eprintln!("failed to persist stream audit for event {id}: {error}");
                }
            }
        }
    }

    impl<S> Drop for Observed<S> {
        fn drop(&mut self) {
            self.persist();
        }
    }

    #[cfg(test)]
    pub(crate) fn observe_stream<S>(
        upstream: S,
        format: UpstreamFormat,
        observation: Observation,
        model: String,
        database: crate::database::Database,
        event_id: Option<i64>,
        started: Instant,
    ) -> impl Stream<Item = Result<Bytes, reqwest::Error>> + Send
    where
        S: Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
    {
        observe_stream_with_shadow(
            upstream,
            format,
            observation,
            model,
            database,
            event_id,
            started,
            None,
        )
    }

    pub(crate) fn observe_stream_with_shadow<S>(
        upstream: S,
        format: UpstreamFormat,
        observation: Observation,
        model: String,
        database: crate::database::Database,
        event_id: Option<i64>,
        started: Instant,
        shadow: Option<ShadowCapture>,
    ) -> impl Stream<Item = Result<Bytes, reqwest::Error>> + Send
    where
        S: Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
    {
        let state = Observed {
            upstream: Box::pin(upstream),
            collector: Collector::new(format, observation),
            buffer: Vec::new(),
            started,
            database,
            event_id,
            model,
            persisted: false,
            eof: false,
            shadow,
        };
        stream::unfold(state, |mut state| async move {
            match state.upstream.next().await {
                Some(Ok(bytes)) => {
                    let elapsed_ms =
                        state.started.elapsed().as_millis().min(u64::MAX as u128) as u64;
                    state
                        .collector
                        .push_sse(&mut state.buffer, &bytes, elapsed_ms);
                    Some((Ok(bytes), state))
                }
                Some(Err(error)) => {
                    state.collector.invalid = true;
                    state.persist();
                    Some((Err(error), state))
                }
                None => {
                    state.eof = true;
                    state.persist();
                    None
                }
            }
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn response_model_declarations_are_bounded_and_never_inherited() {
            let previous = Observation {
                upstream_model: Some("old-model".into()),
                upstream_models: vec!["old-model".into()],
                ..Default::default()
            };
            let mut collector = Collector::new(UpstreamFormat::OpenAi, previous);
            assert!(collector.observation.upstream_model.is_none());
            assert!(collector.observation.upstream_models.is_empty());
            for i in 0..30 {
                collector.model(&json!({"model":format!("model-{i}")}));
            }
            assert_eq!(collector.observation.upstream_models.len(), 16);
            assert!(collector.observation.model_evidence_truncated);
            collector.model(&json!({"model":"x".repeat(4096)}));
            assert!(collector.observation.upstream_model.as_ref().unwrap().len() <= 256);
            assert_eq!(
                crate::model_audit::audit(collector.observation).model_status,
                "conflicting"
            );
        }

        fn collect(format: UpstreamFormat, frames: &[Value]) -> Collector {
            let mut collector = Collector::new(format, Observation::default());
            for frame in frames {
                collector.frame(format!("data: {frame}").as_bytes(), 12);
            }
            collector.complete(true);
            collector
        }

        #[test]
        fn model_audit_stream_snapshots_do_not_double_count_and_reasoning_is_not_visible() {
            let frames = [
                json!({"type":"response.created","response":{"model":"upstream"}}),
                json!({"type":"response.reasoning_summary_text.delta","delta":"hidden"}),
                json!({"type":"response.output_text.delta","delta":"你好1234"}),
                json!({"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","name":"lookup","arguments":""}}),
                json!({"type":"response.function_call_arguments.delta","output_index":1,"delta":"{\"city\":\"杭州\"}"}),
                json!({"type":"response.function_call_arguments.done","output_index":1,"arguments":"{\"city\":\"杭州\"}"}),
                json!({"type":"response.output_text.done","text":"你好1234"}),
                json!({"type":"response.completed","response":{"status":"completed","model":"upstream-final",
                    "output":[{"type":"message","content":[{"type":"output_text","text":"你好1234"}]},{"type":"function_call","name":"lookup","arguments":"{\"city\":\"杭州\"}"}],
                    "usage":{"input_tokens":2,"output_tokens":8,"total_tokens":10,"output_tokens_details":{"reasoning_tokens":3}}}}),
            ];
            let collector = collect(UpstreamFormat::Responses, &frames);
            assert_eq!(collector.observation.visible_output_bytes, Some(10));
            assert_eq!(
                collector.observation.tool_payload_bytes,
                Some(("lookup".len() + "{\"city\":\"杭州\"}".len()) as u64)
            );
            assert_eq!(
                collector.observation.upstream_model.as_deref(),
                Some("upstream-final")
            );
            assert!(collector.observation.complete && collector.observation.text_only);
            assert_eq!(
                collector.observation.upstream_models,
                ["upstream", "upstream-final"]
            );
            let report = crate::model_audit::audit(collector.observation);
            assert_eq!(report.model_status, "conflicting");
            assert_eq!(report.flags, ["inv.model_changed"]);
            let stats = collector.stream.finish();
            assert_eq!(stats.text_delta_count, 1);
            assert_eq!(stats.max_digit_run, 4);
            assert_eq!(stats.max_han_run, 2);
        }

        #[test]
        fn model_audit_stream_all_choices_and_batch_arrival_are_observed() {
            let mut collector = collect(
                UpstreamFormat::OpenAi,
                &[
                    json!({"model":"native","choices":[{"index":0,"delta":{"content":"1234"}},{"index":1,"delta":{"content":"你好"}}]}),
                    json!({"choices":[{"index":0,"delta":{"content":" "},"finish_reason":"stop"},{"index":1,"delta":{"content":" "},"finish_reason":"stop"}]}),
                    json!({"choices":[],"usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3,"completion_tokens_details":{"reasoning_tokens":0}}}),
                ],
            );
            collector.frame(b"data: [DONE]", 12);
            collector.complete(true);
            assert!(collector.observation.complete);
            assert_eq!(collector.observation.visible_output_bytes, Some(12));
            assert_eq!(collector.observation.max_tokens, None);
            let stats = collector.stream.finish();
            assert_eq!(stats.text_delta_count, 4);
            assert!(stats.batched);
        }

        #[test]
        fn model_audit_native_anthropic_and_gemini_streams_keep_usage_presence() {
            let anthropic = collect(
                UpstreamFormat::Anthropic,
                &[
                    json!({"type":"message_start","message":{"model":"claude","usage":{"input_tokens":2}}}),
                    json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                    json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"你好"}}),
                    json!({"type":"content_block_delta","index":1,"delta":{"type":"thinking_delta","thinking":"hidden"}}),
                    json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":20}}),
                    json!({"type":"message_stop"}),
                ],
            );
            assert!(anthropic.observation.complete);
            assert_eq!(anthropic.observation.visible_output_bytes, Some(6));
            assert_eq!(anthropic.observation.input_tokens, Some(2));
            assert!(anthropic.observation.reasoning_tokens.is_none());
            let audit = crate::model_audit::audit(anthropic.observation);
            assert!(!audit.flags.contains(&"inv.byte_bound".into()));
            assert!(audit.gaps.contains(&"byte_bound.reasoning_unknown".into()));

            let gemini = collect(
                UpstreamFormat::Gemini,
                &[
                    json!({"modelVersion":"gemini-native","candidates":[{"index":0,"content":{"parts":[{"text":"你好"},{"text":"hidden","thought":true}]},"finishReason":"STOP"}],
                    "usageMetadata":{"promptTokenCount":2,"candidatesTokenCount":3,"thoughtsTokenCount":4,"totalTokenCount":9}}),
                ],
            );
            assert!(gemini.observation.complete);
            assert_eq!(
                gemini.observation.upstream_model.as_deref(),
                Some("gemini-native")
            );
            assert_eq!(gemini.observation.output_tokens, Some(7));
            assert_eq!(gemini.observation.visible_output_bytes, Some(6));
            assert!(
                crate::model_audit::audit(gemini.observation)
                    .flags
                    .is_empty()
            );
        }

        #[test]
        fn model_audit_anthropic_empty_tool_arguments_are_counted_or_replaced_once() {
            for arguments in [None, Some("{\"city\":\"杭州\"}")] {
                let mut frames = vec![
                    json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","name":"lookup","input":{}}}),
                ];
                if let Some(arguments) = arguments {
                    frames.push(json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":arguments}}));
                }
                frames.push(json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":8,"reasoning_tokens":0}}));
                frames.push(json!({"type":"message_stop"}));
                let collector = collect(UpstreamFormat::Anthropic, &frames);
                assert_eq!(
                    collector.observation.tool_payload_bytes,
                    Some((6 + arguments.unwrap_or("{}").len()) as u64)
                );
                assert!(
                    !crate::model_audit::audit(collector.observation)
                        .flags
                        .contains(&"inv.byte_bound".into())
                );
            }
        }

        #[test]
        fn model_audit_unknown_multimodal_output_and_incomplete_snapshots_skip_byte_proof() {
            let audio = Collector::new(UpstreamFormat::OpenAi, Observation::default()).non_stream(&json!({
                "choices":[{"message":{"content":"hi","audio":{"data":"not retained"}},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":2,"completion_tokens":99,"total_tokens":101,"completion_tokens_details":{"reasoning_tokens":0}}
            }));
            assert!(!audio.text_only);
            assert!(
                !crate::model_audit::audit(audio)
                    .flags
                    .contains(&"inv.byte_bound".into())
            );
            let snapshot = collect(
                UpstreamFormat::Responses,
                &[
                    json!({"type":"response.output_text.delta","delta":"partial"}),
                    json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"partial plus lost delta"}]}]}}),
                ],
            );
            assert!(!snapshot.observation.text_only);
        }

        #[test]
        fn shadow_audit_collector_rejects_empty_tools_and_equal_length_snapshot_rewrites() {
            for message in [
                json!({"content":"text","tool_calls":[{"type":"function","function":{"name":"","arguments":""}}]}),
                json!({"content":"text","function_call":{"name":"","arguments":""}}),
            ] {
                let collector = Collector::new(UpstreamFormat::OpenAi, Observation::default())
                    .capture_non_stream(&json!({
                    "choices":[{"message":message,"finish_reason":"stop"}]}));
                assert_eq!(collector.observation.tool_payload_bytes, Some(0));
                assert!(collector.observation.complete);
                assert!(!collector.shadow_plain_complete());
            }
            let tool_finish = Collector::new(UpstreamFormat::OpenAi, Observation::default())
                .capture_non_stream(&json!({
                "choices":[{"message":{"content":"text"},"finish_reason":"tool_calls"}]}));
            assert!(!tool_finish.shadow_plain_complete());
            for (final_text, expected) in [("same", true), ("edit", false)] {
                let collector = collect(
                    UpstreamFormat::Responses,
                    &[
                        json!({"type":"response.output_text.delta","delta":"same"}),
                        json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":final_text}]}]}}),
                    ],
                );
                assert_eq!(collector.observation.visible_output_bytes, Some(4));
                assert_eq!(collector.shadow_plain_complete(), expected);
            }
        }

        #[test]
        fn shadow_audit_collector_preserves_cache_hits_and_rejects_mixed_model_streams() {
            let mut collector = Collector::new(UpstreamFormat::OpenAi, Observation::default());
            collector.usage(&json!({"prompt_cache_hit_tokens":5,"input_tokens_details":{"cache_write_tokens":7}}));
            collector.usage(&json!({"prompt_tokens_details":{"cached_tokens":0},"cache_creation_input_tokens":0}));
            assert_eq!(collector.cache_read, Some(5));
            assert_eq!(collector.cache_write, Some(7));
            let mut frames = vec![
                json!({"model":"first","choices":[{"delta":{"content":"text"},"finish_reason":"stop"}]}),
                json!({"model":"other","choices":[],"usage":{"prompt_tokens":3}}),
            ];
            let mut collector = collect(UpstreamFormat::OpenAi, &frames);
            collector.frame(b"data: [DONE]", 12);
            collector.complete(true);
            assert!(collector.observation.complete);
            assert!(!collector.shadow_plain_complete());
            frames[1]["model"] = json!("first");
            let mut collector = collect(UpstreamFormat::OpenAi, &frames);
            collector.frame(b"data: [DONE]", 12);
            collector.complete(true);
            assert!(collector.shadow_plain_complete());
        }

        #[test]
        fn shadow_audit_collector_accepts_complete_plain_native_protocols() {
            for (format, body) in [
                (
                    UpstreamFormat::OpenAi,
                    json!({"choices":[{"message":{"role":"assistant","content":"answer"},"finish_reason":"stop"}]}),
                ),
                (
                    UpstreamFormat::Responses,
                    json!({"status":"completed","output":[{"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"answer"}]}]}),
                ),
                (
                    UpstreamFormat::Anthropic,
                    json!({"stop_reason":"end_turn","content":[{"type":"text","text":"answer"}]}),
                ),
                (
                    UpstreamFormat::Gemini,
                    json!({"candidates":[{"content":{"role":"model","parts":[{"text":"answer"}]},"finishReason":"STOP"}]}),
                ),
            ] {
                let collector =
                    Collector::new(format, Observation::default()).capture_non_stream(&body);
                assert!(collector.shadow_plain_complete());
                assert_eq!(
                    format!("{:x}", collector.output_sha.finalize()),
                    format!("{:x}", Sha256::digest(b"answer"))
                );
            }
        }

        #[test]
        fn model_audit_shadow_cache_alias_conflicts_preserve_every_positive_counter() {
            let zero = json!({
                "cache_read_input_tokens":0,
                "prompt_tokens_details":{"cached_tokens":0,"cache_write_tokens":0},
                "input_tokens_details":{"cached_tokens":0,"cache_write_tokens":0},
                "cachedContentTokenCount":0,
                "prompt_cache_hit_tokens":0,
                "cache_creation_input_tokens":0,
            });
            for path in [
                "/cache_read_input_tokens",
                "/prompt_tokens_details/cached_tokens",
                "/input_tokens_details/cached_tokens",
                "/cachedContentTokenCount",
                "/prompt_cache_hit_tokens",
            ] {
                let mut reported = zero.clone();
                *reported.pointer_mut(path).unwrap() = json!(9);
                let mut collector = Collector::new(UpstreamFormat::OpenAi, Observation::default());
                collector.usage(&reported);
                assert_eq!(
                    collector.cache_read,
                    Some(9),
                    "positive alias hidden: {path}"
                );
                collector.usage(&zero);
                assert_eq!(collector.cache_read, Some(9), "later zero erased: {path}");
            }
            for path in [
                "/cache_creation_input_tokens",
                "/input_tokens_details/cache_write_tokens",
                "/prompt_tokens_details/cache_write_tokens",
            ] {
                let mut reported = zero.clone();
                *reported.pointer_mut(path).unwrap() = json!(11);
                let mut collector = Collector::new(UpstreamFormat::OpenAi, Observation::default());
                collector.usage(&reported);
                assert_eq!(
                    collector.cache_write,
                    Some(11),
                    "positive alias hidden: {path}"
                );
                collector.usage(&zero);
                assert_eq!(collector.cache_write, Some(11), "later zero erased: {path}");
            }
            let mut unknown = Collector::new(UpstreamFormat::OpenAi, Observation::default());
            unknown.usage(&json!({"prompt_tokens":100}));
            assert_eq!(unknown.cache_read, None);
            assert_eq!(unknown.cache_write, None);
            unknown.usage(&zero);
            assert_eq!(unknown.cache_read, Some(0));
            assert_eq!(unknown.cache_write, Some(0));
        }

        #[test]
        fn model_audit_shadow_chat_reasoning_excludes_nonempty_but_keeps_empty_placeholders() {
            for (reasoning, expected) in [
                (Value::Null, true),
                (json!(""), true),
                (json!("thinking"), false),
                (json!(" "), false),
            ] {
                let usage = json!({"prompt_tokens":100,"completion_tokens":6,"total_tokens":106,"prompt_tokens_details":{"cached_tokens":0}});
                let body = json!({"choices":[{"message":{"role":"assistant","content":"answer","reasoning_content":reasoning},"finish_reason":"stop"}],"usage":usage});
                let non_stream = Collector::new(UpstreamFormat::OpenAi, Observation::default())
                    .capture_non_stream(&body);
                let mut streamed = collect(
                    UpstreamFormat::OpenAi,
                    &[
                        json!({"choices":[{"delta":{"content":"answer"}}]}),
                        json!({"choices":[{"delta":{"reasoning_content":reasoning},"finish_reason":"stop"}],"usage":usage}),
                    ],
                );
                streamed.frame(b"data: [DONE]", 12);
                streamed.complete(true);
                for collector in [&non_stream, &streamed] {
                    assert_eq!(
                        collector.shadow_plain_complete(),
                        expected,
                        "reasoning={reasoning}"
                    );
                    assert!(collector.observation.complete && collector.observation.text_only);
                    assert_eq!(collector.observation.input_tokens, Some(100));
                    assert_eq!(collector.observation.output_tokens, Some(6));
                    assert_eq!(collector.observation.visible_output_bytes, Some(6));
                    assert_eq!(collector.observation.tool_payload_bytes, Some(0));
                }
            }
        }

        #[test]
        fn model_audit_shadow_gemini_tool_prompt_usage_disqualifies_without_changing_text_counts() {
            for (tool_tokens, expected) in
                [(Value::Null, true), (json!(0), true), (json!(17), false)]
            {
                let mut body = json!({"modelVersion":"native","candidates":[{"content":{"role":"model","parts":[{"text":"answer"}]},"finishReason":"STOP"}],
                    "usageMetadata":{"promptTokenCount":100,"candidatesTokenCount":6,"totalTokenCount":106,"cachedContentTokenCount":0,"toolUsePromptTokenCount":tool_tokens}});
                let non_stream = Collector::new(UpstreamFormat::Gemini, Observation::default())
                    .capture_non_stream(&body);
                let mut streamed = collect(UpstreamFormat::Gemini, &[body.clone()]);
                // Final zero accounting must not restore eligibility after a
                // previous explicit tool-use prompt count in the same stream.
                body["candidates"] = json!([]);
                body["usageMetadata"]["toolUsePromptTokenCount"] = json!(0);
                streamed.frame(format!("data: {body}").as_bytes(), 13);
                streamed.complete(true);
                for collector in [&non_stream, &streamed] {
                    assert_eq!(
                        collector.shadow_plain_complete(),
                        expected,
                        "tool tokens={tool_tokens}"
                    );
                    assert!(collector.observation.complete && collector.observation.text_only);
                    assert_eq!(collector.observation.input_tokens, Some(100));
                    assert_eq!(collector.observation.output_tokens, Some(6));
                    assert_eq!(collector.observation.visible_output_bytes, Some(6));
                    assert_eq!(collector.observation.tool_payload_bytes, Some(0));
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
pub enum UpstreamFormat {
    OpenAi,
    Responses,
    Anthropic,
    Gemini,
}

#[derive(Clone, Copy)]
pub enum ClientFormat {
    Chat,
    Anthropic,
    Responses,
    Gemini,
}

enum Delta {
    Start,
    Text(String),
    Reasoning(String),
    ToolStart {
        index: usize,
        id: String,
        name: String,
    },
    ToolArguments {
        index: usize,
        arguments: String,
    },
    Finish(String),
    Done,
}

struct Context {
    id: String,
    model: String,
    started: bool,
    finished: bool,
    done: bool,
    usage_recorded: bool,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
    text_block: Option<usize>,
    tool_blocks: Vec<usize>,
    tool_names: Vec<Option<String>>,
    tool_arguments: Vec<String>,
    next_block: usize,
    response_items: Vec<Value>,
    reasoning_block: Option<usize>,
    response_tools: BTreeMap<usize, usize>,
    pending_finish: Option<String>,
    sequence_number: u64,
}

impl Context {
    fn new(model: &str) -> Self {
        Self {
            id: format!("stream_{}", uuid::Uuid::new_v4().simple()),
            model: model.to_string(),
            started: false,
            finished: false,
            done: false,
            usage_recorded: false,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            text_block: None,
            tool_blocks: Vec::new(),
            tool_names: Vec::new(),
            tool_arguments: Vec::new(),
            next_block: 0,
            response_items: Vec::new(),
            reasoning_block: None,
            response_tools: BTreeMap::new(),
            pending_finish: None,
            sequence_number: 0,
        }
    }
}

struct TransformState<S> {
    upstream: Pin<Box<S>>,
    format: UpstreamFormat,
    client: ClientFormat,
    context: Context,
    buffer: Vec<u8>,
    pending: VecDeque<Bytes>,
    upstream_done: bool,
    tool_bridge: crate::tool_bridge::ToolBridge,
    database: Option<crate::database::Database>,
    event_id: Option<i64>,
}

fn sse(event: Option<&str>, data: impl AsRef<str>) -> Bytes {
    let event = event
        .map(|value| format!("event: {value}\n"))
        .unwrap_or_default();
    Bytes::from(format!("{event}data: {}\n\n", data.as_ref()))
}

fn parse_frame(frame: &[u8]) -> Result<(Option<String>, String), io::Error> {
    let frame = std::str::from_utf8(frame)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "SSE frame is not UTF-8"))?;
    let mut event = None;
    let mut data = Vec::new();
    for line in frame.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("data:") {
            data.push(value.trim_start());
        }
    }
    Ok((event, data.join("\n")))
}

fn take_frame(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let delimiter = buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| (index, 4))
        .into_iter()
        .chain(
            buffer
                .windows(2)
                .position(|window| window == b"\n\n")
                .map(|index| (index, 2)),
        )
        .min_by_key(|(index, _)| *index)?;
    let frame = buffer[..delimiter.0].to_vec();
    buffer.drain(..delimiter.0 + delimiter.1);
    Some(frame)
}

fn ensure_start(context: &mut Context, deltas: &mut Vec<Delta>) {
    if !context.started {
        context.started = true;
        deltas.push(Delta::Start);
    }
}

fn normalize_openai(data: &str, context: &mut Context) -> Result<Vec<Delta>, io::Error> {
    if data == "[DONE]" {
        return Ok(vec![Delta::Done]);
    }
    let value: Value = serde_json::from_str(data)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid OpenAI SSE JSON"))?;
    if value.get("error").is_some() {
        return Err(io::Error::other("upstream returned an SSE error"));
    }
    if let Some(id) = value.get("id").and_then(Value::as_str) {
        context.id = id.to_string();
    }
    if let Some(model) = value.get("model").and_then(Value::as_str) {
        context.model = model.to_string();
    }
    if let Some(usage) = value.get("usage") {
        context.input_tokens = usage["prompt_tokens"]
            .as_u64()
            .unwrap_or(context.input_tokens);
        context.output_tokens = usage["completion_tokens"]
            .as_u64()
            .unwrap_or(context.output_tokens);
        if let Some(cached) = usage
            .get("prompt_tokens_details")
            .and_then(|d| d.get("cached_tokens"))
            .and_then(Value::as_u64)
        {
            context.cache_read_tokens = cached;
        }
    }
    let Some(choice) = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
    else {
        return Ok(Vec::new());
    };
    let mut result = Vec::new();
    let delta = &choice["delta"];
    if delta.get("role").is_some() {
        ensure_start(context, &mut result);
    }
    if let Some(text) = delta.get("content").and_then(Value::as_str) {
        ensure_start(context, &mut result);
        result.push(Delta::Text(text.to_string()));
    }
    if let Some(text) = delta.get("reasoning_content").and_then(Value::as_str) {
        // GLM / Qwen / DeepSeek 这类上游会在**每个** chunk 里带一个空的
        // reasoning_content 占位。照单全收的话：Responses 客户端（Codex）会先凭空开出一个
        // 空的 reasoning item，再为每个 token 推一条 delta:"" 的思考增量，表现为
        // 每个 token 单独占一行外加一个空思考块；Chat 客户端也会被灌一堆空字段。
        // 空字符串本身不携带任何信息，直接丢掉即可；非空推理内容不受影响。
        if !text.is_empty() {
            ensure_start(context, &mut result);
            result.push(Delta::Reasoning(text.to_string()));
        }
    }
    for call in delta
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        ensure_start(context, &mut result);
        let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        let function = &call["function"];
        if call.get("id").is_some() || function.get("name").is_some() {
            result.push(Delta::ToolStart {
                index,
                id: call["id"].as_str().unwrap_or("").to_string(),
                name: function["name"].as_str().unwrap_or("").to_string(),
            });
        }
        if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
            result.push(Delta::ToolArguments {
                index,
                arguments: arguments.to_string(),
            });
        }
    }
    if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
        result.push(Delta::Finish(reason.to_string()));
    }
    Ok(result)
}

fn normalize_responses(
    event: Option<&str>,
    data: &str,
    context: &mut Context,
) -> Result<Vec<Delta>, io::Error> {
    let value: Value = serde_json::from_str(data)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid Responses SSE JSON"))?;
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .or(event)
        .unwrap_or("");
    let mut result = Vec::new();
    match kind {
        "response.created" | "response.in_progress" => {
            let response = &value["response"];
            if let Some(id) = response.get("id").and_then(Value::as_str) {
                context.id = id.to_string();
            }
            if let Some(model) = response.get("model").and_then(Value::as_str) {
                context.model = model.to_string();
            }
            ensure_start(context, &mut result);
        }
        "response.output_text.delta" => {
            ensure_start(context, &mut result);
            if let Some(text) = value.get("delta").and_then(Value::as_str) {
                result.push(Delta::Text(text.to_string()));
            }
        }
        // 上游（xAI Grok、DeepSeek 官方 Codex 这类原生 Responses）推的思考增量。
        // 之前这里没有分支，会落到函数末尾的 `_ => {}` 被静默丢掉：Responses→Responses
        // 这条路虽然进出格式相同，但流是被拆成 Delta 再重新合成的（见 stream_formats
        // 对 ResponsesToResponses 返回 Some），不接这一支就等于上游想了半天、客户端
        // 一个字都看不到。Delta::Reasoning 和 render_responses 的 reasoning 渲染早就
        // 写好了，缺的只是这里的接线。
        //
        // 三种事件名都收：`reasoning_summary_text` 是 OpenAI 的标准名，
        // `reasoning_text` / `reasoning_summary` 是部分上游的变体。增量既可能是
        // 裸字符串 `delta:"..."`，也可能包一层 `delta:{text:"..."}`。
        // 只收 delta、不收对应的 .done：渲染侧是按增量累加的，done 再推一次会重复。
        "response.reasoning_summary_text.delta"
        | "response.reasoning_text.delta"
        | "response.reasoning_summary.delta" => {
            let text = value
                .get("delta")
                .and_then(|delta| {
                    delta
                        .as_str()
                        .or_else(|| delta.get("text").and_then(Value::as_str))
                })
                .unwrap_or("");
            // 空占位不往下传，理由同上面 normalize_chat 里对空 reasoning_content 的处理：
            // 空串不携带信息，却会让客户端凭空开一个空思考块。
            if !text.is_empty() {
                ensure_start(context, &mut result);
                result.push(Delta::Reasoning(text.to_string()));
            }
        }
        "response.output_item.added" => {
            let item = &value["item"];
            if item.get("type").and_then(Value::as_str) == Some("function_call") {
                ensure_start(context, &mut result);
                result.push(Delta::ToolStart {
                    index: value["output_index"].as_u64().unwrap_or(0) as usize,
                    id: item
                        .get("call_id")
                        .or_else(|| item.get("id"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    name: item["name"].as_str().unwrap_or("").to_string(),
                });
            }
        }
        "response.function_call_arguments.delta" => result.push(Delta::ToolArguments {
            index: value["output_index"].as_u64().unwrap_or(0) as usize,
            arguments: value["delta"].as_str().unwrap_or("").to_string(),
        }),
        "response.completed" | "response.incomplete" | "response.done" => {
            let response = &value["response"];
            let usage_opt = response.get("usage").or_else(|| value.get("usage"));
            if let Some(usage) = usage_opt {
                context.input_tokens = usage
                    .get("input_tokens")
                    .and_then(Value::as_u64)
                    .or_else(|| usage.get("prompt_tokens").and_then(Value::as_u64))
                    .unwrap_or(context.input_tokens);
                context.output_tokens = usage
                    .get("output_tokens")
                    .and_then(Value::as_u64)
                    .or_else(|| usage.get("completion_tokens").and_then(Value::as_u64))
                    .unwrap_or(context.output_tokens);
                let cached = usage
                    .get("cache_read_input_tokens")
                    .or_else(|| usage.pointer("/input_tokens_details/cached_tokens"))
                    .or_else(|| usage.pointer("/prompt_tokens_details/cached_tokens"))
                    .or_else(|| usage.get("prompt_cache_hit_tokens"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                if cached > 0 {
                    context.cache_read_tokens = cached;
                }
                let cache_write = usage
                    .get("cache_creation_input_tokens")
                    .or_else(|| usage.pointer("/input_tokens_details/cache_write_tokens"))
                    .or_else(|| usage.pointer("/prompt_tokens_details/cache_write_tokens"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                if cache_write > 0 {
                    context.cache_creation_tokens = cache_write;
                }
            }
            let reason = if kind == "response.incomplete"
                && response
                    .get("incomplete_details")
                    .and_then(|d| d.get("reason"))
                    .and_then(Value::as_str)
                    == Some("max_output_tokens")
            {
                "length"
            } else {
                "stop"
            };
            result.push(Delta::Finish(reason.to_string()));
            result.push(Delta::Done);
        }
        "response.failed" | "error" => {
            return Err(io::Error::other(
                value["response"]["error"]["message"]
                    .as_str()
                    .or_else(|| value["error"]["message"].as_str())
                    .unwrap_or("Responses stream error"),
            ));
        }
        _ => {}
    }
    Ok(result)
}

fn normalize_anthropic(
    event: Option<&str>,
    data: &str,
    context: &mut Context,
) -> Result<Vec<Delta>, io::Error> {
    let value: Value = serde_json::from_str(data)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid Anthropic SSE JSON"))?;
    let kind = event
        .or_else(|| value.get("type").and_then(Value::as_str))
        .unwrap_or("");
    let mut result = Vec::new();
    match kind {
        "message_start" => {
            let message = &value["message"];
            if let Some(id) = message.get("id").and_then(Value::as_str) {
                context.id = id.to_string();
            }
            if let Some(model) = message.get("model").and_then(Value::as_str) {
                context.model = model.to_string();
            }
            context.input_tokens = message["usage"]["input_tokens"].as_u64().unwrap_or(0);
            ensure_start(context, &mut result);
        }
        "content_block_start" => {
            ensure_start(context, &mut result);
            let index = value["index"].as_u64().unwrap_or(0) as usize;
            let block = &value["content_block"];
            match block["type"].as_str() {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(Value::as_str)
                        && !text.is_empty()
                    {
                        result.push(Delta::Text(text.to_string()));
                    }
                }
                Some("tool_use") => result.push(Delta::ToolStart {
                    index,
                    id: block["id"].as_str().unwrap_or("").to_string(),
                    name: block["name"].as_str().unwrap_or("").to_string(),
                }),
                _ => {}
            }
        }
        "content_block_delta" => {
            let index = value["index"].as_u64().unwrap_or(0) as usize;
            let delta = &value["delta"];
            match delta["type"].as_str() {
                Some("text_delta") => result.push(Delta::Text(
                    delta["text"].as_str().unwrap_or("").to_string(),
                )),
                Some("input_json_delta") => result.push(Delta::ToolArguments {
                    index,
                    arguments: delta["partial_json"].as_str().unwrap_or("").to_string(),
                }),
                _ => {}
            }
        }
        "message_delta" => {
            context.output_tokens = value["usage"]["output_tokens"].as_u64().unwrap_or(0);
            let reason = match value["delta"]["stop_reason"].as_str() {
                Some("tool_use") => "tool_calls",
                Some("max_tokens") => "length",
                _ => "stop",
            };
            result.push(Delta::Finish(reason.to_string()));
        }
        "message_stop" => result.push(Delta::Done),
        "error" => {
            return Err(io::Error::other(
                value["error"]["message"]
                    .as_str()
                    .unwrap_or("Anthropic stream error"),
            ));
        }
        _ => {}
    }
    Ok(result)
}

fn normalize_gemini(data: &str, context: &mut Context) -> Result<Vec<Delta>, io::Error> {
    let value: Value = serde_json::from_str(data)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid Gemini SSE JSON"))?;
    let mut result = Vec::new();
    ensure_start(context, &mut result);
    if let Some(usage) = value.get("usageMetadata") {
        context.input_tokens = usage["promptTokenCount"]
            .as_u64()
            .unwrap_or(context.input_tokens);
        context.output_tokens = usage["candidatesTokenCount"]
            .as_u64()
            .unwrap_or(context.output_tokens);
    }
    let Some(candidate) = value
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
    else {
        return Ok(result);
    };
    for (index, part) in candidate["content"]["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            result.push(Delta::Text(text.to_string()));
        }
        if let Some(call) = part.get("functionCall") {
            result.push(Delta::ToolStart {
                index,
                id: format!("call_{index}"),
                name: call["name"].as_str().unwrap_or("").to_string(),
            });
            result.push(Delta::ToolArguments {
                index,
                arguments: serde_json::to_string(&call["args"])
                    .unwrap_or_else(|_| "{}".to_string()),
            });
        }
    }
    if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) {
        result.push(Delta::Finish(
            if reason == "MAX_TOKENS" {
                "length"
            } else {
                "stop"
            }
            .to_string(),
        ));
    }
    Ok(result)
}

fn render_chat(delta: Delta, context: &mut Context) -> Vec<Bytes> {
    let chunk = |delta: Value, finish: Value, context: &Context| {
        sse(
            None,
            json!({
                "id":context.id,
                "object":"chat.completion.chunk",
                "model":context.model,
                "choices":[{"index":0, "delta":delta, "finish_reason":finish}]
            })
            .to_string(),
        )
    };
    match delta {
        Delta::Start => vec![chunk(
            json!({"role":"assistant", "content":""}),
            Value::Null,
            context,
        )],
        Delta::Text(text) => vec![chunk(json!({"content":text}), Value::Null, context)],
        Delta::Reasoning(text) => vec![chunk(
            json!({"reasoning_content":text}),
            Value::Null,
            context,
        )],
        Delta::ToolStart { index, id, name } => vec![chunk(
            json!({"tool_calls":[{"index":index, "id":id, "type":"function", "function":{"name":name, "arguments":""}}]}),
            Value::Null,
            context,
        )],
        Delta::ToolArguments { index, arguments } => vec![chunk(
            json!({"tool_calls":[{"index":index, "function":{"arguments":arguments}}]}),
            Value::Null,
            context,
        )],
        Delta::Finish(reason) => {
            context.finished = true;
            let mut value = json!({
                "id":context.id,
                "object":"chat.completion.chunk",
                "model":context.model,
                "choices":[{"index":0, "delta":{}, "finish_reason":reason}]
            });
            if context.input_tokens > 0 || context.output_tokens > 0 {
                value["usage"] = json!({
                    "prompt_tokens":context.input_tokens,
                    "completion_tokens":context.output_tokens,
                    "total_tokens":context.input_tokens + context.output_tokens
                });
            }
            vec![sse(None, value.to_string())]
        }
        Delta::Done => {
            context.done = true;
            vec![sse(None, "[DONE]")]
        }
    }
}

fn anthropic_stop_reason(reason: &str) -> &'static str {
    match reason {
        "tool_calls" => "tool_use",
        "length" => "max_tokens",
        _ => "end_turn",
    }
}

fn close_anthropic_blocks(context: &mut Context) -> Vec<Bytes> {
    let mut indexes = Vec::new();
    if let Some(index) = context.text_block.take() {
        indexes.push(index);
    }
    indexes.append(&mut context.tool_blocks);
    indexes.sort_unstable();
    indexes
        .into_iter()
        .map(|index| {
            sse(
                Some("content_block_stop"),
                json!({"type":"content_block_stop", "index":index}).to_string(),
            )
        })
        .collect()
}

fn render_anthropic(delta: Delta, context: &mut Context) -> Vec<Bytes> {
    match delta {
        Delta::Reasoning(_) => Vec::new(),
        Delta::Start => vec![sse(
            Some("message_start"),
            json!({
                "type":"message_start",
                "message":{"id":context.id, "type":"message", "role":"assistant", "model":context.model, "content":[], "stop_reason":null, "stop_sequence":null, "usage":{"input_tokens":context.input_tokens, "output_tokens":0}}
            })
            .to_string(),
        )],
        Delta::Text(text) => {
            let mut output = Vec::new();
            let index = if let Some(index) = context.text_block {
                index
            } else {
                let index = context.next_block;
                context.next_block += 1;
                context.text_block = Some(index);
                output.push(sse(
                    Some("content_block_start"),
                    json!({"type":"content_block_start", "index":index, "content_block":{"type":"text", "text":""}}).to_string(),
                ));
                index
            };
            output.push(sse(
                Some("content_block_delta"),
                json!({"type":"content_block_delta", "index":index, "delta":{"type":"text_delta", "text":text}}).to_string(),
            ));
            output
        }
        Delta::ToolStart { index, id, name } => {
            let block_index = context.next_block.max(index);
            context.next_block = block_index + 1;
            if !context.tool_blocks.contains(&block_index) {
                context.tool_blocks.push(block_index);
            }
            vec![sse(
                Some("content_block_start"),
                json!({"type":"content_block_start", "index":block_index, "content_block":{"type":"tool_use", "id":id, "name":name, "input":{}}}).to_string(),
            )]
        }
        Delta::ToolArguments { index, arguments } => {
            let block_index = context.tool_blocks.get(index).copied().unwrap_or(index);
            vec![sse(
                Some("content_block_delta"),
                json!({"type":"content_block_delta", "index":block_index, "delta":{"type":"input_json_delta", "partial_json":arguments}}).to_string(),
            )]
        }
        Delta::Finish(reason) => {
            let mut output = close_anthropic_blocks(context);
            output.push(sse(
                Some("message_delta"),
                json!({"type":"message_delta", "delta":{"stop_reason":anthropic_stop_reason(&reason), "stop_sequence":null}, "usage":{"output_tokens":context.output_tokens}}).to_string(),
            ));
            context.finished = true;
            output
        }
        Delta::Done => {
            context.done = true;
            vec![sse(
                Some("message_stop"),
                json!({"type":"message_stop"}).to_string(),
            )]
        }
    }
}

fn response_event(context: &mut Context, kind: &str, mut event: Value) -> Bytes {
    event["type"] = json!(kind);
    event["sequence_number"] = json!(context.sequence_number);
    context.sequence_number += 1;
    sse(Some(kind), event.to_string())
}

fn render_responses(delta: Delta, context: &mut Context) -> Vec<Bytes> {
    match delta {
        Delta::Start => vec![response_event(
            context,
            "response.created",
            json!({
                "response":{"id":context.id,"object":"response","status":"in_progress","model":context.model,"output":[]}
            }),
        )],
        Delta::Reasoning(text) => {
            let mut events = Vec::new();
            let index = if let Some(index) = context.reasoning_block {
                index
            } else {
                let index = context.response_items.len();
                let item = json!({"id":format!("rs_{}_{}",context.id,index),"type":"reasoning","summary":[]});
                events.push(response_event(
                    context,
                    "response.output_item.added",
                    json!({"output_index":index,"item":item}),
                ));
                let part = json!({"type":"summary_text","text":""});
                events.push(response_event(context,"response.reasoning_summary_part.added",json!({"item_id":item["id"],"output_index":index,"summary_index":0,"part":part})));
                let mut item = item;
                item["summary"] = json!([part]);
                context.response_items.push(item);
                context.reasoning_block = Some(index);
                index
            };
            let item = &mut context.response_items[index];
            item["summary"][0]["text"] = json!(format!(
                "{}{text}",
                item["summary"][0]["text"].as_str().unwrap_or("")
            ));
            let id = item["id"].clone();
            events.push(response_event(
                context,
                "response.reasoning_summary_text.delta",
                json!({"item_id":id,"output_index":index,"summary_index":0,"delta":text}),
            ));
            events
        }
        Delta::Text(text) => {
            let mut events = Vec::new();
            let index = if let Some(index) = context.text_block {
                index
            } else {
                let index = context.response_items.len();
                let id = format!("msg_{}_{}", context.id, index);
                let item = json!({"id":id,"type":"message","status":"in_progress","role":"assistant","content":[]});
                events.push(response_event(
                    context,
                    "response.output_item.added",
                    json!({"output_index":index,"item":item}),
                ));
                let part = json!({"type":"output_text","text":"","annotations":[]});
                events.push(response_event(
                    context,
                    "response.content_part.added",
                    json!({"item_id":id,"output_index":index,"content_index":0,"part":part}),
                ));
                context.response_items.push(json!({"id":id,"type":"message","status":"in_progress","role":"assistant","content":[part]}));
                context.text_block = Some(index);
                index
            };
            let item = &mut context.response_items[index];
            let previous = item["content"][0]["text"].as_str().unwrap_or("");
            item["content"][0]["text"] = json!(format!("{previous}{text}"));
            let id = item["id"].clone();
            events.push(response_event(
                context,
                "response.output_text.delta",
                json!({"item_id":id,"output_index":index,"content_index":0,"delta":text}),
            ));
            events
        }
        Delta::ToolStart { index, id, name } => {
            if let Some(&output_index) = context.response_tools.get(&index) {
                // Some providers split the function name across chunks.
                let item = &mut context.response_items[output_index];
                if !name.is_empty() {
                    item["name"] = json!(format!("{}{name}", item["name"].as_str().unwrap_or("")));
                }
                if !id.is_empty() {
                    item["call_id"] = json!(id);
                }
                return Vec::new();
            }
            let output_index = context.response_items.len();
            let item = json!({"id":format!("fc_{}_{}",context.id,output_index),"type":"function_call","status":"in_progress","call_id":id,"name":name,"arguments":""});
            context.response_tools.insert(index, output_index);
            context.response_items.push(item.clone());
            vec![response_event(
                context,
                "response.output_item.added",
                json!({"output_index":output_index,"item":item}),
            )]
        }
        Delta::ToolArguments { index, arguments } => {
            let Some(&output_index) = context.response_tools.get(&index) else {
                return Vec::new();
            };
            let item = &mut context.response_items[output_index];
            item["arguments"] = json!(format!(
                "{}{arguments}",
                item["arguments"].as_str().unwrap_or("")
            ));
            let id = item["id"].clone();
            vec![response_event(
                context,
                "response.function_call_arguments.delta",
                json!({"item_id":id,"output_index":output_index,"delta":arguments}),
            )]
        }
        Delta::Finish(reason) => {
            if context.finished {
                return Vec::new();
            }
            context.finished = true;
            let incomplete = matches!(reason.as_str(), "length" | "content_filter" | "max_tokens");
            let status = if incomplete {
                "incomplete"
            } else {
                "completed"
            };
            let mut events = Vec::new();
            for index in 0..context.response_items.len() {
                context.response_items[index]["status"] = json!(status);
                let item = context.response_items[index].clone();
                if item["type"] == "function_call" {
                    events.push(response_event(context,"response.function_call_arguments.done", json!({"item_id":item["id"],"output_index":index,"arguments":item["arguments"],"name":item["name"]})));
                } else if item["type"] == "reasoning" {
                    events.push(response_event(context,"response.reasoning_summary_text.done",json!({"item_id":item["id"],"output_index":index,"summary_index":0,"text":item["summary"][0]["text"]})));
                    events.push(response_event(context,"response.reasoning_summary_part.done",json!({"item_id":item["id"],"output_index":index,"summary_index":0,"part":item["summary"][0]})));
                } else {
                    events.push(response_event(context,"response.output_text.done", json!({"item_id":item["id"],"output_index":index,"content_index":0,"text":item["content"][0]["text"]})));
                    events.push(response_event(context,"response.content_part.done", json!({"item_id":item["id"],"output_index":index,"content_index":0,"part":item["content"][0]})));
                }
                events.push(response_event(
                    context,
                    "response.output_item.done",
                    json!({"output_index":index,"item":item}),
                ));
            }
            let mut response = json!({"id":context.id,"object":"response","status":status,"model":context.model,"output":context.response_items,"usage":{"input_tokens":context.input_tokens,"output_tokens":context.output_tokens,"total_tokens":context.input_tokens+context.output_tokens}});
            if incomplete {
                response["incomplete_details"] = json!({"reason":if reason == "content_filter" { "content_filter" } else { "max_output_tokens" }});
            }
            events.push(response_event(
                context,
                if incomplete {
                    "response.incomplete"
                } else {
                    "response.completed"
                },
                json!({"response":response}),
            ));
            events
        }
        Delta::Done => {
            context.done = true;
            match context.pending_finish.take() {
                Some(reason) => render_responses(Delta::Finish(reason), context),
                None => Vec::new(),
            }
        }
    }
}

fn render_gemini(delta: Delta, context: &mut Context) -> Vec<Bytes> {
    match delta {
        Delta::Reasoning(_) => Vec::new(),
        Delta::Start => Vec::new(),
        Delta::Text(text) => vec![sse(
            None,
            json!({"candidates":[{"content":{"role":"model", "parts":[{"text":text}]}, "index":0}]}).to_string(),
        )],
        Delta::ToolStart { index, name, .. } => {
            if context.tool_names.len() <= index {
                context.tool_names.resize(index + 1, None);
                context.tool_arguments.resize(index + 1, String::new());
            }
            context.tool_names[index] = Some(name);
            Vec::new()
        }
        Delta::ToolArguments { index, arguments } => {
            if context.tool_arguments.len() <= index {
                context.tool_names.resize(index + 1, None);
                context.tool_arguments.resize(index + 1, String::new());
            }
            context.tool_arguments[index].push_str(&arguments);
            Vec::new()
        }
        Delta::Finish(reason) => {
            let parts: Vec<Value> = context
                .tool_names
                .iter()
                .enumerate()
                .filter_map(|(index, name)| name.as_ref().map(|name| (index, name)))
                .map(|(index, name)| {
                    let arguments = context
                        .tool_arguments
                        .get(index)
                        .and_then(|value| serde_json::from_str::<Value>(value).ok())
                        .unwrap_or_else(|| json!({}));
                    json!({"functionCall":{"name":name, "args":arguments}})
                })
                .collect();
            context.finished = true;
            vec![sse(
                None,
                json!({
                    "candidates":[{"content":{"role":"model", "parts":parts}, "finishReason":if reason == "length" { "MAX_TOKENS" } else { "STOP" }, "index":0}],
                    "usageMetadata":{"promptTokenCount":context.input_tokens, "candidatesTokenCount":context.output_tokens, "totalTokenCount":context.input_tokens + context.output_tokens}
                }).to_string(),
            )]
        }
        Delta::Done => {
            context.done = true;
            Vec::new()
        }
    }
}

fn process_frame(
    format: UpstreamFormat,
    client: ClientFormat,
    frame: &[u8],
    context: &mut Context,
) -> Result<Vec<Bytes>, io::Error> {
    let (event, data) = parse_frame(frame)?;
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let deltas = match format {
        UpstreamFormat::OpenAi => normalize_openai(&data, context)?,
        UpstreamFormat::Responses => normalize_responses(event.as_deref(), &data, context)?,
        UpstreamFormat::Anthropic => normalize_anthropic(event.as_deref(), &data, context)?,
        UpstreamFormat::Gemini => normalize_gemini(&data, context)?,
    };
    Ok(deltas
        .into_iter()
        .flat_map(|delta| match client {
            ClientFormat::Chat => render_chat(delta, context),
            ClientFormat::Anthropic => render_anthropic(delta, context),
            ClientFormat::Responses => {
                if matches!(format, UpstreamFormat::OpenAi) {
                    if let Delta::Finish(reason) = delta {
                        context.pending_finish = Some(reason);
                        return Vec::new();
                    }
                }
                render_responses(delta, context)
            }
            ClientFormat::Gemini => render_gemini(delta, context),
        })
        .collect())
}

#[cfg(test)]
pub fn transform<S>(
    upstream: S,
    format: UpstreamFormat,
    client: ClientFormat,
    model: String,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    transform_with_tools(
        upstream,
        format,
        client,
        model,
        crate::tool_bridge::ToolBridge::default(),
        None,
        None,
    )
}

pub fn transform_with_tools<S>(
    upstream: S,
    format: UpstreamFormat,
    client: ClientFormat,
    model: String,
    tool_bridge: crate::tool_bridge::ToolBridge,
    database: Option<crate::database::Database>,
    event_id: Option<i64>,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    let state = TransformState {
        upstream: Box::pin(upstream),
        format,
        client,
        context: Context::new(&model),
        buffer: Vec::new(),
        pending: VecDeque::new(),
        upstream_done: false,
        tool_bridge,
        database,
        event_id,
    };
    stream::try_unfold(state, |mut state| async move {
        loop {
            if let Some(item) = state.pending.pop_front() {
                if matches!(state.client, ClientFormat::Responses) {
                    let (_, data) = parse_frame(&item)?;
                    let mut event: Value = serde_json::from_str(&data).map_err(io::Error::other)?;
                    let alias = event["output_index"]
                        .as_u64()
                        .and_then(|i| state.context.response_items.get(i as usize))
                        .and_then(|item| item["name"].as_str());
                    if !state
                        .tool_bridge
                        .restore_event(&mut event, alias)
                        .map_err(io::Error::other)?
                    {
                        continue;
                    }
                    return Ok(Some((
                        sse(event["type"].as_str(), event.to_string()),
                        state,
                    )));
                }
                return Ok(Some((item, state)));
            }
            if let Some(frame) = take_frame(&mut state.buffer) {
                state.pending.extend(process_frame(
                    state.format,
                    state.client,
                    &frame,
                    &mut state.context,
                )?);
                continue;
            }
            if state.upstream_done {
                if !state.context.finished {
                    state.pending.extend(match state.client {
                        ClientFormat::Chat => render_chat(Delta::Done, &mut state.context),
                        ClientFormat::Anthropic => {
                            render_anthropic(Delta::Done, &mut state.context)
                        }
                        ClientFormat::Responses => {
                            let reason = state.context.pending_finish.take().ok_or_else(|| {
                                io::Error::new(
                                    io::ErrorKind::UnexpectedEof,
                                    "upstream stream ended without a finish event",
                                )
                            })?;
                            render_responses(Delta::Finish(reason), &mut state.context)
                        }
                        ClientFormat::Gemini => {
                            render_gemini(Delta::Finish("stop".to_string()), &mut state.context)
                        }
                    });
                    state.context.finished = true;
                    continue;
                }
                if !state.context.done {
                    state.pending.extend(match state.client {
                        ClientFormat::Chat => render_chat(Delta::Done, &mut state.context),
                        ClientFormat::Anthropic => {
                            render_anthropic(Delta::Done, &mut state.context)
                        }
                        ClientFormat::Responses => {
                            render_responses(Delta::Done, &mut state.context)
                        }
                        ClientFormat::Gemini => render_gemini(Delta::Done, &mut state.context),
                    });
                    state.context.done = true;
                    continue;
                }
                if !state.context.usage_recorded {
                    state.context.usage_recorded = true;
                    if let (Some(db), Some(event_id)) = (&state.database, state.event_id) {
                        let cost = crate::session_usage_codex::estimate_codex_cost(
                            &state.context.model,
                            state.context.input_tokens as i64,
                            state.context.output_tokens as i64,
                            state.context.cache_read_tokens as i64,
                        );
                        let usage = crate::database::RequestUsage {
                            model: Some(state.context.model.clone()),
                            input_tokens: Some(state.context.input_tokens as i64),
                            output_tokens: Some(state.context.output_tokens as i64),
                            cache_creation_tokens: (state.context.cache_creation_tokens > 0)
                                .then_some(state.context.cache_creation_tokens as i64),
                            cache_read_tokens: (state.context.cache_read_tokens > 0)
                                .then_some(state.context.cache_read_tokens as i64),
                            cost: (cost > 0.0).then_some(cost),
                            price_source: (cost > 0.0).then(|| "standard".to_string()),
                            price_version: (cost > 0.0).then(|| "v1".to_string()),
                            ..Default::default()
                        };
                        let _ = db.update_request_usage(event_id, &usage);
                    }
                }
                return Ok(None);
            }
            match state.upstream.next().await {
                Some(Ok(bytes)) => {
                    state.buffer.extend_from_slice(&bytes);
                    if state.buffer.len() > MAX_SSE_FRAME_BYTES {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "SSE frame exceeds size limit",
                        ));
                    }
                }
                Some(Err(error)) => return Err(io::Error::other(error.to_string())),
                None => {
                    state.upstream_done = true;
                    if !state.buffer.is_empty() {
                        let frame = std::mem::take(&mut state.buffer);
                        state.pending.extend(process_frame(
                            state.format,
                            state.client,
                            &frame,
                            &mut state.context,
                        )?);
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_frames(format: UpstreamFormat, client: ClientFormat, frames: &[&str]) -> String {
        let mut context = Context::new("test-model");
        frames
            .iter()
            .flat_map(|frame| {
                process_frame(format, client, frame.as_bytes(), &mut context).unwrap()
            })
            .map(|bytes| String::from_utf8(bytes.to_vec()).unwrap())
            .collect()
    }

    #[test]
    fn converts_anthropic_text_stream_to_chat_sse() {
        let output = render_frames(
            UpstreamFormat::Anthropic,
            ClientFormat::Chat,
            &[
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude\",\"usage\":{\"input_tokens\":2}}}",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}",
                "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}",
            ],
        );
        assert!(output.contains("chat.completion.chunk"));
        assert!(output.contains("\"content\":\"Hi\""));
        assert!(output.contains("\"total_tokens\":3"));
        assert!(output.contains("data: [DONE]"));
    }

    #[test]
    fn converts_openai_tool_stream_to_anthropic_sse() {
        let output = render_frames(
            UpstreamFormat::OpenAi,
            ClientFormat::Anthropic,
            &[
                "data: {\"id\":\"chat_1\",\"model\":\"gpt\",\"choices\":[{\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}",
                "data: {\"id\":\"chat_1\",\"model\":\"gpt\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"lookup\",\"arguments\":\"{\"}}]},\"finish_reason\":null}]}",
                "data: {\"id\":\"chat_1\",\"model\":\"gpt\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}",
                "data: [DONE]",
            ],
        );
        assert!(output.contains("content_block_start"));
        assert!(output.contains("input_json_delta"));
        assert!(output.contains("tool_use"));
        assert!(output.contains("message_stop"));
    }

    #[test]
    fn converts_gemini_text_stream_to_responses_sse() {
        let output = render_frames(
            UpstreamFormat::Gemini,
            ClientFormat::Responses,
            &[
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hello\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":2,\"candidatesTokenCount\":1}}",
            ],
        );
        assert!(output.contains("response.created"));
        assert!(output.contains("response.output_text.delta"));
        assert!(output.contains("response.completed"));
    }

    #[test]
    fn converts_responses_stream_to_chat_sse() {
        let output = render_frames(
            UpstreamFormat::Responses,
            ClientFormat::Chat,
            &[
                "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-native\"}}",
                "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}",
                "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":3,\"output_tokens\":1}}}",
            ],
        );
        assert!(output.contains("chat.completion.chunk"));
        assert!(output.contains("\"content\":\"Hello\""));
        assert!(output.contains("\"total_tokens\":4"));
        assert!(output.contains("data: [DONE]"));
    }
    /// 原生 Responses 上游（xAI Grok / DeepSeek 官方 Codex）的思考增量必须活着穿过
    /// Responses→Responses 这条路。以前 normalize_responses 没有 reasoning 分支，
    /// 这些事件会落到 `_ => {}` 被丢掉，客户端一个思考字都收不到。
    #[test]
    fn preserves_reasoning_on_responses_to_responses() {
        let output = render_frames(
            UpstreamFormat::Responses,
            ClientFormat::Responses,
            &[
                "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"grok-4.5\"}}",
                "event: response.reasoning_summary_text.delta\ndata: {\"type\":\"response.reasoning_summary_text.delta\",\"delta\":\"weighing\"}",
                "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"done\"}",
                "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":3,\"output_tokens\":2}}}",
            ],
        );
        // 思考内容出去了，而且是以 reasoning item 的形态，不是被塞进正文
        assert!(output.contains("\"type\":\"reasoning\""));
        assert!(output.contains("response.reasoning_summary_text.delta"));
        assert!(output.contains("weighing"));
        // 正文仍然正常
        assert!(output.contains("done"));
    }

    /// 变体事件名与包了一层的 delta 形状也要认；空增量不能凭空开思考块。
    #[test]
    fn accepts_reasoning_event_variants_and_drops_empty() {
        let nested = render_frames(
            UpstreamFormat::Responses,
            ClientFormat::Responses,
            &[
                "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"r\",\"model\":\"grok-4.5\"}}",
                "event: response.reasoning_text.delta\ndata: {\"type\":\"response.reasoning_text.delta\",\"delta\":{\"text\":\"nested\"}}",
            ],
        );
        assert!(nested.contains("nested"));
        assert!(nested.contains("\"type\":\"reasoning\""));

        let empty = render_frames(
            UpstreamFormat::Responses,
            ClientFormat::Responses,
            &[
                "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"r\",\"model\":\"grok-4.5\"}}",
                "event: response.reasoning_summary_text.delta\ndata: {\"type\":\"response.reasoning_summary_text.delta\",\"delta\":\"\"}",
            ],
        );
        assert!(!empty.contains("\"type\":\"reasoning\""));
    }

    include!("streaming_regression_tests.rs");
}
