mod parsers;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, Metadata},
    io::Read,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_FILES: usize = 10_000;
const MAX_DIRECTORIES: usize = 10_000;
const MAX_HEADER_BYTES: usize = 256 * 1024;
const MAX_FILE_BYTES: usize = 32 * 1024 * 1024;
const MAX_SCAN_BYTES: usize = 64 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;
const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_MESSAGES: usize = 5_000;
const MAX_WARNINGS: usize = 30;
const MAX_DELETE_BATCH: usize = 100;
const MAX_INDEX_BYTES: usize = 4 * 1024 * 1024;
const MAX_INDEX_RECORDS: usize = 20_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub provider_id: String,
    pub session_id: String,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub project_dir: Option<String>,
    pub created_at: Option<i64>,
    pub last_active_at: Option<i64>,
    pub source_path: Option<String>,
    pub resume_command: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
    pub ts: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionIdPair {
    pub provider_id: String,
    pub session_id: String,
}

#[derive(Default, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCatalog {
    pub sessions: Vec<SessionMeta>,
    pub warnings: Vec<String>,
}

#[derive(Default, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    pub messages: Vec<SessionMessage>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Client {
    Codex,
    Claude,
    Gemini,
}

impl Client {
    fn id(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Gemini => "gemini",
        }
    }
    fn from_id(id: &str) -> Result<Self, String> {
        match id {
            "codex" => Ok(Self::Codex),
            "claude" => Ok(Self::Claude),
            "gemini" => Ok(Self::Gemini),
            _ => Err("不支持的会话客户端；当前支持 Codex、Claude Code 和 Gemini CLI".into()),
        }
    }
    fn candidate(self, path: &Path) -> bool {
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        match self {
            Self::Codex => name.starts_with("rollout-") && name.ends_with(".jsonl"),
            Self::Claude => {
                path.extension().is_some_and(|s| s == "jsonl")
                    && path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .is_some_and(valid_id)
            }
            Self::Gemini => {
                name.starts_with("session-")
                    && name.ends_with(".json")
                    && path
                        .parent()
                        .and_then(Path::file_name)
                        .is_some_and(|s| s == "chats")
            }
        }
    }
}

struct SourceRoot {
    client: Client,
    path: PathBuf,
    depth: usize,
}

/// Discovery is restricted to transcripts and Codex's title-only index.
/// Configuration, credentials and client databases are never read or deleted.
pub struct SessionStore {
    roots: Vec<SourceRoot>,
    codex_home: PathBuf,
}

impl SessionStore {
    pub fn new(home: &Path, codex_home: &Path) -> Result<Self, String> {
        Self::with_overrides(home, codex_home, None, None)
    }

    pub fn with_overrides(
        home: &Path,
        codex_home: &Path,
        claude_config_dir: Option<&Path>,
        gemini_cli_home: Option<&Path>,
    ) -> Result<Self, String> {
        // Check raw overrides before PathBuf::join can normalize a Windows verbatim path.
        for directory in [Some(home), Some(codex_home), claude_config_dir, gemini_cli_home].into_iter().flatten() {
            if !directory.is_absolute() || directory.components().any(|part| matches!(part, std::path::Component::ParentDir)) {
                return Err("会话目录必须是不含上级跳转的绝对路径".into());
            }
        }
        let claude_home = claude_config_dir.map(Path::to_path_buf).unwrap_or_else(|| home.join(".claude"));
        // GEMINI_CLI_HOME overrides the user's home, not the .gemini directory.
        let gemini_home = gemini_cli_home.unwrap_or(home).join(".gemini");
        Ok(Self {
            codex_home: codex_home.to_path_buf(),
            roots: vec![
                SourceRoot { client: Client::Codex, path: codex_home.join("sessions"), depth: 5 },
                SourceRoot { client: Client::Codex, path: codex_home.join("archived_sessions"), depth: 3 },
                SourceRoot { client: Client::Claude, path: claude_home.join("projects"), depth: 1 },
                SourceRoot { client: Client::Gemini, path: gemini_home.join("tmp"), depth: 2 },
            ],
        })
    }

