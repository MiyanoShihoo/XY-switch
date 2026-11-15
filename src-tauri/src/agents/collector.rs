use super::models::{AgentCapability, AgentEvent, AgentSource, LineContext, ParseRecord};
use super::store::{
    CompressedCursor, EventStore, FileCursor, SeedScanCursor, digest, relative_path_is_safe,
};
use crate::skills_local::is_link;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;
use zstd::stream::raw::{DParameter, Decoder, InBuffer, Operation, OutBuffer};

const DISCOVERY_INTERVAL_MS: i64 = 15_000;
const MAX_ROOTS: usize = 16;
const MAX_DEPTH: usize = 8;
const MAX_DIRECTORIES: usize = 4096;
const MAX_ENTRIES: usize = 50_000;
const MAX_FILES: usize = 4096;
const MAX_POLL_BYTES: usize = 4 * 1024 * 1024;
const MAX_FILE_BYTES: usize = 512 * 1024;
const MAX_LINE_BYTES: usize = 384 * 1024;
const MAX_RECORD_EVENTS: usize = 1024;
const MAX_BATCH_EVENTS: usize = 4096;
const MAX_POLL_EVENTS: usize = 16_384;
const MAX_POLL_READ_FILES: usize = 64;
const HEADER_BYTES: usize = 1024;
const TAIL_BYTES: usize = 128;
const MIN_FILE_BUDGET: usize = 4096;
const MAX_COMPRESSED_STREAMS: usize = 4;
const ZSTD_WINDOW_LOG: u32 = 23;
const ZSTD_INPUT_BYTES: usize = 16 * 1024;
const ZSTD_OUTPUT_BYTES: usize = 32 * 1024;
const MAX_ZSTD_STEPS: usize = 4096;

#[derive(Clone)]
pub struct SourceRoot {
    pub id: String,
    pub client_id: String,
    pub path: PathBuf,
    pub adapter: ParseRecord,
}

#[derive(Default)]
pub struct CollectResult {
    pub events: Vec<AgentEvent>,
    pub sources: Vec<AgentSource>,
    pub warnings: Vec<String>,
    pub changed: bool,
    pub bytes_read: u64,
    pub decoded_bytes: u64,
    pub pending_files: usize,
    pub pruned_events: usize,
}

pub struct Collector {
    roots: Vec<RootState>,
    store: EventStore,
    previous_status: String,
    fair_offset: usize,
    last_prune: Option<i64>,
    retention_days: u32,
    compressed: BTreeMap<(usize, String), CompressedRuntime>,
}

struct RootState {
    root: SourceRoot,
    root_key: String,
    cursors: BTreeMap<String, FileCursor>,
    files: BTreeMap<String, Candidate>,
    last_discovery: Option<i64>,
    ever_seen: bool,
    last_event_at: Option<i64>,
    file_count: usize,
    online: bool,
    io_error: bool,
    discovery_error: bool,
    unsafe_path: bool,
    limited: bool,
    compressed_history: bool,
    rejected_links: usize,
    skipped_lines: usize,
    pending_files: usize,
}

#[derive(Clone, Default)]
struct Candidate {
    path: String,
    modified: u128,
    last_stamp: Option<FileStamp>,
    last_verified: Option<i64>,
    caught_up: bool,
}

#[derive(Clone, PartialEq)]
struct FileStamp {
    len: u64,
    modified: Option<String>,
    created: Option<String>,
    identity_hint: Option<String>,
}

#[derive(Default)]
struct Discovery {
    files: Vec<Candidate>,
    file_count: usize,
    io_error: bool,
    limited: bool,
    compressed_history: bool,
    rejected_links: usize,
}

struct FileRead {
    cursor: FileCursor,
    events: Vec<AgentEvent>,
    stamp: FileStamp,
    caught_up: bool,
    skipped_lines: usize,
    reset: bool,
}

struct ReadBudget {
    remaining: usize,
    used: u64,
    decoded: u64,
}

#[derive(Default)]
struct CompressedRuntime {
    validation: Option<DecodeStream>,
    replay: Option<ReplayStream>,
}

#[derive(Clone, Copy, Default)]
struct FramePoint {
    raw: u64,
    decoded: u64,
}

struct DecodeStream {
    decoder: Decoder<'static>,
    raw: u64,
    decoded: u64,
    input: Vec<u8>,
    input_pos: usize,
    boundary: FramePoint,
    at_boundary: bool,
}

struct ReplayStream {
    stream: DecodeStream,
    pending: Vec<u8>,
    scanning: bool,
}

struct PhysicalProof {
    end: u64,
    tail: Vec<u8>,
}

enum DecodeStep {
    Data(Vec<u8>),
    End,
    Incomplete,
    Invalid,
    Budget,
}

impl PhysicalProof {
    fn observe(&mut self, offset: u64, data: &[u8]) -> Result<(), LogError> {
        let end = offset + data.len() as u64;
        if end <= self.end {
            return Ok(());
        }
        if offset > self.end {
            return Err(LogError::Changed);
        }
        let length = end.min(TAIL_BYTES as u64) as usize;
        let suffix = length.min(data.len());
        let prefix = length - suffix;
        let overlap = self.end.saturating_sub(offset) as usize;
        if prefix != 0 && prefix + overlap > self.tail.len() {
            return Err(LogError::Changed);
        }
        let mut tail = if prefix == 0 {
            Vec::new()
        } else {
            self.tail[self.tail.len() - overlap - prefix..self.tail.len() - overlap].to_vec()
        };
        tail.extend_from_slice(&data[data.len() - suffix..]);
        self.end = end;
        self.tail = tail;
        Ok(())
    }
}

impl DecodeStream {
    fn new(point: FramePoint) -> Result<Self, LogError> {
        let mut decoder = Decoder::new().map_err(|_| LogError::Unreadable)?;
        decoder
            .set_parameter(DParameter::WindowLogMax(ZSTD_WINDOW_LOG))
            .map_err(|_| LogError::Unreadable)?;
        Ok(Self {
            decoder,
            raw: point.raw,
            decoded: point.decoded,
            input: Vec::new(),
            input_pos: 0,
            boundary: point,
            at_boundary: true,
        })
    }

    fn step(
        &mut self,
        file: &mut File,
        limit: u64,
        budget: &mut ReadBudget,
        proof: &mut PhysicalProof,
    ) -> Result<DecodeStep, LogError> {
        if self.raw > limit {
            return Err(LogError::Changed);
        }
        if self.at_boundary && self.raw == limit {
            return Ok(DecodeStep::End);
        }
        if budget.remaining == 0 {
            return Ok(DecodeStep::Budget);
        }
        if self.input_pos == self.input.len() && self.raw < limit {
            let length = (limit - self.raw)
                .min(ZSTD_INPUT_BYTES as u64)
                .min((budget.remaining / 2) as u64) as usize;
            if length == 0 {
                return Ok(DecodeStep::Budget);
            }
            self.input = read_range(file, self.raw, length, budget)?;
            proof.observe(self.raw, &self.input)?;
            self.input_pos = 0;
        }
        if self.at_boundary {
            if self.decoder.reinit().is_err() {
                return Ok(DecodeStep::Invalid);
            }
            self.at_boundary = false;
        }
        let mut output = vec![0; ZSTD_OUTPUT_BYTES.min(budget.remaining)];
        let (remaining, bytes_read, bytes_written) = {
            let mut input = InBuffer::around(&self.input[self.input_pos..]);
            let mut decoded = OutBuffer::around(&mut output[..]);
            let remaining = self.decoder.run(&mut input, &mut decoded);
            (remaining, input.pos(), decoded.pos())
        };
        budget.remaining -= bytes_written;
        budget.decoded += bytes_written as u64;
        self.input_pos += bytes_read;
        self.raw += bytes_read as u64;
        self.decoded = match self.decoded.checked_add(bytes_written as u64) {
            Some(decoded) if decoded <= i64::MAX as u64 => decoded,
            _ => return Ok(DecodeStep::Invalid),
        };
        output.truncate(bytes_written);
        let Ok(remaining) = remaining else {
            return Ok(DecodeStep::Invalid);
        };
        if remaining == 0 {
            self.boundary = FramePoint {
                raw: self.raw,
                decoded: self.decoded,
            };
            self.at_boundary = true;
        } else if bytes_read == 0 && bytes_written == 0 {
            return Ok(if self.raw == limit && self.input_pos == self.input.len() {
                DecodeStep::Incomplete
            } else {
                DecodeStep::Invalid
            });
        }
        Ok(DecodeStep::Data(output))
    }
}

