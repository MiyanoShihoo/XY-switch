//! Read-only DeepSeek Harness v4 usage import. Schema verified against
//! deepseek-ai/deepseek-harness @ 687f282e8625c6b8d11ff58211a90524be2e4317.
use crate::{database::Database, skills_local::is_link};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_DECODED_BYTES: u64 = 128 * 1024 * 1024;
const SOURCE: &str = "deepseek_harness";
const PROVIDER: &str = "local:deepseek_harness";

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub imported: usize,
    pub scanned: usize,
    pub warnings: Vec<String>,
}
#[derive(Debug)]
struct Entry {
    key: String,
    model: String,
    time: i64,
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
    total: Option<i64>,
    reasoning: Option<i64>,
    success: bool,
}

fn count(value: &Value, key: &str) -> Result<i64, String> {
    value
        .get(key)
        .map(|v| {
            v.as_i64()
                .filter(|v| *v >= 0)
                .ok_or_else(|| format!("无效的 token 计数：{key}"))
        })
        .unwrap_or(Ok(0))
}
fn parse(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    // Trim the in-flight tail before UTF-8 validation: it may end in a multibyte character.
    let end = bytes
        .iter()
        .rposition(|b| *b == b'\n')
        .map(|i| i + 1)
        .ok_or("没有完整的会话头")?;
    let text = std::str::from_utf8(&bytes[..end]).map_err(|_| "会话文件不是 UTF-8")?;
    let mut lines = text.split_inclusive('\n');
    let header: Value =
        serde_json::from_str(lines.next().ok_or("空会话日志")?).map_err(|e| e.to_string())?;
    if header["type"] != "session" || header["version"].as_u64() != Some(4) {
        return Err("只支持 Harness v4 会话格式；不回退读取旧版本，以免重复统计".into());
    }
    let session = header["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("缺少 session id")?;
    let seeded = header["isSeeded"].as_bool().unwrap_or(false);
    let mut entries = Vec::new();
    let mut model = String::new();
    let mut boundary_seen = false;
    let mut expected = 0;
    for line in lines {
        // A running process may have written only part of its last JSONL record.
        if !line.ends_with('\n') {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        let event: Value =
            serde_json::from_str(line).map_err(|e| format!("损坏的会话事件：{e}"))?;
        let seq = event["seq"].as_u64().ok_or("事件缺少 seq")?;
        if seq != expected {
            return Err(format!("事件序号不连续：预期 {expected}，得到 {seq}"));
        }
        expected += 1;
        let data = &event["data"];
        match event["type"].as_str().unwrap_or("") {
            "request/header" => {
                if let Some(m) = data.pointer("/header/config/model").and_then(Value::as_str) {
                    model = m.to_string();
                }
            }
            "request/context" => {
                if let Some(m) = data["model"].as_str() {
                    model = m.to_string();
                }
            }
            "session/end-seed" if data["inherited"] == true => {
                entries.clear();
                boundary_seen = true;
            }
            "assistant/message" | "assistant/attempt" => {
                let usage = data.get("usage").filter(|v| v.is_object()).or_else(|| {
                    data["stream"].as_array()?.iter().rev().find_map(|record| {
                        if record["type"] == "chunk" && record["chunk"]["type"] == "usage" {
                            record["chunk"].get("usage")
                        } else {
                            None
                        }
                    })
                });
                let Some(usage) = usage else {
                    continue;
                };
                if !usage["inputTokens"].is_number() || !usage["outputTokens"].is_number() {
                    return Err("usage 缺少输入或输出 token 计数".into());
                }
                let cache_read = count(usage, "cacheReadTokens")?;
                let cache_write = count(usage, "cacheWriteTokens")?;
                // Harness uses disjoint counts; request_events uses aggregate prompt input.
                let input = count(usage, "inputTokens")?
                    .checked_add(cache_read)
                    .and_then(|n| n.checked_add(cache_write))
                    .ok_or("token 计数溢出")?;
                let output = count(usage, "outputTokens")?;
                let total = usage
                    .get("totalTokens")
                    .map(|_| count(usage, "totalTokens"))
                    .transpose()?;
                let reasoning = usage
                    .get("reasoningTokens")
                    .map(|_| count(usage, "reasoningTokens"))
                    .transpose()?;
                let time = event["time"]
                    .as_i64()
                    .filter(|t| *t >= 0)
                    .ok_or("事件缺少有效时间")?;
                let key = format!(
                    "{:x}",
                    Sha256::digest(format!("{session}:{seq}").as_bytes())
                );
                entries.push(Entry {
                    key,
                    model: if model.is_empty() {
                        "unknown".into()
                    } else {
                        model.clone()
                    },
                    time,
                    input,
                    output,
                    cache_read,
                    cache_write,
                    total,
                    reasoning,
                    success: event["type"] == "assistant/message" && data["interrupted"] != true,
                });
            }
            _ => {}
        }
    }
    if seeded && !boundary_seen {
        return Err("继承会话缺少 end-seed 边界，未计入以防重复".into());
    }
    if !seeded && boundary_seen {
        return Err("非继承会话包含继承边界，未计入".into());
    }
    Ok(entries)
}

fn generation(name: &str) -> Option<(u32, bool)> {
    let (raw, compressed) = if let Some(raw) = name
        .strip_suffix(".zstd")
        .or_else(|| name.strip_suffix(".zst"))
    {
        (raw, true)
    } else {
        (name, false)
    };
    if raw == "session.jsonl" {
        return Some((0, compressed));
    }
    let version = raw.strip_prefix("session.v")?.strip_suffix(".jsonl")?;
    if version.starts_with('0') {
        return None;
    }
    Some((version.parse().ok()?, compressed))
}
fn collect(root: &Path, result: &mut SyncResult) -> Vec<PathBuf> {
    let mut selected = BTreeMap::<PathBuf, (u32, bool, PathBuf)>::new();
    let mut stack = vec![(root.to_path_buf(), 0)];
    let mut dirs = 0;
    while let Some((dir, depth)) = stack.pop() {
        dirs += 1;
        if dirs > 20000 {
            result
                .warnings
                .push("Harness 会话扫描达到 20000 目录上限".into());
            break;
        }
        let meta = match fs::symlink_metadata(&dir) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                result.warnings.push(e.to_string());
                continue;
            }
        };
        if is_link(&meta) || !meta.is_dir() {
            continue;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                result.warnings.push(e.to_string());
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if is_link(&meta) {
                continue;
            }
            if meta.is_dir() && depth < 5 {
                stack.push((path, depth + 1));
            } else if meta.is_file() {
                if let Some((version, compressed)) =
                    generation(&entry.file_name().to_string_lossy())
                {
                    let candidate = (version, compressed, path);
                    let old = selected
                        .entry(dir.clone())
                        .or_insert_with(|| candidate.clone());
                    if (candidate.0, candidate.1) > (old.0, old.1) {
                        *old = candidate;
                    }
                }
            }
        }
    }
    selected.into_values().map(|(_, _, p)| p).collect()
}
fn read_log(path: &Path) -> Result<Vec<u8>, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_DECODED_BYTES {
        return Err("会话文件超过 128 MiB 限制".into());
    }
    let compressed = path.extension().is_some_and(|e| e == "zstd" || e == "zst");
    let reader: Box<dyn Read> = if compressed {
        Box::new(
            zstd::stream::read::Decoder::new(file)
                .map_err(|e| format!("Zstandard 解压失败：{e}"))?,
        )
    } else {
        Box::new(file)
    };
    let mut bytes = Vec::new();
    reader
        .take(MAX_DECODED_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_DECODED_BYTES {
        return Err("解压后的会话超过 128 MiB 限制".into());
    }
    Ok(bytes)
}

