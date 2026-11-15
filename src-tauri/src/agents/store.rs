use super::models::{AgentEvent, AgentEventPage, AgentEventQuery, AgentHistory};
use crate::database::Database;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path};

pub const MAX_STORED_EVENTS: usize = 200_000;
const DAY_MS: i64 = 86_400_000;
const DEFAULT_RETENTION_DAYS: u32 = 30;
const MAX_PAGE_SIZE: usize = 500;
const MAX_STATE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct FileCursor {
    pub source_id: String,
    pub root_key: String,
    pub path: String,
    pub offset: u64,
    pub line_number: u64,
    pub file_len: u64,
    pub mtime: Option<String>,
    pub file_identity: String,
    pub header_len: u64,
    pub header_hash: String,
    pub tail_len: u64,
    pub tail_hash: String,
    pub discard_bytes: u64,
    pub parser_state: Value,
    pub last_seen_at: Option<i64>,
    pub last_event_at: Option<i64>,
    pub malformed_lines: u64,
    pub seed_scan: Option<SeedScanCursor>,
    pub compressed: Option<CompressedCursor>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct CompressedCursor {
    pub verified_offset: u64,
    pub verified_decoded: u64,
    pub inspected_offset: u64,
    pub replay_offset: u64,
    pub replay_decoded: u64,
    pub scan_offset: u64,
    pub scan_decoded: u64,
    pub incomplete: bool,
    pub failed: bool,
    pub waiting_for_change: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct SeedScanCursor {
    pub offset: u64,
    pub line_number: u64,
    pub discard_bytes: u64,
    pub session_id: Option<String>,
    pub boundary_seq: Option<u64>,
    pub header_checked: bool,
    pub complete: bool,
    pub tail_len: u64,
    pub tail_hash: String,
}

#[derive(Clone)]
pub struct EventStore {
    database: Database,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PageCursor {
    version: u8,
    timestamp: i64,
    id: String,
    filter: String,
}

struct EventRef {
    id: String,
    agent_id: String,
    parent_agent_id: Option<String>,
    related_agent_id: Option<String>,
}

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn relative_path_is_safe(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && !path.contains(['\\', ':', '\0'])
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn cursor_json(cursor: &FileCursor) -> Result<String, String> {
    let decoded_len = cursor
        .compressed
        .as_ref()
        .map_or(cursor.file_len, |state| state.verified_decoded);
    let tail_offset = cursor
        .compressed
        .as_ref()
        .map_or(cursor.offset, |state| state.inspected_offset);
    if cursor.source_id.is_empty()
        || cursor.source_id.len() > 240
        || cursor.root_key.len() != 64
        || !cursor.root_key.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !relative_path_is_safe(&cursor.path)
        || cursor.offset > decoded_len
        || cursor.discard_bytes > decoded_len.saturating_sub(cursor.offset)
        || cursor.header_len > cursor.file_len.min(1024)
        || cursor.tail_len > tail_offset.min(128)
        || cursor.file_len > i64::MAX as u64
        || cursor.line_number > i64::MAX as u64
        || cursor.file_identity.len() > 256
        || cursor.compressed.as_ref().is_some_and(|state| {
            state.verified_offset > cursor.file_len
                || state.verified_decoded > i64::MAX as u64
                || state.inspected_offset > cursor.file_len
                || state.inspected_offset < state.verified_offset
                || state.replay_offset > state.verified_offset
                || state.replay_decoded > cursor.offset.saturating_add(cursor.discard_bytes)
                || state.scan_offset > state.verified_offset
                || state.scan_decoded
                    > cursor
                        .seed_scan
                        .as_ref()
                        .map_or(0, |scan| scan.offset.saturating_add(scan.discard_bytes))
                || state.replay_decoded > state.verified_decoded
                || state.scan_decoded > state.verified_decoded
                || (state.replay_offset == 0 && state.replay_decoded != 0)
                || (state.scan_offset == 0 && state.scan_decoded != 0)
        })
        || cursor.seed_scan.as_ref().is_some_and(|scan| {
            scan.offset > decoded_len
                || scan.discard_bytes > decoded_len.saturating_sub(scan.offset)
                || scan.tail_len > scan.offset.saturating_add(scan.discard_bytes).min(128)
                || scan
                    .boundary_seq
                    .is_some_and(|seq| seq > 9_007_199_254_740_991)
                || scan.session_id.as_ref().is_some_and(|id| {
                    id.is_empty()
                        || id.len() > 240
                        || !id
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"-_:./".contains(&byte))
                        || id.starts_with("sk-")
                        || id.starts_with("eyJ")
                })
                || (!scan.tail_hash.is_empty()
                    && (scan.tail_hash.len() != 64
                        || !scan.tail_hash.bytes().all(|byte| byte.is_ascii_hexdigit())))
        })
        || [&cursor.header_hash, &cursor.tail_hash].iter().any(|hash| {
            !hash.is_empty()
                && (hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        })
    {
        return Err("Agent 文件游标无效".into());
    }
    let state = serde_json::to_vec(&cursor.parser_state).map_err(|_| "无法编码 Agent 解析状态")?;
    if state.len() > MAX_STATE_BYTES {
        return Err("Agent 解析状态超过存储上限".into());
    }
    serde_json::to_string(cursor).map_err(|_| "无法编码 Agent 文件游标".into())
}

fn decode_event(text: String) -> Result<AgentEvent, String> {
    serde_json::from_str(&text).map_err(|_| "Agent 观测记录损坏".into())
}

fn retention_on(connection: &Connection) -> Result<u32, String> {
    let stored: Option<String> = connection
        .query_row(
            "SELECT value FROM agent_observation_settings WHERE key='retention_days'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| "无法读取 Agent 保留设置")?;
    match stored {
        None => Ok(DEFAULT_RETENTION_DAYS),
        Some(value) => value
            .parse::<u32>()
            .ok()
            .filter(|days| (1..=365).contains(days))
            .ok_or_else(|| "Agent 保留设置无效".into()),
    }
}

fn count_on(connection: &Connection) -> Result<usize, String> {
    connection
        .query_row("SELECT COUNT(*) FROM agent_observation_events", [], |row| {
            row.get(0)
        })
        .map_err(|_| "无法统计 Agent 观测记录".into())
}

impl EventStore {
    pub fn new(database: Database) -> Result<Self, String> {
        {
            let arc = database.conn();
            let mut connection = arc.lock().map_err(|_| "Agent 数据库暂不可用")?;
            let transaction = connection
                .transaction()
                .map_err(|_| "无法初始化 Agent 存储")?;
            transaction
                .execute_batch(
                    "CREATE TABLE IF NOT EXISTS agent_observation_events (
                    id TEXT PRIMARY KEY NOT NULL,
                    source_id TEXT NOT NULL,
                    client_id TEXT NOT NULL,
                    agent_id TEXT NOT NULL,
                    task_id TEXT,
                    timestamp INTEGER NOT NULL,
                    observed_at INTEGER NOT NULL,
                    kind TEXT NOT NULL,
                    parent_agent_id TEXT,
                    related_agent_id TEXT,
                    usage_key TEXT,
                    event_json TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS agent_observation_event_time
                    ON agent_observation_events(timestamp,id);
                CREATE INDEX IF NOT EXISTS agent_observation_event_agent
                    ON agent_observation_events(agent_id,timestamp,id);
                CREATE INDEX IF NOT EXISTS agent_observation_event_task
                    ON agent_observation_events(task_id,timestamp,id);
                CREATE INDEX IF NOT EXISTS agent_observation_event_kind
                    ON agent_observation_events(kind,agent_id,timestamp,id);
                CREATE INDEX IF NOT EXISTS agent_observation_spawn_target
                    ON agent_observation_events(related_agent_id,agent_id)
                    WHERE kind='agent_spawned';
                DROP INDEX IF EXISTS agent_observation_usage_key;
                CREATE UNIQUE INDEX IF NOT EXISTS agent_observation_usage_key_immutable
                    ON agent_observation_events(client_id,usage_key)
                    WHERE usage_key IS NOT NULL AND client_id<>'deepseek_harness';
                CREATE TABLE IF NOT EXISTS agent_observation_cursors (
                    source_id TEXT NOT NULL,
                    root_key TEXT NOT NULL,
                    path TEXT NOT NULL,
                    cursor_json TEXT NOT NULL,
                    PRIMARY KEY(source_id,root_key,path)
                );
                CREATE TABLE IF NOT EXISTS agent_observation_settings (
                    key TEXT PRIMARY KEY NOT NULL,
                    value TEXT NOT NULL
                );",
                )
                .map_err(|_| "无法初始化 Agent 存储")?;
            transaction
                .commit()
                .map_err(|_| "无法提交 Agent 存储初始化")?;
        }
        Ok(Self { database })
    }

    pub fn database(&self) -> &Database {
        &self.database
    }

    pub fn load_events(&self, at: Option<i64>) -> Result<Vec<AgentEvent>, String> {
        let arc = self.database.conn();
        let connection = arc.lock().map_err(|_| "Agent 数据库暂不可用")?;
        let mut statement = connection
            .prepare(
                "SELECT event_json FROM agent_observation_events
             WHERE (?1 IS NULL OR timestamp<=?1) ORDER BY timestamp,id",
            )
            .map_err(|_| "无法读取 Agent 观测记录")?;
        let rows = statement
            .query_map([at], |row| row.get::<_, String>(0))
            .map_err(|_| "无法读取 Agent 观测记录")?;
        rows.map(|row| decode_event(row.map_err(|_| "无法读取 Agent 观测记录")?))
            .collect()
    }

    pub fn append(
        &self,
        events: &[AgentEvent],
        cursor: &FileCursor,
    ) -> Result<Vec<AgentEvent>, String> {
        let encoded_cursor = cursor_json(cursor)?;
        let mut prepared = Vec::with_capacity(events.len());
        for original in events {
            let mut event = original.clone();
            if !super::privacy::valid_timestamp(event.timestamp)
                || event.lifecycle_at.is_some_and(|at| !super::privacy::valid_timestamp(at))
                || !super::privacy::valid_timestamp(event.observed_at)
                || event.context.as_ref().is_some_and(|context| !super::privacy::valid_timestamp(context.measured_at))
            {
                return Err("Agent 观测时间戳超出日期范围".into());
            }
            super::privacy::scrub_event(&mut event);
            super::privacy::price_event(&self.database, &mut event)
                .map_err(|_| "无法准备 Agent 观测用量")?;
            super::privacy::scrub_event(&mut event);
            if event.id.is_empty()
                || event.id.len() > 1024
                || event.agent_id.is_empty()
                || event.client_id.is_empty()
                || event.source_id != cursor.source_id
                || event.source_path != cursor.path
                || event
                    .usage
                    .as_ref()
                    .is_some_and(|usage| usage.key.is_empty() || usage.key.len() > 1024)
            {
                return Err("Agent 观测事件标识无效".into());
            }
            let encoded = serde_json::to_string(&event).map_err(|_| "无法编码 Agent 观测记录")?;
            if encoded.len() > 64 * 1024 {
                return Err("Agent 观测事件超过存储上限".into());
            }
            prepared.push((event, encoded));
        }
        let arc = self.database.conn();
        let mut connection = arc.lock().map_err(|_| "Agent 数据库暂不可用")?;
        let transaction = connection
            .transaction()
            .map_err(|_| "无法写入 Agent 观测记录")?;
        let mut inserted = Vec::new();
        {
            let mut statement = transaction
                .prepare_cached(
                    "INSERT INTO agent_observation_events
                 (id,source_id,client_id,agent_id,task_id,timestamp,observed_at,kind,
                  parent_agent_id,related_agent_id,usage_key,event_json)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12) ON CONFLICT DO NOTHING",
                )
                .map_err(|_| "无法写入 Agent 观测记录")?;
            for (event, encoded) in prepared {
                let changed = statement
                    .execute(params![
                        event.id,
                        event.source_id,
                        event.client_id,
                        event.agent_id,
                        event.task_id,
                        event.timestamp,
                        event.observed_at,
                        event.kind,
                        event.parent_agent_id,
                        event.related_agent_id,
                        event.usage.as_ref().map(|usage| &usage.key),
                        encoded
                    ])
                    .map_err(|_| "无法写入 Agent 观测记录")?;
                if changed != 0 {
                    inserted.push(event);
                }
            }
        }
        transaction
            .execute(
                "INSERT INTO agent_observation_cursors(source_id,root_key,path,cursor_json)
             VALUES(?1,?2,?3,?4) ON CONFLICT(source_id,root_key,path)
             DO UPDATE SET cursor_json=excluded.cursor_json",
                params![
                    cursor.source_id,
                    cursor.root_key,
                    cursor.path,
                    encoded_cursor
                ],
            )
            .map_err(|_| "无法保存 Agent 文件游标")?;
        transaction
            .commit()
            .map_err(|_| "无法提交 Agent 观测记录")?;
        inserted
            .sort_by(|left, right| (left.timestamp, &left.id).cmp(&(right.timestamp, &right.id)));
        Ok(inserted)
    }

    pub fn cursors(&self) -> Result<Vec<FileCursor>, String> {
        let arc = self.database.conn();
        let connection = arc.lock().map_err(|_| "Agent 数据库暂不可用")?;
        let mut statement = connection.prepare(
            "SELECT cursor_json FROM agent_observation_cursors ORDER BY source_id,root_key,path"
        ).map_err(|_| "无法读取 Agent 文件游标")?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|_| "无法读取 Agent 文件游标")?;
        rows.map(|row| {
            let encoded = row.map_err(|_| "无法读取 Agent 文件游标")?;
            let cursor: FileCursor =
                serde_json::from_str(&encoded).map_err(|_| "Agent 文件游标损坏")?;
            cursor_json(&cursor)?;
            Ok(cursor)
        })
        .collect()
    }

    pub fn event_page(
        &self,
        query: &AgentEventQuery,
        allowed_agents: Option<&[String]>,
    ) -> Result<AgentEventPage, String> {
        let limit = query.limit.unwrap_or(100);
        if limit == 0 || limit > MAX_PAGE_SIZE {
            return Err("Agent 事件分页大小须为 1 至 500".into());
        }
        if query
            .since
            .zip(query.until)
            .is_some_and(|(since, until)| since > until)
        {
            return Err("Agent 事件时间范围无效".into());
        }
        if query.team_id.is_some() && allowed_agents.is_none() {
            return Err("Agent 团队查询缺少成员范围".into());
        }
        let allowed = allowed_agents.map(|agents| {
            let mut agents = agents.to_vec();
            agents.sort();
            agents.dedup();
            agents
        });
        let allowed_json = allowed
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|_| "无法编码 Agent 成员范围")?;
        let filter = digest(
            &serde_json::to_vec(&(
                &query.team_id,
                &query.agent_id,
                &query.task_id,
                query.since,
                query.until,
                &allowed,
            ))
            .map_err(|_| "无法编码 Agent 查询")?,
        );
        let cursor = query
            .cursor
            .as_deref()
            .map(|text| decode_page_cursor(text, &filter))
            .transpose()?;
        let arc = self.database.conn();
        let connection = arc.lock().map_err(|_| "Agent 数据库暂不可用")?;
        let total = connection
            .query_row(
                "SELECT COUNT(*) FROM agent_observation_events
             WHERE (?1 IS NULL OR agent_id=?1) AND (?2 IS NULL OR task_id=?2)
               AND (?3 IS NULL OR timestamp>=?3) AND (?4 IS NULL OR timestamp<=?4)
               AND (?5 IS NULL OR agent_id IN (SELECT value FROM json_each(?5)))",
                params![
                    query.agent_id,
                    query.task_id,
                    query.since,
                    query.until,
                    allowed_json
                ],
                |row| row.get(0),
            )
            .map_err(|_| "无法统计 Agent 查询结果")?;
        let mut statement = connection
            .prepare(
                "SELECT event_json FROM agent_observation_events
             WHERE (?1 IS NULL OR agent_id=?1) AND (?2 IS NULL OR task_id=?2)
               AND (?3 IS NULL OR timestamp>=?3) AND (?4 IS NULL OR timestamp<=?4)
               AND (?5 IS NULL OR agent_id IN (SELECT value FROM json_each(?5)))
               AND (?6 IS NULL OR timestamp<?6 OR (timestamp=?6 AND id<?7))
             ORDER BY timestamp DESC,id DESC LIMIT ?8",
            )
            .map_err(|_| "无法读取 Agent 事件分页")?;
        let rows = statement
            .query_map(
                params![
                    query.agent_id,
                    query.task_id,
                    query.since,
                    query.until,
                    allowed_json,
                    cursor.as_ref().map(|cursor| cursor.timestamp),
                    cursor.as_ref().map(|cursor| &cursor.id),
                    (limit + 1) as i64,
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(|_| "无法读取 Agent 事件分页")?;
        let mut events = rows
            .map(|row| decode_event(row.map_err(|_| "无法读取 Agent 事件分页")?))
            .collect::<Result<Vec<_>, String>>()?;
        let has_more = events.len() > limit;
        events.truncate(limit);
        let next_cursor = if has_more {
            events
                .last()
                .map(|event| encode_page_cursor(event, filter))
                .transpose()?
        } else {
            None
        };
        Ok(AgentEventPage {
            events,
            next_cursor,
            total,
        })
    }

    pub fn history(&self) -> Result<AgentHistory, String> {
        let arc = self.database.conn();
        let connection = arc.lock().map_err(|_| "Agent 数据库暂不可用")?;
        let (oldest_at, latest_at, event_count) = connection
            .query_row(
                "SELECT MIN(timestamp),MAX(timestamp),COUNT(*) FROM agent_observation_events",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|_| "无法读取 Agent 历史范围")?;
        let truncated = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_observation_settings WHERE key='truncated' AND value='1')",
            [], |row| row.get(0),
        ).map_err(|_| "无法读取 Agent 历史范围")?;
        Ok(AgentHistory {
            oldest_at,
            latest_at,
            event_count,
            retention_days: retention_on(&connection)?,
            truncated,
        })
    }

    pub fn retention_days(&self) -> Result<u32, String> {
        let arc = self.database.conn();
        let connection = arc.lock().map_err(|_| "Agent 数据库暂不可用")?;
        retention_on(&connection)
    }

    pub fn set_retention_days(&self, days: u32) -> Result<(), String> {
        if !(1..=365).contains(&days) {
            return Err("Agent 保留天数须为 1 至 365".into());
        }
        let arc = self.database.conn();
        let connection = arc.lock().map_err(|_| "Agent 数据库暂不可用")?;
        connection
            .execute(
                "INSERT INTO agent_observation_settings(key,value) VALUES('retention_days',?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [days.to_string()],
            )
            .map_err(|_| "无法保存 Agent 保留设置")?;
        Ok(())
    }

    pub fn prune(&self, now: i64) -> Result<usize, String> {
        let arc = self.database.conn();
        let mut connection = arc.lock().map_err(|_| "Agent 数据库暂不可用")?;
        let transaction = connection
            .transaction()
            .map_err(|_| "无法维护 Agent 观测历史")?;
        let cutoff = now.saturating_sub(i64::from(retention_on(&transaction)?) * DAY_MS);
        let expired: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_observation_events WHERE timestamp<?1)",
                [cutoff],
                |row| row.get(0),
            )
            .map_err(|_| "无法检查 Agent 保留范围")?;
        let mut removed = 0;
        if expired {
            removed += transaction
                .execute(
                    "WITH RECURSIVE needed(agent_id) AS (
                    SELECT agent_id FROM agent_observation_events WHERE timestamp>=?1
                    UNION SELECT parent_agent_id FROM agent_observation_events
                          WHERE timestamp>=?1 AND parent_agent_id IS NOT NULL
                    UNION SELECT related_agent_id FROM agent_observation_events
                          WHERE timestamp>=?1 AND related_agent_id IS NOT NULL
                    UNION SELECT events.parent_agent_id FROM agent_observation_events events
                          JOIN needed ON events.agent_id=needed.agent_id
                          WHERE events.kind='agent_created' AND events.parent_agent_id IS NOT NULL
                    UNION SELECT events.related_agent_id FROM agent_observation_events events
                          JOIN needed ON events.agent_id=needed.agent_id
                          WHERE events.kind='agent_created' AND events.related_agent_id IS NOT NULL
                    UNION SELECT events.agent_id FROM agent_observation_events events
                          JOIN needed ON events.related_agent_id=needed.agent_id
                          WHERE events.kind='agent_spawned'
                 )
                 DELETE FROM agent_observation_events WHERE timestamp<?1
                   AND (kind<>'agent_created' OR agent_id NOT IN (SELECT agent_id FROM needed))
                   AND (kind<>'agent_spawned' OR related_agent_id IS NULL
                        OR related_agent_id NOT IN (SELECT agent_id FROM needed))",
                    [cutoff],
                )
                .map_err(|_| "无法清理过期 Agent 观测记录")?;
        }
        if count_on(&transaction)? > MAX_STORED_EVENTS {
            removed += enforce_event_limit(&transaction, MAX_STORED_EVENTS)?;
        }
        if removed != 0 {
            transaction
                .execute(
                    "INSERT INTO agent_observation_settings(key,value) VALUES('truncated','1')
                 ON CONFLICT(key) DO UPDATE SET value='1'",
                    [],
                )
                .map_err(|_| "无法保存 Agent 历史截断状态")?;
        }
        transaction
            .commit()
            .map_err(|_| "无法提交 Agent 历史清理")?;
        Ok(removed)
    }

    pub(super) fn source_seen(&self, source_id: &str, root_key: &str) -> Result<bool, String> {
        let arc = self.database.conn();
        let connection = arc.lock().map_err(|_| "Agent 数据库暂不可用")?;
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_observation_settings WHERE key=?1)",
                [format!("source_seen:{root_key}:{source_id}")],
                |row| row.get(0),
            )
            .map_err(|_| "无法读取 Agent 来源历史".into())
    }

    pub(super) fn mark_source_seen(&self, source_id: &str, root_key: &str) -> Result<(), String> {
        let arc = self.database.conn();
        let connection = arc.lock().map_err(|_| "Agent 数据库暂不可用")?;
        connection.execute(
            "INSERT INTO agent_observation_settings(key,value) VALUES(?1,'1') ON CONFLICT DO NOTHING",
            [format!("source_seen:{root_key}:{source_id}")],
        ).map_err(|_| "无法保存 Agent 来源历史")?;
        Ok(())
    }
}