struct OpenedLog {
    file: File,
    #[cfg(windows)]
    _parents: Vec<File>,
}

#[derive(Clone, Copy, Debug)]
enum LogError {
    Unreadable,
    UnsafePath,
    Changed,
}

impl Collector {
    pub fn new(roots: Vec<SourceRoot>, store: EventStore) -> Result<Self, String> {
        if roots.len() > MAX_ROOTS {
            return Err("Agent 日志根目录数量超过上限".into());
        }
        let persisted = store.cursors()?;
        let retention_days = store.retention_days()?;
        let mut ids = HashSet::new();
        let mut states = Vec::with_capacity(roots.len());
        for mut root in roots {
            if root.id.is_empty() || root.id.len() > 240 || !ids.insert(root.id.clone()) {
                return Err("Agent 日志来源标识无效或重复".into());
            }
            if !matches!(root.client_id.as_str(), "codex" | "deepseek_harness") {
                return Err("Agent 日志来源类型不受支持".into());
            }
            if !root.path.is_absolute()
                || root
                    .path
                    .components()
                    .any(|part| matches!(part, Component::ParentDir))
            {
                return Err("Agent 日志根目录必须是无父级跳转的绝对路径".into());
            }
            root.path = root
                .path
                .components()
                .filter(|part| !matches!(part, Component::CurDir))
                .collect();
            let root_key = root_key(&root.path);
            let cursors: BTreeMap<_, _> = persisted
                .iter()
                .filter(|cursor| cursor.source_id == root.id && cursor.root_key == root_key)
                .map(|cursor| (cursor.path.clone(), cursor.clone()))
                .collect();
            let last_event_at = cursors
                .values()
                .filter_map(|cursor| cursor.last_event_at)
                .max();
            let ever_seen = !cursors.is_empty() || store.source_seen(&root.id, &root_key)?;
            states.push(RootState {
                root,
                root_key,
                cursors,
                files: BTreeMap::new(),
                last_discovery: None,
                ever_seen,
                last_event_at,
                file_count: 0,
                online: false,
                io_error: false,
                discovery_error: false,
                unsafe_path: false,
                limited: false,
                compressed_history: false,
                rejected_links: 0,
                skipped_lines: 0,
                pending_files: 0,
            });
        }
        Ok(Self {
            roots: states,
            store,
            previous_status: String::new(),
            fair_offset: 0,
            last_prune: None,
            retention_days,
            compressed: BTreeMap::new(),
        })
    }

    pub fn poll(&mut self, now: i64) -> Result<CollectResult, String> {
        let mut result = CollectResult::default();
        let mut budget = ReadBudget {
            remaining: MAX_POLL_BYTES,
            used: 0,
            decoded: 0,
        };
        let mut work = Vec::new();
        for (root_index, state) in self.roots.iter_mut().enumerate() {
            state.online = false;
            state.io_error = state.discovery_error;
            state.unsafe_path = false;
            state.skipped_lines = 0;
            state.pending_files = 0;
            match readable_root(&state.root.path) {
                Ok(()) => state.online = true,
                Err(error) => {
                    state.unsafe_path = matches!(error, LogError::UnsafePath);
                    state.io_error = true;
                    continue;
                }
            }
            if !state.ever_seen {
                match self.store.mark_source_seen(&state.root.id, &state.root_key) {
                    Ok(()) => state.ever_seen = true,
                    Err(_) => warning(&mut result.warnings, "Agent 来源历史暂无法保存"),
                }
            }
            if interval_elapsed(state.last_discovery, now) {
                let discovery = discover(&state.root);
                state.last_discovery = Some(now);
                state.file_count = discovery.file_count;
                state.discovery_error = discovery.io_error;
                state.io_error = discovery.io_error;
                state.limited = discovery.limited;
                state.compressed_history = discovery.compressed_history;
                state.rejected_links = discovery.rejected_links;
                let mut files = BTreeMap::new();
                for mut candidate in discovery.files {
                    if let Some(previous) = state.files.remove(&candidate.path) {
                        candidate.last_stamp = previous.last_stamp;
                        candidate.last_verified = previous.last_verified;
                        candidate.caught_up = previous.caught_up;
                    }
                    files.insert(candidate.path.clone(), candidate);
                }
                if discovery.io_error || discovery.limited {
                    for (path, previous) in std::mem::take(&mut state.files) {
                        if files.len() >= MAX_FILES {
                            break;
                        }
                        files.entry(path).or_insert(previous);
                    }
                }
                state.files = files;
            }
            for candidate in state.files.values_mut() {
                match safe_metadata(&state.root.path, &candidate.path) {
                    Ok(metadata) => {
                        let stamp = file_stamp(&metadata);
                        candidate.modified = modified_order(&metadata);
                        if candidate.last_stamp.as_ref() != Some(&stamp)
                            || !candidate.caught_up
                            || interval_elapsed(candidate.last_verified, now)
                        {
                            work.push((root_index, candidate.path.clone(), candidate.modified));
                        }
                    }
                    Err(error) => {
                        state.io_error = true;
                        state.unsafe_path |= matches!(error, LogError::UnsafePath);
                    }
                }
            }
        }
        self.compressed.retain(|(root_index, path), _| {
            work.iter()
                .any(|(index, candidate, _)| index == root_index && candidate == path)
        });
        work.sort_by(|left, right| {
            right
                .2
                .cmp(&left.2)
                .then_with(|| {
                    self.compressed
                        .contains_key(&(right.0, right.1.clone()))
                        .cmp(&self.compressed.contains_key(&(left.0, left.1.clone())))
                })
                .then_with(|| left.0.cmp(&right.0))
                .then_with(|| left.1.cmp(&right.1))
        });
        if work.len() > 3 {
            let index = self.fair_offset % work.len();
            self.fair_offset = self.fair_offset.wrapping_add(1);
            if index > 3 {
                let fair = work.remove(index);
                work.insert(3, fair);
            }
        }
        let mut advanced_streams = HashSet::new();
        for (work_index, (root_index, path, modified)) in work.into_iter().enumerate() {
            if budget.remaining < MIN_FILE_BUDGET
                || work_index >= MAX_POLL_READ_FILES
                || result.events.len() >= MAX_POLL_EVENTS
            {
                self.roots[root_index].pending_files += 1;
                continue;
            }
            let key = (root_index, path.clone());
            let compressed = is_compressed(&path);
            if compressed
                && !self.compressed.contains_key(&key)
                && self.compressed.len() >= MAX_COMPRESSED_STREAMS
            {
                let victim = self
                    .compressed
                    .keys()
                    .filter(|key| !advanced_streams.contains(*key))
                    .filter_map(|key| {
                        let stamp = self.roots[key.0].files.get(&key.1)?.modified;
                        (stamp < modified).then_some((stamp, key.clone()))
                    })
                    .min()
                    .map(|(_, key)| key);
                if let Some(victim) = victim {
                    self.compressed.remove(&victim);
                } else {
                    self.roots[root_index].pending_files += 1;
                    continue;
                }
            }
            let state = &mut self.roots[root_index];
            let old_cursor = state.cursors.get(&path);
            let mut runtime = self.compressed.remove(&key);
            let read = if compressed {
                read_compressed(
                    &state.root,
                    &state.root_key,
                    &path,
                    old_cursor,
                    now,
                    &mut budget,
                    &mut runtime,
                )
            } else {
                read_file(
                    &state.root,
                    &state.root_key,
                    &path,
                    old_cursor,
                    now,
                    &mut budget,
                )
            };
            match read {
                Ok(mut read) => {
                    let changed_cursor = old_cursor != Some(&read.cursor);
                    if changed_cursor || !read.events.is_empty() {
                        read.cursor.last_seen_at = Some(now);
                        match self.store.append(&read.events, &read.cursor) {
                            Ok(events) => {
                                state.ever_seen = true;
                                result.events.extend(events);
                            }
                            Err(_) => {
                                state.io_error = true;
                                warning(
                                    &mut result.warnings,
                                    "Agent 观测存储暂不可用，文件游标未推进，将重试",
                                );
                                continue;
                            }
                        }
                        state.last_event_at = state
                            .last_event_at
                            .into_iter()
                            .chain(read.cursor.last_event_at)
                            .max();
                        state.cursors.insert(path.clone(), read.cursor);
                    }
                    if let Some(candidate) = state.files.get_mut(&path) {
                        candidate.last_stamp = Some(read.stamp);
                        candidate.last_verified = Some(now);
                        candidate.caught_up = read.caught_up;
                    }
                    if !read.caught_up {
                        state.pending_files += 1;
                        if let Some(runtime) = runtime {
                            advanced_streams.insert(key.clone());
                            self.compressed.insert(key, runtime);
                        }
                    }
                    state.skipped_lines += read.skipped_lines;
                    if read.reset {
                        warning(
                            &mut result.warnings,
                            "检测到日志轮转或改写，已重置文件游标并按事件标识去重",
                        );
                    }
                }
                Err(error) => {
                    if matches!(error, LogError::Changed) {
                        state.pending_files += 1;
                        warning(
                            &mut result.warnings,
                            "日志在读取期间发生变动，本批游标未推进，将重试",
                        );
                    } else {
                        state.io_error = true;
                        state.unsafe_path |= matches!(error, LogError::UnsafePath);
                    }
                }
            }
        }
        let current_retention = match self.store.retention_days() {
            Ok(days) => days,
            Err(_) => {
                warning(&mut result.warnings, "Agent 保留设置暂不可读");
                self.retention_days
            }
        };
        if !result.events.is_empty()
            || current_retention != self.retention_days
            || interval_elapsed(self.last_prune, now)
        {
            match self.store.prune(now) {
                Ok(removed) => {
                    result.pruned_events = removed;
                    self.last_prune = Some(now);
                    self.retention_days = current_retention;
                }
                Err(_) => warning(&mut result.warnings, "Agent 观测历史维护暂不可用，将重试"),
            }
        }
        for state in &self.roots {
            if state.skipped_lines != 0 {
                warning(
                    &mut result.warnings,
                    &format!(
                        "已跳过 {} 条损坏、超长或输出超限的完整日志行",
                        state.skipped_lines
                    ),
                );
            }
            if state.rejected_links != 0 || state.unsafe_path {
                warning(&mut result.warnings, "已拒绝链接或重解析路径");
            }
            if state.limited {
                warning(
                    &mut result.warnings,
                    "日志发现达到目录、文件或深度上限，历史覆盖不完整",
                );
            }
            if state
                .cursors
                .values()
                .any(|cursor| cursor.compressed.as_ref().is_some_and(|state| state.failed))
            {
                warning(
                    &mut result.warnings,
                    "压缩历史含损坏帧或超出解码窗口上限；仅保留已验证完整帧，等待文件更新",
                );
            }
            if state.cursors.values().any(|cursor| {
                cursor
                    .compressed
                    .as_ref()
                    .is_some_and(|state| state.incomplete)
            }) {
                warning(
                    &mut result.warnings,
                    "压缩历史末帧尚未完整写入，等待补齐；完整帧已分批导入",
                );
            }
            if state.cursors.values().any(|cursor| {
                cursor
                    .seed_scan
                    .as_ref()
                    .is_some_and(|scan| !scan.complete && scan.session_id.is_some())
            }) {
                warning(
                    &mut result.warnings,
                    "DSH seeded 日志正在预扫描或等待完整 end-seed，继承用量尚未导入",
                );
            }
            result.pending_files += state.pending_files;
            let mut source = source_view(state, now);
            for capability in parser_diagnostics(state) {
                source.detail.push_str(&capability.reason);
                warning(&mut result.warnings, &capability.reason);
                source.capabilities.push(capability);
            }
            result.sources.push(source);
        }
        result
            .events
            .sort_by(|left, right| (left.timestamp, &left.id).cmp(&(right.timestamp, &right.id)));
        result.bytes_read = budget.used;
        result.decoded_bytes = budget.decoded;
        let mut status_sources = result.sources.clone();
        for source in &mut status_sources {
            source.last_checked_at = None;
        }
        let status = serde_json::to_string(&(&status_sources, &result.warnings))
            .map_err(|_| "无法编码 Agent 来源状态")?;
        result.changed = !result.events.is_empty()
            || result.pruned_events != 0
            || status != self.previous_status;
        self.previous_status = status;
        Ok(result)
    }
}