    pub fn catalog(&self) -> Result<SessionCatalog, String> {
        let (entries, mut warnings) = self.scan()?;
        let mut unique = BTreeMap::new();
        for entry in entries {
            let key = (
                entry.meta.provider_id.clone(),
                entry.meta.session_id.clone(),
            );
            if unique.contains_key(&key) {
                warn(
                    &mut warnings,
                    format!(
                        "{} 会话 {} 存在多个源文件，读取和删除将拒绝歧义记录",
                        key.0, key.1
                    ),
                );
            } else {
                unique.insert(key, entry.meta);
            }
        }
        let mut sessions: Vec<_> = unique.into_values().collect();
        let titles = self.codex_titles(&mut warnings);
        for session in &mut sessions {
            if session.provider_id == "codex" {
                if let Some((_, title)) = titles.get(&session.session_id) {
                    session.title = Some(title.clone());
                }
            }
        }
        sessions.sort_by(|a, b| {
            b.last_active_at
                .cmp(&a.last_active_at)
                .then_with(|| a.provider_id.cmp(&b.provider_id))
                .then_with(|| a.session_id.cmp(&b.session_id))
        });
        Ok(SessionCatalog { sessions, warnings })
    }

    fn codex_titles(&self, warnings: &mut Vec<String>) -> BTreeMap<String, (Option<i64>, String)> {
        let mut titles = BTreeMap::new();
        let index_path = self.codex_home.join("session_index.jsonl");
        let read = || -> Result<Vec<u8>, String> {
            let meta = match fs::symlink_metadata(&index_path) {
                Ok(meta) => meta,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
                Err(error) => return Err(error.to_string()),
            };
            let entry = Snapshot {
                root: self.codex_home.clone(), path: index_path.clone(), client: Client::Codex,
                len: meta.len(), modified: meta.modified().ok(),
                meta: SessionMeta { provider_id: "codex".into(), session_id: String::new(), title: None, summary: None,
                    project_dir: None, created_at: None, last_active_at: None, source_path: None, resume_command: None },
            };
            // Normalize the explicit root only after rejecting a reparse point.
            let root_meta = fs::symlink_metadata(&entry.root).map_err(|e| e.to_string())?;
            if linked(&root_meta) || !root_meta.is_dir() { return Err("标题索引根目录不是普通目录".into()); }
            let root = entry.root.canonicalize().map_err(|e| e.to_string())?;
            let entry = Snapshot { path: root.join("session_index.jsonl"), root, ..entry };
            read_checked(&entry, MAX_INDEX_BYTES)
        };
        let bytes = match read() {
            Ok(bytes) => bytes,
            Err(error) => { warn(warnings, format!("Codex 标题索引不可用，仍显示日志标题：{error}")); return titles; }
        };
        for (line_number, line) in bytes.split(|b| *b == b'\n').enumerate() {
            if line_number >= MAX_INDEX_RECORDS { warn(warnings, "Codex 标题索引超过记录上限，部分重命名标题可能未加载".into()); break; }
            if line.iter().all(u8::is_ascii_whitespace) { continue; }
            let value: serde_json::Value = match serde_json::from_slice(line) {
                Ok(value) => value,
                Err(_) => { warn(warnings, "Codex 标题索引含无效记录，已忽略；会话日志不受影响".into()); continue; }
            };
            let Some(id) = value.get("id").and_then(|v| v.as_str()).filter(|id| valid_id(id)) else { continue; };
            let Some(title) = value.get("thread_name").and_then(|v| v.as_str()).map(str::trim)
                .filter(|title| !title.is_empty() && title.len() <= 1024 && !title.chars().any(char::is_control)) else { continue; };
            let updated = value.get("updated_at").and_then(|v| v.as_str()).and_then(crate::session_usage_codex::parse_iso_timestamp_ms);
            // Newer timestamps win; equal/missing timestamps use append order.
            if titles.get(id).is_none_or(|(previous, _)| updated >= *previous) {
                titles.insert(id.to_owned(), (updated, title.to_owned()));
            }
        }
        titles
    }