pub fn sync_root(database: &Database, root: &Path) -> Result<SyncResult, String> {
    let mut result = SyncResult::default();
    let files = collect(root, &mut result);
    let arc = database.conn();
    {
        let conn = arc.lock().map_err(|e| e.to_string())?;
        conn.execute_batch("CREATE TABLE IF NOT EXISTS local_usage_imports(source TEXT NOT NULL,event_key TEXT NOT NULL,event_id INTEGER NOT NULL REFERENCES request_events(id) ON DELETE CASCADE,PRIMARY KEY(source,event_key)); CREATE TABLE IF NOT EXISTS local_usage_files(source TEXT NOT NULL,path TEXT NOT NULL,signature TEXT NOT NULL,PRIMARY KEY(source,path));").map_err(|e|e.to_string())?;
    }
    for path in files {
        result.scanned += 1;
        let imported = (|| -> Result<usize, String> {
            let meta = fs::metadata(&path).map_err(|e| e.to_string())?;
            let signature = format!(
                "{}:{}",
                meta.len(),
                meta.modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            );
            let path_text = path.to_string_lossy().into_owned();
            {
                let conn = arc.lock().map_err(|e| e.to_string())?;
                let previous: Option<String> = conn
                    .query_row(
                        "SELECT signature FROM local_usage_files WHERE source=?1 AND path=?2",
                        params![SOURCE, path_text],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(|e| e.to_string())?;
                if previous.as_deref() == Some(signature.as_str()) {
                    return Ok(0);
                }
            }
            let entries = parse(&read_log(&path)?)?;
            let mut conn = arc.lock().map_err(|e| e.to_string())?;
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            if !entries.is_empty() {
                ensure_source(&tx, PROVIDER, "DeepSeek Harness · 本地用量")?;
            }
            let mut imported = 0;
            for e in entries {
                let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM local_usage_imports WHERE source=?1 AND event_key=?2)",params![SOURCE,e.key],|r|r.get(0)).map_err(|e|e.to_string())?;
                if exists {
                    continue;
                }
                tx.execute("INSERT INTO request_events(provider_id,route,status_code,success,latency_ms,retry_count,error_kind,model,input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,total_tokens,reasoning_tokens,cost,price_source,price_version,created_at) VALUES(?1,'session/deepseek_harness',?2,?3,0,0,?4,?5,?6,?7,?8,?9,?10,?11,NULL,'harness_session',?12,?13)",params![PROVIDER,if e.success{200}else{0},e.success as i64,if e.success{None}else{Some("harness_interrupted_or_failed")},e.model,e.input,e.output,e.cache_read,e.cache_write,e.total,e.reasoning,e.key,e.time]).map_err(|e|e.to_string())?;
                tx.execute(
                    "INSERT INTO local_usage_imports(source,event_key,event_id) VALUES(?1,?2,?3)",
                    params![SOURCE, e.key, tx.last_insert_rowid()],
                )
                .map_err(|e| e.to_string())?;
                imported += 1;
            }
            tx.execute("INSERT INTO local_usage_files(source,path,signature) VALUES(?1,?2,?3) ON CONFLICT(source,path) DO UPDATE SET signature=excluded.signature",params![SOURCE,path_text,signature]).map_err(|e|e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            Ok(imported)
        })();
        match imported {
            Ok(n) => result.imported += n,
            Err(e) => result.warnings.push(format!("{}：{e}", path.display())),
        }
    }
    result.warnings.truncate(20);
    Ok(result)
}
pub(crate) fn ensure_source(
    conn: &rusqlite::Connection,
    id: &str,
    name: &str,
) -> Result<(), String> {
    conn.execute("INSERT OR IGNORE INTO providers(id,name,provider_type,endpoint,model,app_group,enabled,sort_order) VALUES(?1,?2,'local_usage','local://sessions','','Codex',0,999999)",params![id,name]).map(|_|()).map_err(|e|e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn log(id: &str, seeded: bool, events: Vec<Value>) -> Vec<u8> {
        let mut text=json!({"type":"session","version":4,"id":id,"createdAt":0,"isSeeded":seeded,"delegationDepth":0}).to_string()+"\n";
        for (i, mut event) in events.into_iter().enumerate() {
            event["seq"] = json!(i);
            event["time"] = json!(1000 + i * 10);
            text.push_str(&event.to_string());
            text.push('\n');
        }
        text.into_bytes()
    }
    fn usage() -> Value {
        json!({"type":"assistant/message","data":{"usage":{"inputTokens":168,"outputTokens":25,"cacheReadTokens":2816,"cacheWriteTokens":10,"totalTokens":3019}}})
    }
    #[test]
    fn token_semantics_model_switch_and_fork_boundary() {
        let rows = parse(&log(
            "test",
            true,
            vec![
                usage(),
                json!({"type":"session/end-seed","data":{"inherited":true}}),
                json!({"type":"request/context","data":{"model":"deepseek-flash"}}),
                usage(),
            ],
        ))
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].input, 2994);
        assert_eq!(rows[0].cache_read, 2816);
        assert_eq!(rows[0].model, "deepseek-flash");
        assert!(parse(&log("bad", true, vec![usage()])).is_err());
    }
    #[test]
    fn compressed_plain_duplicate_files_and_restarts_are_idempotent() {
        let root = std::env::temp_dir().join(format!("xy-harness-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("sessions/p/a")).unwrap();
        fs::create_dir_all(root.join("sessions/p/copy")).unwrap();
        let bytes = log(
            "same-session",
            false,
            vec![
                json!({"type":"request/header","data":{"header":{"config":{"model":"ds"}}}}),
                usage(),
            ],
        );
        fs::write(
            root.join("sessions/p/a/session.v4.jsonl.zstd"),
            zstd::stream::encode_all(bytes.as_slice(), 1).unwrap(),
        )
        .unwrap();
        fs::write(root.join("sessions/p/copy/session.v4.jsonl"), &bytes).unwrap();
        fs::write(root.join("sessions/p/a/session.v3.jsonl"), "not read").unwrap();
        let db = Database::open(&root.join("test.db")).unwrap();
        assert_eq!(sync_root(&db, &root.join("sessions")).unwrap().imported, 1);
        assert_eq!(sync_root(&db, &root.join("sessions")).unwrap().imported, 0);
        assert_eq!(db.list_providers().unwrap().len(), 4);
        let conn = db.conn();
        assert_eq!(
            conn.lock()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM request_events WHERE cost IS NULL",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        drop(db);
        drop(conn);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn failed_attempt_usage_and_partial_tail() {
        let mut bytes = log(
            "attempt",
            false,
            vec![
                json!({"type":"assistant/attempt","data":{"stream":[{"type":"chunk","chunk":{"type":"usage","usage":{"inputTokens":10,"outputTokens":3}}}]}}),
            ],
        );
        bytes.extend_from_slice(b"{\"type\":");
        let rows = parse(&bytes).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].success);
        assert_eq!(rows[0].output, 3);
    }
    // Usage-only projection of the upstream snapshot; synthetic framing restores seq/time
    // omitted by the snapshot writer. No conversation or credentials are retained.
    #[test]
    fn reads_official_v4_usage_snapshot_without_cache_double_counting() {
        let rows = parse(include_bytes!("../tests/fixtures/harness-v4-usage.jsonl")).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].model, "deepseek-v4-flash");
        assert_eq!(rows[0].input, 2984);
        assert_eq!(rows[0].cache_read, 2816);
        assert_eq!(rows[0].cache_write, 0);
        assert_eq!(rows[0].output, 25);
        assert_eq!(rows[0].total, Some(3009));
    }
    #[test]
    fn only_the_last_seed_boundary_counts_and_partial_utf8_is_ignored() {
        let mut bytes = log(
            "fork",
            true,
            vec![
                usage(),
                json!({"type":"session/end-seed","data":{"inherited":true}}),
                usage(),
                json!({"type":"session/end-seed","data":{"inherited":true}}),
                usage(),
            ],
        );
        bytes.extend_from_slice(&[b'{', b'"', 0xf0, 0x9f]);
        assert_eq!(parse(&bytes).unwrap().len(), 1);
    }
    #[test]
    fn malformed_sequence_and_unknown_versions_are_not_silently_counted() {
        let text = String::from_utf8(log("corrupt", false, vec![usage()])).unwrap();
        assert!(parse(text.replace("\"seq\":0", "\"seq\":3").as_bytes()).is_err());
        assert!(parse(text.replace("\"version\":4", "\"version\":5").as_bytes()).is_err());
    }
}