fn interval_elapsed(previous: Option<i64>, now: i64) -> bool {
    previous.is_none_or(|last| now < last || now.saturating_sub(last) >= DISCOVERY_INTERVAL_MS)
}

fn warning(warnings: &mut Vec<String>, value: &str) {
    if warnings.len() < 32 && !warnings.iter().any(|warning| warning == value) {
        warnings.push(value.to_owned());
    }
}

fn root_key(path: &Path) -> String {
    #[cfg(windows)]
    {
        digest(
            path.to_string_lossy()
                .replace('\\', "/")
                .to_lowercase()
                .as_bytes(),
        )
    }
    #[cfg(not(windows))]
    {
        digest(path.as_os_str().as_encoded_bytes())
    }
}

fn source_view(state: &RootState, now: i64) -> AgentSource {
    let connection = if !state.online || state.io_error {
        if state.ever_seen {
            "disconnected"
        } else {
            "unavailable"
        }
    } else {
        "connected"
    };
    let mut detail = if state.unsafe_path {
        "已拒绝链接或重解析路径，等待可读的普通日志目录。".to_string()
    } else if connection == "unavailable" {
        "尚未发现可读日志目录，等待来源可用。".to_string()
    } else if connection == "disconnected" {
        "日志来源暂不可读，等待恢复；已观测状态保留。".to_string()
    } else {
        "只读日志约每 2 秒检查；新文件最多 15 秒发现，另有客户端写入延迟。".to_string()
    };
    if state.pending_files != 0 {
        detail.push_str(&format!("还有 {} 个文件分批读取。", state.pending_files));
    }
    if state.limited {
        detail.push_str("发现范围达到上限，历史覆盖不完整。");
    }
    if state.compressed_history {
        detail.push_str(
            "压缩日志按历史模式分批导入，仅处理验证完成的帧；末帧或末行不完整时等待补齐。",
        );
    }
    if state.skipped_lines != 0 {
        detail.push_str(&format!("已跳过 {} 条无效完整行。", state.skipped_lines));
    }
    let mut capabilities = vec![AgentCapability {
        feature: "collection".into(),
        mode: "delayed".into(),
        reason: "本地 JSONL 文件只读采集，源写入和目录发现存在延迟".into(),
    }];
    if state.root.client_id == "deepseek_harness" {
        capabilities.push(AgentCapability {
            feature: "compressed_history".into(), mode: "history".into(),
            reason: "最高 generation 的 zstd 历史逐完整帧验证并有界回放；重启从帧检查点恢复，解码窗口上限 8 MiB".into(),
        });
        capabilities.push(AgentCapability {
            feature: "seeded_history".into(),
            mode: "delayed".into(),
            reason: "有界预扫描最后一个完整 inherited end-seed 后再回放；边界未完成时等待写入"
                .into(),
        });
    }
    AgentSource {
        id: state.root.id.clone(),
        client_id: state.root.client_id.clone(),
        name: if state.root.client_id == "codex" {
            "Codex 会话日志"
        } else {
            "DeepSeek Harness 会话日志"
        }
        .into(),
        connection: connection.into(),
        mode: if state.compressed_history {
            "history"
        } else {
            "delayed"
        }
        .into(),
        last_checked_at: Some(now),
        last_event_at: state.last_event_at,
        file_count: state.file_count,
        detail,
        capabilities,
    }
}