    pub fn detail(&self, provider_id: &str, session_id: &str) -> Result<SessionDetail, String> {
        validate_pair(provider_id, session_id)?;
        let (entries, _) = self.scan()?;
        let entry = find_entry(&entries, provider_id, session_id)?;
        let bytes = read_checked(entry, MAX_FILE_BYTES)?;
        let parsed = parsers::parse(entry.client, &entry.path, &bytes, false, true)?;
        if parsed.id.as_deref() != Some(session_id) {
            return Err("会话源文件已改变，请刷新后重试".into());
        }
        Ok(parsed.detail)
    }

    pub fn delete(&self, ids: &[SessionIdPair]) -> Result<usize, String> {
        if ids.is_empty() || ids.len() > MAX_DELETE_BATCH {
            return Err(format!("每次需要指定 1–{MAX_DELETE_BATCH} 个会话"));
        }
        let mut unique = BTreeSet::new();
        for id in ids {
            validate_pair(&id.provider_id, &id.session_id)?;
            unique.insert((id.provider_id.as_str(), id.session_id.as_str()));
        }
        let (entries, warnings) = self.scan()?;
        if !warnings.is_empty() {
            return Err("会话扫描存在错误或不完整，为避免误删，请先处理列表中的警告".into());
        }
        // Resolve and validate the whole batch before changing any source file.
        let targets = unique
            .into_iter()
            .map(|(provider, id)| find_entry(&entries, provider, id))
            .collect::<Result<Vec<_>, _>>()?;
        for entry in &targets {
            let bytes = read_checked(entry, MAX_FILE_BYTES)?;
            let parsed = parsers::parse(entry.client, &entry.path, &bytes, false, false)?;
            if parsed.id.as_deref() != Some(entry.meta.session_id.as_str()) {
                return Err("会话源文件 ID 已改变，未删除任何日志".into());
            }
            if !parsed.detail.warnings.is_empty() {
                return Err("会话日志不完整或已损坏，未删除任何日志".into());
            }
        }
        for (removed, entry) in targets.iter().enumerate() {
            verify_snapshot(entry)
                .and_then(|()| fs::remove_file(&entry.path).map_err(|e| e.to_string()))
                .map_err(|e| {
                    format!(
                        "已删除 {removed}/{} 个会话日志；其余记录保留：{e}",
                        targets.len()
                    )
                })?;
        }
        Ok(targets.len())
    }

