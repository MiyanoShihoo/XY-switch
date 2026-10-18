use serde::{Deserialize, Serialize};

/// 会话元信息。
///
/// 字段名必须与 `src/types/session.ts` 的 `SessionMeta` 一一对应。
/// 之前这里没有加 `rename_all = "camelCase"`，Rust 发出去的是
/// `provider_id` / `session_id`，而前端读的是 `providerId` / `sessionId`，
/// 列表里每一行都会显示成 `undefined`。现在只是因为命令固定返回空数组
/// 才没暴露出来。
#[derive(Serialize, Deserialize)]
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
    /// 在终端里恢复该会话的命令，例如 `claude --resume <id>`。
    pub resume_command: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
    pub ts: Option<i64>,
}

/// 批量删除时前端传过来的 `{ providerId, sessionId }`。
///
/// 同样必须是 camelCase：前端发的就是 camelCase，缺了 rename 之后
/// serde 会以「missing field `provider_id`」失败，用户看到的是一句
/// 看不懂的反序列化错误，而不是下面那句明确的「尚未实现」。
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionIdPair {
    pub provider_id: String,
    pub session_id: String,
}

/// 会话管理目前只做到「查询返回空列表」，写操作一律显式报错，
/// 不能返回 Ok 假装成功——否则前端会把行从列表里抹掉，用户以为删掉了，
/// 刷新后又冒出来。
const NOT_IMPLEMENTED: &str = "会话管理功能尚未实现，当前版本无法执行该操作";

#[tauri::command]
pub async fn list_sessions() -> Result<Vec<SessionMeta>, String> {
    Ok(vec![])
}

#[tauri::command]
pub async fn get_session_messages(
    _provider_id: String,
    _session_id: String,
) -> Result<Vec<SessionMessage>, String> {
    Ok(vec![])
}

#[tauri::command]
pub async fn delete_sessions(_ids: Vec<SessionIdPair>) -> Result<usize, String> {
    Err(NOT_IMPLEMENTED.to_string())
}