fn parser_diagnostics(state: &RootState) -> Vec<AgentCapability> {
    if state.root.client_id != "deepseek_harness" {
        return Vec::new();
    }
    let mut blocked = false;
    let mut sequence_gap = false;
    let mut partial_links = false;
    let mut unsupported_team = false;
    let mut unsupported_descriptor = false;
    for path in state.files.keys() {
        let Some(cursor) = state.cursors.get(path) else {
            continue;
        };
        let parser = &cursor.parser_state;
        blocked |= parser.get("blocked").is_some_and(|value| !value.is_null());
        sequence_gap |= parser["blocked"].as_str() == Some("sequence_gap");
        partial_links |= parser["partialLinks"].as_bool() == Some(true);
        unsupported_team |= parser["unsupportedTeamEvents"].as_bool() == Some(true);
        unsupported_descriptor |= parser["unsupportedDescriptor"].as_bool() == Some(true);
    }
    let mut diagnostics = Vec::new();
    for (present, feature, reason) in [
        (
            blocked,
            "parser_coverage",
            if sequence_gap {
                "DSH 日志存在序列缺口，后续解析已暂停，覆盖不完整；等待完整文件修复、轮转或新 generation。"
            } else {
                "DSH 日志版本或结构无法继续解析，覆盖不完整；等待完整文件修复、轮转或新 generation。"
            },
        ),
        (
            partial_links,
            "complete_relationships",
            "DSH 关联跟踪达到上限，部分任务或工具归属不可用；已确认关系保留。",
        ),
        (
            unsupported_team,
            "team_protocol",
            "DSH 日志包含尚不支持的团队事件，该部分编队与任务覆盖不完整。",
        ),
        (
            unsupported_descriptor,
            "model_descriptor",
            "DSH 模型描述格式不受支持，相关模型与上下文元数据不可用。",
        ),
    ] {
        if present {
            diagnostics.push(AgentCapability {
                feature: feature.into(),
                mode: "unsupported".into(),
                reason: reason.into(),
            });
        }
    }
    diagnostics
}

fn checked_chain(path: &Path) -> Result<Metadata, LogError> {
    let mut current = PathBuf::new();
    let mut last = None;
    for component in path.components() {
        match component {
            Component::ParentDir | Component::CurDir => return Err(LogError::UnsafePath),
            Component::Prefix(_) => {
                current.push(component.as_os_str());
                continue;
            }
            _ => current.push(component.as_os_str()),
        }
        let metadata = fs::symlink_metadata(&current).map_err(|_| LogError::Unreadable)?;
        if is_link(&metadata) {
            return Err(LogError::UnsafePath);
        }
        last = Some(metadata);
    }
    last.ok_or(LogError::UnsafePath)
}

fn readable_root(root: &Path) -> Result<(), LogError> {
    #[cfg(windows)]
    let _guards = directory_guards(root)?;
    let metadata = checked_chain(root)?;
    if !metadata.is_dir() {
        return Err(LogError::UnsafePath);
    }
    fs::read_dir(root).map_err(|_| LogError::Unreadable)?;
    Ok(())
}

fn safe_metadata(root: &Path, relative: &str) -> Result<Metadata, LogError> {
    if !relative_path_is_safe(relative) {
        return Err(LogError::UnsafePath);
    }
    let metadata = checked_chain(&root.join(relative))?;
    if !metadata.is_file() {
        return Err(LogError::UnsafePath);
    }
    Ok(metadata)
}

fn modified_order(metadata: &Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos())
}

fn file_stamp(metadata: &Metadata) -> FileStamp {
    let nanos = |value: io::Result<std::time::SystemTime>| {
        value
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos().to_string())
    };
    #[cfg(unix)]
    let identity_hint = {
        use std::os::unix::fs::MetadataExt;
        Some(format!("{}:{}", metadata.dev(), metadata.ino()))
    };
    #[cfg(not(unix))]
    let identity_hint = None;
    FileStamp {
        len: metadata.len(),
        modified: nanos(metadata.modified()),
        created: nanos(metadata.created()),
        identity_hint,
    }
}

fn harness_generation(name: &str) -> Option<(u32, bool)> {
    let (plain, compressed) = name
        .strip_suffix(".zstd")
        .or_else(|| name.strip_suffix(".zst"))
        .map_or((name, false), |plain| (plain, true));
    if plain == "session.jsonl" {
        return Some((0, compressed));
    }
    let version = plain.strip_prefix("session.v")?.strip_suffix(".jsonl")?;
    if version.is_empty()
        || version.starts_with('0')
        || !version.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some((version.parse().ok()?, compressed))
}

fn is_compressed(path: &str) -> bool {
    path.ends_with(".zstd") || path.ends_with(".zst")
}

fn discover(root: &SourceRoot) -> Discovery {
    let mut result = Discovery::default();
    let mut pending = vec![(root.path.clone(), 0)];
    let mut directories = 0;
    let mut entries_seen = 0;
    while let Some((directory, depth)) = pending.pop() {
        if directories >= MAX_DIRECTORIES || entries_seen >= MAX_ENTRIES {
            result.limited = true;
            break;
        }
        directories += 1;
        match checked_chain(&directory) {
            Ok(metadata) if metadata.is_dir() => {}
            Err(LogError::UnsafePath) => {
                result.rejected_links += 1;
                continue;
            }
            _ => {
                result.io_error = true;
                continue;
            }
        }
        #[cfg(windows)]
        let _guards = match directory_guards(&directory) {
            Ok(guards) => guards,
            Err(LogError::UnsafePath) => {
                result.rejected_links += 1;
                continue;
            }
            Err(_) => {
                result.io_error = true;
                continue;
            }
        };
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => {
                result.io_error = true;
                continue;
            }
        };
        let mut children = Vec::new();
        let mut generation: Option<(u32, bool, Candidate)> = None;
        for entry in entries {
            if entries_seen >= MAX_ENTRIES {
                result.limited = true;
                break;
            }
            entries_seen += 1;
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    result.io_error = true;
                    continue;
                }
            };
            let path = entry.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    result.io_error = true;
                    continue;
                }
            };
            if is_link(&metadata) {
                result.rejected_links += 1;
                continue;
            }
            if metadata.is_dir() {
                if depth < MAX_DEPTH {
                    children.push((modified_order(&metadata), path, depth + 1));
                } else {
                    result.limited = true;
                }
                continue;
            }
            if !metadata.is_file() {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let relative = match path.strip_prefix(&root.path).ok().and_then(Path::to_str) {
                Some(relative) => relative.replace('\\', "/"),
                None => continue,
            };
            if !relative_path_is_safe(&relative) {
                continue;
            }
            let candidate = Candidate {
                path: relative,
                modified: modified_order(&metadata),
                ..Default::default()
            };
            if root.client_id == "codex" {
                if name.starts_with("rollout-")
                    && name.ends_with(".jsonl")
                    && name.len() > "rollout-.jsonl".len()
                {
                    result.files.push(candidate);
                }
            } else if let Some((version, compressed)) = harness_generation(name) {
                if generation
                    .as_ref()
                    .is_none_or(|old| (version, compressed) > (old.0, old.1))
                {
                    generation = Some((version, compressed, candidate));
                }
            }
        }
        if let Some((_, compressed, candidate)) = generation {
            result.compressed_history |= compressed;
            result.files.push(candidate);
        }
        children.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
        pending.extend(children.into_iter().map(|(_, path, depth)| (path, depth)));
    }
    result.file_count = result.files.len();
    result.files.sort_by(|left, right| {
        right
            .modified
            .cmp(&left.modified)
            .then_with(|| left.path.cmp(&right.path))
    });
    if result.files.len() > MAX_FILES {
        result.files.truncate(MAX_FILES);
        result.limited = true;
    }
    result
}

#[cfg(windows)]
fn directory_guards(path: &Path) -> Result<Vec<File>, LogError> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let mut current = PathBuf::new();
    let mut guards = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir | Component::CurDir => return Err(LogError::UnsafePath),
            Component::Prefix(_) => {
                current.push(component.as_os_str());
                continue;
            }
            _ => current.push(component.as_os_str()),
        }
        let handle = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&current)
            .map_err(|_| LogError::Unreadable)?;
        let metadata = handle.metadata().map_err(|_| LogError::Unreadable)?;
        if is_link(&metadata) || !metadata.is_dir() {
            return Err(LogError::UnsafePath);
        }
        guards.push(handle);
    }
    if guards.is_empty() {
        return Err(LogError::UnsafePath);
    }
    Ok(guards)
}