    fn scan(&self) -> Result<(Vec<Snapshot>, Vec<String>), String> {
        let mut warnings = Vec::new();
        let mut entries = Vec::new();
        let mut remaining = MAX_SCAN_BYTES;
        let mut files_seen = 0;
        let mut directories_seen = 0;
        let mut items_seen = 0;
        for source in &self.roots {
            let root_meta = match fs::symlink_metadata(&source.path) {
                Ok(m) => m,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    warn(
                        &mut warnings,
                        format!("无法读取 {} 会话目录：{e}", source.client.id()),
                    );
                    continue;
                }
            };
            if linked(&root_meta) || !root_meta.is_dir() {
                warn(
                    &mut warnings,
                    format!("跳过非普通目录或链接：{}", source.path.display()),
                );
                continue;
            }
            let root = match source.path.canonicalize() {
                Ok(p) => p,
                Err(e) => {
                    warn(&mut warnings, format!("无法解析会话目录：{e}"));
                    continue;
                }
            };
            let mut pending = vec![(root.clone(), 0)];
            while let Some((directory, depth)) = pending.pop() {
                directories_seen += 1;
                if files_seen >= MAX_FILES || directories_seen > MAX_DIRECTORIES || remaining == 0 {
                    warn(
                        &mut warnings,
                        "会话扫描已达到安全上限，列表可能不完整".into(),
                    );
                    return Ok((entries, warnings));
                }
                if safe_path(&root, &directory, false).is_err() {
                    warn(
                        &mut warnings,
                        format!("跳过不安全的会话目录：{}", directory.display()),
                    );
                    continue;
                }
                let children = match fs::read_dir(&directory) {
                    Ok(children) => children,
                    Err(e) => {
                        warn(&mut warnings, format!("无法枚举会话目录：{e}"));
                        continue;
                    }
                };
                for child in children {
                    items_seen += 1;
                    if items_seen > MAX_FILES * 10 {
                        warn(
                            &mut warnings,
                            "会话扫描已达到目录项安全上限，列表可能不完整".into(),
                        );
                        return Ok((entries, warnings));
                    }
                    let child = match child {
                        Ok(c) => c,
                        Err(e) => {
                            warn(&mut warnings, format!("无法读取会话目录项：{e}"));
                            continue;
                        }
                    };
                    let path = child.path();
                    let meta = match fs::symlink_metadata(&path) {
                        Ok(m) => m,
                        Err(e) => {
                            warn(&mut warnings, format!("无法读取会话文件属性：{e}"));
                            continue;
                        }
                    };
                    if linked(&meta) {
                        continue;
                    }
                    if meta.is_dir() {
                        if depth < source.depth && pending.len() < MAX_DIRECTORIES {
                            pending.push((path, depth + 1));
                        }
                        continue;
                    }
                    if !meta.is_file() || !source.client.candidate(&path) {
                        continue;
                    }
                    files_seen += 1;
                    if files_seen > MAX_FILES || remaining == 0 {
                        warn(
                            &mut warnings,
                            "会话扫描已达到安全上限，列表可能不完整".into(),
                        );
                        return Ok((entries, warnings));
                    }
                    let limit = if source.client == Client::Gemini {
                        MAX_FILE_BYTES
                    } else {
                        MAX_HEADER_BYTES
                    };
                    let limit = limit.min(remaining);
                    let mut entry = Snapshot {
                        root: root.clone(),
                        path: path.clone(),
                        client: source.client,
                        len: meta.len(),
                        modified: meta.modified().ok(),
                        meta: SessionMeta {
                            provider_id: source.client.id().into(),
                            session_id: String::new(),
                            title: None,
                            summary: None,
                            project_dir: None,
                            created_at: None,
                            last_active_at: None,
                            source_path: Some(path.to_string_lossy().into_owned()),
                            resume_command: None,
                        },
                    };
                    let bytes = match read_prefix(&entry, limit, false) {
                        Ok(b) => b,
                        Err(e) => {
                            warn(&mut warnings, format!("无法读取 {}：{e}", path.display()));
                            continue;
                        }
                    };
                    remaining = remaining.saturating_sub(bytes.len());
                    let truncated = meta.len() > bytes.len() as u64;
                    let parsed =
                        match parsers::parse(source.client, &path, &bytes, truncated, false) {
                            Ok(p) => p,
                            Err(e) => {
                                warn(&mut warnings, format!("跳过 {}：{e}", path.display()));
                                continue;
                            }
                        };
                    let Some(id) = parsed.id.filter(|id| valid_id(id)) else {
                        warn(
                            &mut warnings,
                            format!("跳过缺少有效会话 ID 的文件：{}", path.display()),
                        );
                        continue;
                    };
                    if source.client == Client::Claude
                        && path.file_stem().and_then(|s| s.to_str()) != Some(id.as_str())
                    {
                        warn(
                            &mut warnings,
                            format!("跳过 ID 与文件名不一致的会话：{}", path.display()),
                        );
                        continue;
                    }
                    if source.client == Client::Codex
                        && !path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .is_some_and(|name| name.ends_with(&format!("-{id}.jsonl")))
                    {
                        warn(
                            &mut warnings,
                            format!("跳过 ID 与文件名不一致的会话：{}", path.display()),
                        );
                        continue;
                    }
                    entry.meta.session_id = id.clone();
                    entry.meta.title = parsed.title;
                    entry.meta.project_dir = parsed.project_dir;
                    entry.meta.created_at = parsed.created_at;
                    entry.meta.last_active_at = parsed
                        .last_active_at
                        .or_else(|| modified_ms(entry.modified));
                    entry.meta.resume_command = Some(match source.client {
                        Client::Codex => format!("codex resume {id}"),
                        Client::Claude => format!("claude --resume {id}"),
                        Client::Gemini => format!("gemini --resume {id}"),
                    });
                    entries.push(entry);
                }
            }
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok((entries, warnings))
    }
}