fn encode_page_cursor(event: &AgentEvent, filter: String) -> Result<String, String> {
    let encoded = serde_json::to_vec(&PageCursor {
        version: 1,
        timestamp: event.timestamp,
        id: event.id.clone(),
        filter,
    })
    .map_err(|_| "无法编码 Agent 分页游标")?;
    let mut result = String::with_capacity(3 + encoded.len() * 2);
    result.push_str("v1.");
    use std::fmt::Write;
    for byte in encoded {
        let _ = write!(result, "{byte:02x}");
    }
    Ok(result)
}

fn decode_page_cursor(text: &str, filter: &str) -> Result<PageCursor, String> {
    let invalid = || "Agent 分页游标无效或不属于当前查询".to_string();
    let hex = text
        .strip_prefix("v1.")
        .filter(|hex| hex.len() <= 8192 && hex.len() % 2 == 0)
        .ok_or_else(invalid)?;
    let bytes = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16).ok_or_else(invalid)?;
            let low = (pair[1] as char).to_digit(16).ok_or_else(invalid)?;
            Ok((high * 16 + low) as u8)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let cursor: PageCursor = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if cursor.version != 1
        || cursor.id.is_empty()
        || cursor.id.len() > 1024
        || cursor.filter != filter
    {
        return Err(invalid());
    }
    Ok(cursor)
}