#[cfg(windows)]
fn open_log(root: &Path, relative: &str) -> Result<OpenedLog, LogError> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    if !relative_path_is_safe(relative) {
        return Err(LogError::UnsafePath);
    }
    let path = root.join(relative);
    let parents = directory_guards(path.parent().ok_or(LogError::UnsafePath)?)?;
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| LogError::Unreadable)?;
    let metadata = file.metadata().map_err(|_| LogError::Unreadable)?;
    if is_link(&metadata) || !metadata.is_file() {
        return Err(LogError::UnsafePath);
    }
    Ok(OpenedLog {
        file,
        _parents: parents,
    })
}

#[cfg(unix)]
fn open_log(root: &Path, relative: &str) -> Result<OpenedLog, LogError> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    if !relative_path_is_safe(relative) || !root.is_absolute() {
        return Err(LogError::UnsafePath);
    }
    let path = root.join(relative);
    let mut handle = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/")
        .map_err(|_| LogError::Unreadable)?;
    let mut parts = path.components().peekable();
    while let Some(component) = parts.next() {
        let name = match component {
            Component::RootDir => continue,
            Component::Normal(name) => name,
            _ => return Err(LogError::UnsafePath),
        };
        let name = CString::new(name.as_bytes()).map_err(|_| LogError::UnsafePath)?;
        let directory = parts.peek().is_some();
        let flags = libc::O_RDONLY
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | if directory {
                libc::O_DIRECTORY
            } else {
                libc::O_NONBLOCK
            };
        let descriptor = unsafe { libc::openat(handle.as_raw_fd(), name.as_ptr(), flags) };
        if descriptor < 0 {
            return Err(
                if io::Error::last_os_error().raw_os_error() == Some(libc::ELOOP) {
                    LogError::UnsafePath
                } else {
                    LogError::Unreadable
                },
            );
        }
        handle = unsafe { File::from_raw_fd(descriptor) };
        let metadata = handle.metadata().map_err(|_| LogError::Unreadable)?;
        if is_link(&metadata)
            || (directory && !metadata.is_dir())
            || (!directory && !metadata.is_file())
        {
            return Err(LogError::UnsafePath);
        }
    }
    Ok(OpenedLog { file: handle })
}

#[cfg(not(any(unix, windows)))]
fn open_log(_: &Path, _: &str) -> Result<OpenedLog, LogError> {
    Err(LogError::UnsafePath)
}

fn file_identity(file: &File, metadata: &Metadata) -> Result<String, LogError> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(LogError::Unreadable);
        }
        let _ = metadata;
        Ok(format!(
            "win:{:x}:{:x}:{:x}:{:x}:{:x}",
            info.dwVolumeSerialNumber,
            info.nFileIndexHigh,
            info.nFileIndexLow,
            info.ftCreationTime.dwHighDateTime,
            info.ftCreationTime.dwLowDateTime
        ))
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = file;
        Ok(format!(
            "unix:{}:{}:{}",
            metadata.dev(),
            metadata.ino(),
            file_stamp(metadata).created.unwrap_or_default()
        ))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (file, metadata);
        Err(LogError::UnsafePath)
    }
}