struct Snapshot {
    root: PathBuf,
    path: PathBuf,
    client: Client,
    len: u64,
    modified: Option<SystemTime>,
    meta: SessionMeta,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

fn validate_pair(provider_id: &str, session_id: &str) -> Result<(), String> {
    Client::from_id(provider_id)?;
    if !valid_id(session_id) {
        return Err("无效的会话 ID".into());
    }
    Ok(())
}

fn find_entry<'a>(
    entries: &'a [Snapshot],
    provider_id: &str,
    session_id: &str,
) -> Result<&'a Snapshot, String> {
    let mut matches = entries
        .iter()
        .filter(|e| e.meta.provider_id == provider_id && e.meta.session_id == session_id);
    let first = matches
        .next()
        .ok_or_else(|| "找不到会话日志，请刷新列表；文件可能已被客户端移走".to_string())?;
    if matches.next().is_some() {
        return Err("会话 ID 对应多个源文件，已拒绝读取或删除歧义记录".into());
    }
    Ok(first)
}

fn linked(meta: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

fn safe_path(root: &Path, path: &Path, file: bool) -> Result<Metadata, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "会话路径超出允许目录")?;
    let mut current = root.to_path_buf();
    let mut meta = fs::symlink_metadata(&current).map_err(|e| e.to_string())?;
    if linked(&meta) || !meta.is_dir() {
        return Err("会话根目录已被替换".into());
    }
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err("不安全的会话路径".into());
        }
        current.push(component);
        meta = fs::symlink_metadata(&current).map_err(|e| e.to_string())?;
        if linked(&meta) {
            return Err("拒绝访问会话符号链接或重解析点".into());
        }
    }
    if (file && !meta.is_file())
        || (!file && !meta.is_dir())
        || !current
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(root)
    {
        return Err("会话路径不是允许目录内的普通文件或目录".into());
    }
    Ok(meta)
}

fn verify_snapshot(entry: &Snapshot) -> Result<(), String> {
    let meta = safe_path(&entry.root, &entry.path, true)?;
    if meta.len() != entry.len || meta.modified().ok() != entry.modified {
        return Err("会话正在更新或已被替换，请停止该会话并刷新后重试".into());
    }
    Ok(())
}

fn read_prefix(entry: &Snapshot, limit: usize, stable: bool) -> Result<Vec<u8>, String> {
    safe_path(&entry.root, &entry.path, true)?;
    if stable {
        verify_snapshot(entry)?;
    }
    let file = File::open(&entry.path).map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if linked(&meta)
        || !meta.is_file()
        || (stable && (meta.len() != entry.len || meta.modified().ok() != entry.modified))
    {
        return Err("会话源文件在读取前发生变化".into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    safe_path(&entry.root, &entry.path, true)?;
    if stable {
        verify_snapshot(entry)?;
    }
    Ok(bytes)
}

fn read_checked(entry: &Snapshot, limit: usize) -> Result<Vec<u8>, String> {
    if entry.len > limit as u64 {
        return Err(format!(
            "会话日志超过 {} MiB 的读取上限，请使用原客户端查看",
            limit / 1024 / 1024
        ));
    }
    read_prefix(entry, limit, true)
}

fn modified_ms(time: Option<SystemTime>) -> Option<i64> {
    time?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis()
        .try_into()
        .ok()
}

fn warn(warnings: &mut Vec<String>, message: String) {
    if warnings.len() < MAX_WARNINGS && !warnings.contains(&message) {
        warnings.push(message);
    }
}