fn enforce_event_limit(connection: &Connection, limit: usize) -> Result<usize, String> {
    let read_ref = |row: &rusqlite::Row<'_>| -> rusqlite::Result<EventRef> {
        Ok(EventRef {
            id: row.get(0)?,
            agent_id: row.get(1)?,
            parent_agent_id: row.get(2)?,
            related_agent_id: row.get(3)?,
        })
    };
    let mut metadata = HashMap::new();
    {
        let mut statement = connection
            .prepare(
                "SELECT id,agent_id,parent_agent_id,related_agent_id FROM agent_observation_events
             WHERE kind='agent_created' ORDER BY timestamp,id",
            )
            .map_err(|_| "无法读取 Agent 元数据")?;
        for row in statement
            .query_map([], read_ref)
            .map_err(|_| "无法读取 Agent 元数据")?
        {
            let event = row.map_err(|_| "无法读取 Agent 元数据")?;
            metadata.entry(event.agent_id.clone()).or_insert(event);
        }
    }
    let mut spawns: HashMap<String, Vec<EventRef>> = HashMap::new();
    {
        let mut statement = connection
            .prepare(
                "SELECT id,agent_id,parent_agent_id,related_agent_id FROM agent_observation_events
                 WHERE kind='agent_spawned' AND related_agent_id IS NOT NULL ORDER BY timestamp,id",
            )
            .map_err(|_| "无法读取 Agent 派生关系")?;
        for row in statement
            .query_map([], read_ref)
            .map_err(|_| "无法读取 Agent 派生关系")?
        {
            let event = row.map_err(|_| "无法读取 Agent 派生关系")?;
            spawns
                .entry(
                    event
                        .related_agent_id
                        .clone()
                        .expect("non-null spawn target"),
                )
                .or_default()
                .push(event);
        }
    }
    let mut keep = HashSet::with_capacity(limit);
    {
        let mut statement = connection
            .prepare(
                "SELECT id,agent_id,parent_agent_id,related_agent_id FROM agent_observation_events
             ORDER BY timestamp DESC,id DESC",
            )
            .map_err(|_| "无法检查 Agent 历史容量")?;
        let rows = statement
            .query_map([], read_ref)
            .map_err(|_| "无法检查 Agent 历史容量")?;
        for row in rows {
            let event = row.map_err(|_| "无法检查 Agent 历史容量")?;
            if keep.contains(&event.id) {
                continue;
            }
            let mut required = HashSet::from([event.id.clone()]);
            let mut visited = HashSet::new();
            let mut pending = vec![event.agent_id];
            pending.extend(event.parent_agent_id);
            pending.extend(event.related_agent_id);
            while let Some(agent_id) = pending.pop() {
                if !visited.insert(agent_id.clone()) {
                    continue;
                }
                for evidence in metadata
                    .get(&agent_id)
                    .into_iter()
                    .chain(spawns.get(&agent_id).into_iter().flatten())
                {
                    if keep.contains(&evidence.id) {
                        continue;
                    }
                    required.insert(evidence.id.clone());
                    pending.push(evidence.agent_id.clone());
                    pending.extend(evidence.parent_agent_id.iter().cloned());
                    pending.extend(evidence.related_agent_id.iter().cloned());
                }
                if keep.len() + required.len() > limit {
                    break;
                }
            }
            if keep.len() + required.len() <= limit {
                keep.extend(required);
            }
            if keep.len() == limit {
                break;
            }
        }
    }
    let encoded = serde_json::to_string(&keep).map_err(|_| "无法编码 Agent 保留范围")?;
    connection.execute(
        "DELETE FROM agent_observation_events WHERE id NOT IN (SELECT value FROM json_each(?1))",
        [encoded],
    ).map_err(|_| "无法限制 Agent 历史容量".into())
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