fn read_range(
    file: &mut File,
    offset: u64,
    length: usize,
    budget: &mut ReadBudget,
) -> Result<Vec<u8>, LogError> {
    if length > budget.remaining {
        return Err(LogError::Changed);
    }
    if length == 0 {
        return Ok(Vec::new());
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| LogError::Unreadable)?;
    let mut bytes = vec![0; length];
    let mut filled = 0;
    while filled < length {
        match file.read(&mut bytes[filled..]) {
            Ok(0) => return Err(LogError::Changed),
            Ok(count) => {
                filled += count;
                budget.remaining -= count;
                budget.used += count as u64;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(LogError::Unreadable),
        }
    }
    Ok(bytes)
}

fn charge_budget(budget: &mut ReadBudget, used: &ReadBudget) {
    budget.remaining -= (used.used + used.decoded) as usize;
    budget.used += used.used;
    budget.decoded += used.decoded;
}

fn validate_frames(
    file: &mut File,
    length: u64,
    state: &mut CompressedCursor,
    runtime: &mut CompressedRuntime,
    budget: &mut ReadBudget,
    proof: &mut PhysicalProof,
) -> Result<(), LogError> {
    if state.failed || state.incomplete || state.verified_offset == length {
        runtime.validation = None;
        return Ok(());
    }
    if runtime.validation.is_none() {
        runtime.validation = Some(DecodeStream::new(FramePoint {
            raw: state.verified_offset,
            decoded: state.verified_decoded,
        })?);
    }
    let stream = runtime.validation.as_mut().ok_or(LogError::Changed)?;
    for _ in 0..MAX_ZSTD_STEPS {
        match stream.step(file, length, budget, proof)? {
            DecodeStep::Data(_) => {
                state.verified_offset = stream.boundary.raw;
                state.verified_decoded = stream.boundary.decoded;
            }
            DecodeStep::End => {
                runtime.validation = None;
                break;
            }
            DecodeStep::Incomplete => {
                state.incomplete = true;
                runtime.validation = None;
                break;
            }
            DecodeStep::Invalid => {
                state.failed = true;
                runtime.validation = None;
                break;
            }
            DecodeStep::Budget => break,
        }
    }
    Ok(())
}

fn replay_target(cursor: &FileCursor, scanning: bool) -> u64 {
    if scanning {
        cursor
            .seed_scan
            .as_ref()
            .map_or(0, |scan| scan.offset + scan.discard_bytes)
    } else {
        cursor.offset + cursor.discard_bytes
    }
}

fn save_frame_point(cursor: &FileCursor, state: &mut CompressedCursor, replay: &ReplayStream) {
    let point = replay.stream.boundary;
    if point.decoded > replay_target(cursor, replay.scanning) {
        return;
    }
    if replay.scanning {
        state.scan_offset = point.raw;
        state.scan_decoded = point.decoded;
    } else {
        state.replay_offset = point.raw;
        state.replay_decoded = point.decoded;
    }
}

fn consume_decoded(
    root: &SourceRoot,
    relative: &str,
    now: i64,
    cursor: &mut FileCursor,
    replay: &mut ReplayStream,
    events: &mut Vec<AgentEvent>,
    skipped_lines: &mut usize,
) -> bool {
    let mut position = 0;
    let mut full = false;
    while let Some(newline) = replay.pending[position..]
        .iter()
        .position(|byte| *byte == b'\n')
    {
        let end = position + newline + 1;
        let (discard, line_number) = if replay.scanning {
            let scan = cursor.seed_scan.as_ref().expect("seed scan initialized");
            (scan.discard_bytes, scan.line_number)
        } else {
            (cursor.discard_bytes, cursor.line_number)
        };
        let mut invalid = discard != 0 || newline > MAX_LINE_BYTES;
        let mut bytes = &replay.pending[position..end - 1];
        if line_number == 0 {
            bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
        }
        let record = if invalid || bytes.iter().all(u8::is_ascii_whitespace) {
            None
        } else {
            match serde_json::from_slice::<Value>(bytes) {
                Ok(record) => Some(record),
                Err(_) => {
                    invalid = true;
                    None
                }
            }
        };
        if replay.scanning {
            let scan = cursor.seed_scan.as_mut().expect("seed scan initialized");
            if !scan.header_checked {
                scan.header_checked = true;
                scan.session_id = record
                    .as_ref()
                    .filter(|record| {
                        record["type"] == "session"
                            && record["version"].as_u64() == Some(4)
                            && record["isSeeded"].as_bool() == Some(true)
                    })
                    .and_then(|record| record["id"].as_str())
                    .and_then(super::privacy::identifier);
                if scan.session_id.is_none() {
                    scan.complete = true;
                    cursor.offset = scan.offset;
                    cursor.line_number = scan.line_number;
                    cursor.discard_bytes = scan.discard_bytes;
                    replay.scanning = false;
                    continue;
                }
            } else if let Some(record) = record.as_ref().filter(|record| {
                record["type"] == "session/end-seed"
                    && record["data"]["inherited"].as_bool() == Some(true)
            }) {
                scan.boundary_seq = record["seq"]
                    .as_u64()
                    .filter(|seq| *seq <= 9_007_199_254_740_991);
            }
            scan.offset += scan.discard_bytes + (end - position) as u64;
            scan.line_number += 1;
            scan.discard_bytes = 0;
        } else {
            if let Some(record) = record {
                let context = LineContext {
                    source_id: &root.id,
                    client_id: &root.client_id,
                    source_path: relative,
                    line_number: cursor.line_number + 1,
                    observed_at: now,
                };
                let mut next_state = cursor.parser_state.clone();
                let mut parsed = (root.adapter)(&record, &context, &mut next_state);
                let state_ok = serde_json::to_vec(&next_state)
                    .is_ok_and(|encoded| encoded.len() <= 1024 * 1024);
                if parsed.len() > MAX_RECORD_EVENTS || !state_ok {
                    invalid = true;
                } else {
                    if events.len() + parsed.len() > MAX_BATCH_EVENTS {
                        full = true;
                        break;
                    }
                    for event in &mut parsed {
                        event.source_id.clone_from(&root.id);
                        event.client_id.clone_from(&root.client_id);
                        event.source_path = relative.into();
                        event.source_line = context.line_number;
                        event.observed_at = now;
                        cursor.last_event_at = cursor
                            .last_event_at
                            .into_iter()
                            .chain(Some(event.timestamp))
                            .max();
                    }
                    cursor.parser_state = next_state;
                    events.extend(parsed);
                }
            }
            if invalid {
                *skipped_lines += 1;
                cursor.malformed_lines = cursor.malformed_lines.saturating_add(1);
            }
            cursor.offset += cursor.discard_bytes + (end - position) as u64;
            cursor.line_number += 1;
            cursor.discard_bytes = 0;
        }
        position = end;
    }
    replay.pending.drain(..position);
    if !full {
        let discard = if replay.scanning {
            &mut cursor
                .seed_scan
                .as_mut()
                .expect("seed scan initialized")
                .discard_bytes
        } else {
            &mut cursor.discard_bytes
        };
        if *discard != 0 || replay.pending.len() > MAX_LINE_BYTES {
            *discard += replay.pending.len() as u64;
            replay.pending.clear();
        }
    }
    full
}

fn replay_frames(
    root: &SourceRoot,
    relative: &str,
    file: &mut File,
    now: i64,
    cursor: &mut FileCursor,
    state: &mut CompressedCursor,
    runtime: &mut CompressedRuntime,
    budget: &mut ReadBudget,
    proof: &mut PhysicalProof,
    events: &mut Vec<AgentEvent>,
    skipped_lines: &mut usize,
) -> Result<bool, LogError> {
    let scanning = !cursor
        .seed_scan
        .get_or_insert_with(SeedScanCursor::default)
        .complete;
    if runtime
        .replay
        .as_ref()
        .is_none_or(|replay| replay.scanning != scanning)
    {
        let point = if scanning {
            FramePoint {
                raw: state.scan_offset,
                decoded: state.scan_decoded,
            }
        } else {
            FramePoint {
                raw: state.replay_offset,
                decoded: state.replay_decoded,
            }
        };
        runtime.replay = Some(ReplayStream {
            stream: DecodeStream::new(point)?,
            pending: Vec::new(),
            scanning,
        });
    }
    let replay = runtime.replay.as_mut().ok_or(LogError::Changed)?;
    for _ in 0..MAX_ZSTD_STEPS {
        let full = consume_decoded(root, relative, now, cursor, replay, events, skipped_lines);
        save_frame_point(cursor, state, replay);
        if full {
            return Ok(false);
        }
        let start = replay.stream.decoded;
        match replay
            .stream
            .step(file, state.verified_offset, budget, proof)?
        {
            DecodeStep::Data(bytes) => {
                let skip = replay_target(cursor, replay.scanning)
                    .saturating_sub(start)
                    .min(bytes.len() as u64) as usize;
                replay.pending.extend_from_slice(&bytes[skip..]);
            }
            DecodeStep::End => return Ok(true),
            DecodeStep::Budget => return Ok(false),
            DecodeStep::Incomplete | DecodeStep::Invalid => return Err(LogError::Changed),
        }
    }
    Ok(false)
}

fn read_compressed(
    root: &SourceRoot,
    root_key: &str,
    relative: &str,
    previous: Option<&FileCursor>,
    now: i64,
    budget: &mut ReadBudget,
    runtime: &mut Option<CompressedRuntime>,
) -> Result<FileRead, LogError> {
    let mut local = ReadBudget {
        remaining: budget.remaining.min(MAX_FILE_BYTES),
        used: 0,
        decoded: 0,
    };
    let result =
        read_compressed_bounded(root, root_key, relative, previous, now, &mut local, runtime);
    charge_budget(budget, &local);
    result
}

fn read_compressed_bounded(
    root: &SourceRoot,
    root_key: &str,
    relative: &str,
    previous: Option<&FileCursor>,
    now: i64,
    budget: &mut ReadBudget,
    runtime: &mut Option<CompressedRuntime>,
) -> Result<FileRead, LogError> {
    let mut opened = open_log(&root.path, relative)?;
    let metadata = opened.file.metadata().map_err(|_| LogError::Unreadable)?;
    let snapshot = file_stamp(&metadata);
    if snapshot.len > i64::MAX as u64 {
        return Err(LogError::Unreadable);
    }
    let identity = file_identity(&opened.file, &metadata)?;
    let header = read_range(
        &mut opened.file,
        0,
        snapshot.len.min(HEADER_BYTES as u64) as usize,
        budget,
    )?;
    let mut cursor = previous.cloned().unwrap_or_default();
    let changed =
        previous.is_none() || cursor.file_len != snapshot.len || cursor.mtime != snapshot.modified;
    let mut reset = previous.is_some()
        && (cursor.source_id != root.id
            || cursor.root_key != root_key
            || cursor.path != relative
            || cursor.file_identity != identity
            || cursor.compressed.is_none()
            || snapshot.len < cursor.file_len
            || (snapshot.len == cursor.file_len && cursor.mtime != snapshot.modified)
            || cursor.header_len > header.len() as u64
            || digest(&header[..(cursor.header_len as usize).min(header.len())])
                != cursor.header_hash);
    let mut proof = PhysicalProof {
        end: 0,
        tail: Vec::new(),
    };
    if previous.is_some() && !reset {
        let state = cursor.compressed.as_ref().ok_or(LogError::Changed)?;
        if state.inspected_offset > snapshot.len || cursor.tail_len > state.inspected_offset {
            reset = true;
        } else {
            proof.end = state.inspected_offset;
            proof.tail = read_range(
                &mut opened.file,
                proof.end - cursor.tail_len,
                cursor.tail_len as usize,
                budget,
            )?;
            reset = digest(&proof.tail) != cursor.tail_hash;
        }
    }
    if reset || previous.is_none() {
        cursor = FileCursor {
            source_id: root.id.clone(),
            root_key: root_key.into(),
            path: relative.into(),
            parser_state: serde_json::json!({}),
            last_event_at: cursor.last_event_at,
            last_seen_at: cursor.last_seen_at,
            malformed_lines: cursor.malformed_lines,
            compressed: Some(CompressedCursor::default()),
            ..Default::default()
        };
        proof = PhysicalProof {
            end: 0,
            tail: Vec::new(),
        };
        *runtime = None;
    }
    let mut state = cursor.compressed.take().ok_or(LogError::Changed)?;
    let mut active = runtime.take().unwrap_or_default();
    if changed || reset {
        state.waiting_for_change = false;
        state.incomplete = false;
        state.failed = false;
        if let Some(scan) = cursor.seed_scan.as_mut()
            && scan.complete
            && scan.session_id.is_some()
            && cursor.offset == 0
        {
            scan.complete = false;
            cursor.parser_state = serde_json::json!({});
            active.replay = None;
            state.replay_offset = 0;
            state.replay_decoded = 0;
        }
    }
    let mut events = Vec::new();
    let mut skipped_lines = 0;
    let mut caught_up = state.waiting_for_change;
    if !caught_up {
        let reserve = header.len() + TAIL_BYTES;
        let mut validation_budget = ReadBudget {
            remaining: budget.remaining.saturating_sub(reserve) / 2,
            used: 0,
            decoded: 0,
        };
        let validation = validate_frames(
            &mut opened.file,
            snapshot.len,
            &mut state,
            &mut active,
            &mut validation_budget,
            &mut proof,
        );
        charge_budget(budget, &validation_budget);
        validation?;
        let mut replay_budget = ReadBudget {
            remaining: budget.remaining.saturating_sub(reserve),
            used: 0,
            decoded: 0,
        };
        let replay = replay_frames(
            root,
            relative,
            &mut opened.file,
            now,
            &mut cursor,
            &mut state,
            &mut active,
            &mut replay_budget,
            &mut proof,
            &mut events,
            &mut skipped_lines,
        );
        charge_budget(budget, &replay_budget);
        let reached_end = replay?;
        caught_up = reached_end
            && (state.verified_offset == snapshot.len || state.incomplete || state.failed);
        if let Some(scan) = cursor.seed_scan.as_mut()
            && !scan.complete
            && scan.session_id.is_some()
            && scan.boundary_seq.is_some()
            && reached_end
            && state.verified_offset == snapshot.len
            && scan.offset == state.verified_decoded
            && scan.discard_bytes == 0
        {
            scan.complete = true;
            cursor.parser_state = serde_json::json!({
                "seedBoundary": { "sessionId": scan.session_id, "seq": scan.boundary_seq }
            });
            active.replay = None;
            caught_up = false;
        }
    }
    let verified_header = read_range(&mut opened.file, 0, header.len(), budget)?;
    let tail = read_range(
        &mut opened.file,
        proof.end - proof.tail.len() as u64,
        proof.tail.len(),
        budget,
    )?;
    let final_metadata = opened.file.metadata().map_err(|_| LogError::Unreadable)?;
    if header != verified_header
        || tail != proof.tail
        || file_stamp(&final_metadata) != snapshot
        || file_identity(&opened.file, &final_metadata)? != identity
    {
        return Err(LogError::Changed);
    }
    state.inspected_offset = proof.end;
    state.waiting_for_change = caught_up;
    cursor.compressed = Some(state);
    cursor.file_len = snapshot.len;
    cursor.mtime = snapshot.modified.clone();
    cursor.file_identity = identity;
    cursor.header_len = header.len() as u64;
    cursor.header_hash = digest(&header);
    cursor.tail_len = tail.len() as u64;
    cursor.tail_hash = digest(&tail);
    *runtime = Some(active);
    Ok(FileRead {
        cursor,
        events,
        stamp: snapshot,
        caught_up,
        skipped_lines,
        reset,
    })
}

fn read_file(
    root: &SourceRoot,
    root_key: &str,
    relative: &str,
    previous: Option<&FileCursor>,
    now: i64,
    budget: &mut ReadBudget,
) -> Result<FileRead, LogError> {
    let mut local = ReadBudget {
        remaining: budget.remaining.min(MAX_FILE_BYTES),
        used: 0,
        decoded: 0,
    };
    let result = read_file_bounded(root, root_key, relative, previous, now, &mut local);
    budget.remaining -= local.used as usize;
    budget.used += local.used;
    result
}

fn read_file_bounded(
    root: &SourceRoot,
    root_key: &str,
    relative: &str,
    previous: Option<&FileCursor>,
    now: i64,
    budget: &mut ReadBudget,
) -> Result<FileRead, LogError> {
    let mut opened = open_log(&root.path, relative)?;
    let metadata = opened.file.metadata().map_err(|_| LogError::Unreadable)?;
    let snapshot = file_stamp(&metadata);
    if snapshot.len > i64::MAX as u64 {
        return Err(LogError::Unreadable);
    }
    let identity = file_identity(&opened.file, &metadata)?;
    let header = read_range(
        &mut opened.file,
        0,
        snapshot.len.min(HEADER_BYTES as u64) as usize,
        budget,
    )?;
    let mut cursor = previous.cloned().unwrap_or_else(|| FileCursor {
        source_id: root.id.clone(),
        root_key: root_key.into(),
        path: relative.into(),
        parser_state: serde_json::json!({}),
        ..Default::default()
    });
    let mut reset = previous.is_some()
        && (cursor.source_id != root.id
            || cursor.root_key != root_key
            || cursor.path != relative
            || cursor.file_identity != identity
            || snapshot.len < cursor.file_len
            || cursor.offset > snapshot.len
            || cursor.discard_bytes > snapshot.len.saturating_sub(cursor.offset)
            || cursor.header_len > header.len() as u64
            || digest(&header[..(cursor.header_len as usize).min(header.len())])
                != cursor.header_hash
            || (root.client_id == "deepseek_harness"
                && cursor.seed_scan.is_none()
                && cursor.offset != 0));
    let mut committed_tail = Vec::new();
    if previous.is_some() && !reset {
        committed_tail = read_range(
            &mut opened.file,
            cursor.offset - cursor.tail_len,
            cursor.tail_len as usize,
            budget,
        )?;
        reset = digest(&committed_tail) != cursor.tail_hash;
    }
    if !reset
        && let Some(scan) = cursor
            .seed_scan
            .as_ref()
            .filter(|scan| !scan.complete || cursor.offset == 0)
    {
        let end = scan.offset + scan.discard_bytes;
        let tail = read_range(
            &mut opened.file,
            end - scan.tail_len,
            scan.tail_len as usize,
            budget,
        )?;
        if !scan.tail_hash.is_empty() {
            reset = digest(&tail) != scan.tail_hash;
        }
    }
    if reset {
        cursor = FileCursor {
            source_id: root.id.clone(),
            root_key: root_key.into(),
            path: relative.into(),
            parser_state: serde_json::json!({}),
            last_event_at: cursor.last_event_at,
            last_seen_at: cursor.last_seen_at,
            malformed_lines: cursor.malformed_lines,
            ..Default::default()
        };
        committed_tail.clear();
    }
    if root.client_id == "deepseek_harness" {
        if let Some(scan) = cursor.seed_scan.as_mut()
            && scan.complete
            && scan.session_id.is_some()
            && cursor.offset == 0
            && scan.offset < snapshot.len
        {
            scan.complete = false;
            cursor.parser_state = serde_json::json!({});
        }
        if cursor.seed_scan.is_none()
            && let Some(newline) = header.iter().position(|byte| *byte == b'\n')
        {
            let first = header[..newline]
                .strip_prefix(&[0xef, 0xbb, 0xbf])
                .unwrap_or(&header[..newline]);
            if serde_json::from_slice::<Value>(first)
                .ok()
                .is_none_or(|record| {
                    record["type"] != "session" || record["isSeeded"].as_bool() != Some(true)
                })
            {
                cursor.seed_scan = Some(SeedScanCursor {
                    header_checked: true,
                    complete: true,
                    ..Default::default()
                });
            }
        }
        if cursor.seed_scan.as_ref().is_none_or(|scan| !scan.complete) {
            return scan_seed(
                &mut opened.file,
                cursor,
                snapshot,
                identity,
                &header,
                reset,
                budget,
            );
        }
    }
    let start = cursor.offset + cursor.discard_bytes;
    let before_data = if cursor.discard_bytes == 0 {
        committed_tail.clone()
    } else {
        read_range(
            &mut opened.file,
            start.saturating_sub(TAIL_BYTES as u64),
            start.min(TAIL_BYTES as u64) as usize,
            budget,
        )?
    };
    let available = budget.remaining.saturating_sub(header.len() + TAIL_BYTES);
    let length = snapshot.len.saturating_sub(start).min(available as u64) as usize;
    let data = read_range(&mut opened.file, start, length, budget)?;
    let mut scan = 0;
    let mut examined_to = start;
    let mut events = Vec::new();
    let mut skipped_lines = 0;
    while scan < data.len() {
        let Some(newline) = data[scan..].iter().position(|byte| *byte == b'\n') else {
            let trailing = data.len() - scan;
            if cursor.discard_bytes != 0 || trailing > MAX_LINE_BYTES {
                cursor.discard_bytes += trailing as u64;
            }
            examined_to = start + data.len() as u64;
            break;
        };
        let end = scan + newline + 1;
        let mut invalid = cursor.discard_bytes != 0 || newline > MAX_LINE_BYTES;
        if !invalid {
            let mut line = &data[scan..end - 1];
            if cursor.line_number == 0 && cursor.offset == 0 {
                line = line.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(line);
            }
            if !line.iter().all(u8::is_ascii_whitespace) {
                match serde_json::from_slice::<Value>(line) {
                    Ok(record) => {
                        let mut next_state = cursor.parser_state.clone();
                        let context = LineContext {
                            source_id: &root.id,
                            client_id: &root.client_id,
                            source_path: relative,
                            line_number: cursor.line_number + 1,
                            observed_at: now,
                        };
                        let mut parsed = (root.adapter)(&record, &context, &mut next_state);
                        let state_ok = serde_json::to_vec(&next_state)
                            .is_ok_and(|encoded| encoded.len() <= 1024 * 1024);
                        if parsed.len() > MAX_RECORD_EVENTS || !state_ok {
                            invalid = true;
                        } else {
                            if events.len() + parsed.len() > MAX_BATCH_EVENTS {
                                break;
                            }
                            for event in &mut parsed {
                                event.source_id.clone_from(&root.id);
                                event.client_id.clone_from(&root.client_id);
                                event.source_path = relative.into();
                                event.source_line = context.line_number;
                                event.observed_at = now;
                                cursor.last_event_at = cursor
                                    .last_event_at
                                    .into_iter()
                                    .chain(Some(event.timestamp))
                                    .max();
                            }
                            cursor.parser_state = next_state;
                            events.extend(parsed);
                        }
                    }
                    Err(_) => invalid = true,
                }
            }
        }
        if invalid {
            skipped_lines += 1;
            cursor.malformed_lines = cursor.malformed_lines.saturating_add(1);
        }
        cursor.offset = start + end as u64;
        cursor.line_number += 1;
        cursor.discard_bytes = 0;
        scan = end;
        examined_to = cursor.offset;
    }
    let final_metadata = opened.file.metadata().map_err(|_| LogError::Unreadable)?;
    if final_metadata.len() < snapshot.len
        || file_identity(&opened.file, &final_metadata)? != identity
    {
        return Err(LogError::Changed);
    }
    let verified_header = read_range(&mut opened.file, 0, header.len(), budget)?;
    if header != verified_header {
        return Err(LogError::Changed);
    }
    let tail_len = cursor.offset.min(TAIL_BYTES as u64) as usize;
    let tail = read_range(
        &mut opened.file,
        cursor.offset - tail_len as u64,
        tail_len,
        budget,
    )?;
    let expected_tail = if cursor.offset <= start {
        committed_tail
    } else {
        let end = (cursor.offset - start) as usize;
        let data_tail = end.min(tail_len);
        let prefix = tail_len - data_tail;
        if prefix > before_data.len() {
            return Err(LogError::Changed);
        }
        let mut expected = before_data[before_data.len() - prefix..].to_vec();
        expected.extend_from_slice(&data[end - data_tail..end]);
        expected
    };
    if tail != expected_tail {
        return Err(LogError::Changed);
    }
    cursor.file_len = snapshot.len;
    cursor.mtime = snapshot.modified.clone();
    cursor.file_identity = identity;
    cursor.header_len = header.len() as u64;
    cursor.header_hash = digest(&header);
    cursor.tail_len = tail_len as u64;
    cursor.tail_hash = digest(&tail);
    Ok(FileRead {
        cursor,
        events,
        stamp: snapshot,
        caught_up: examined_to == metadata.len(),
        skipped_lines,
        reset,
    })
}

fn scan_seed(
    file: &mut File,
    mut cursor: FileCursor,
    snapshot: FileStamp,
    identity: String,
    header: &[u8],
    reset: bool,
    budget: &mut ReadBudget,
) -> Result<FileRead, LogError> {
    let mut scan = cursor.seed_scan.take().unwrap_or_default();
    let start = scan.offset + scan.discard_bytes;
    let prefix = read_range(
        file,
        start.saturating_sub(TAIL_BYTES as u64),
        start.min(TAIL_BYTES as u64) as usize,
        budget,
    )?;
    let available = budget.remaining.saturating_sub(header.len() + TAIL_BYTES);
    let length = snapshot.len.saturating_sub(start).min(available as u64) as usize;
    let data = read_range(file, start, length, budget)?;
    let mut position = 0;
    let mut examined_to = start;
    while position < data.len() {
        let Some(newline) = data[position..].iter().position(|byte| *byte == b'\n') else {
            let trailing = data.len() - position;
            if scan.discard_bytes != 0 || trailing > MAX_LINE_BYTES {
                scan.discard_bytes += trailing as u64;
            }
            examined_to = start + data.len() as u64;
            break;
        };
        let end = position + newline + 1;
        let record = if scan.discard_bytes == 0 && newline <= MAX_LINE_BYTES {
            let line = &data[position..end - 1];
            let line = if scan.line_number == 0 {
                line.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(line)
            } else {
                line
            };
            serde_json::from_slice::<Value>(line).ok()
        } else {
            None
        };
        if !scan.header_checked {
            scan.header_checked = true;
            if let Some(record) = record.as_ref().filter(|record| {
                record["type"] == "session"
                    && record["version"].as_u64() == Some(4)
                    && record["isSeeded"].as_bool() == Some(true)
            }) {
                scan.session_id = record["id"].as_str().and_then(super::privacy::identifier);
            }
            if scan.session_id.is_none() {
                scan.complete = true;
            }
        } else if let Some(record) = record.as_ref().filter(|record| {
            record["type"] == "session/end-seed"
                && record["data"]["inherited"].as_bool() == Some(true)
        }) {
            scan.boundary_seq = record["seq"]
                .as_u64()
                .filter(|seq| *seq <= 9_007_199_254_740_991);
        }
        scan.offset = start + end as u64;
        scan.line_number += 1;
        scan.discard_bytes = 0;
        position = end;
        examined_to = scan.offset;
        if scan.complete {
            break;
        }
    }
    let final_metadata = file.metadata().map_err(|_| LogError::Unreadable)?;
    if final_metadata.len() < snapshot.len
        || file_identity(file, &final_metadata)? != identity
        || read_range(file, 0, header.len(), budget)? != header
    {
        return Err(LogError::Changed);
    }
    let tail_end = scan.offset + scan.discard_bytes;
    let tail_len = tail_end.min(TAIL_BYTES as u64) as usize;
    let tail = read_range(file, tail_end - tail_len as u64, tail_len, budget)?;
    let end = (tail_end - start) as usize;
    let data_tail = end.min(tail_len);
    let prefix_length = tail_len - data_tail;
    if prefix_length > prefix.len() {
        return Err(LogError::Changed);
    }
    let mut expected = prefix[prefix.len() - prefix_length..].to_vec();
    expected.extend_from_slice(&data[end - data_tail..end]);
    if tail != expected {
        return Err(LogError::Changed);
    }
    if scan.session_id.is_some()
        && scan.boundary_seq.is_some()
        && scan.offset == snapshot.len
        && final_metadata.len() == snapshot.len
    {
        scan.complete = true;
        cursor.parser_state = serde_json::json!({
            "seedBoundary": { "sessionId": scan.session_id, "seq": scan.boundary_seq }
        });
    }
    scan.tail_len = tail_len as u64;
    scan.tail_hash = digest(&tail);
    let caught_up = !scan.complete && examined_to == snapshot.len;
    cursor.seed_scan = Some(scan);
    cursor.file_len = snapshot.len;
    cursor.file_identity = identity;
    cursor.mtime = snapshot.modified.clone();
    cursor.header_len = header.len() as u64;
    cursor.header_hash = digest(header);
    cursor.tail_len = 0;
    cursor.tail_hash = digest(&[]);
    Ok(FileRead {
        cursor,
        events: Vec::new(),
        stamp: snapshot,
        caught_up,
        skipped_lines: 0,
        reset,
    })
}

#[cfg(test)]
#[path = "collector_tests.rs"]
mod tests;
