//! Explicit user controls and bounded, offline analysis of passive shadow pairs.
use crate::{database::Database, model_audit, shadow_audit_runtime::{ShadowSettings, ShadowStatus}, usage::{UsageFilter, build_filter_clause}};
use serde::Serialize;
use std::collections::BTreeMap;
use tauri::State;

const SAMPLE_LIMIT: usize = 10_000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShadowAnalysis {
    sample_limit: usize,
    sample_count: usize,
    pair_count: usize,
    baseline_count: usize,
    excluded_count: usize,
    truncated: bool,
    groups: Vec<ShadowGroup>,
    reasons: Vec<ReasonCount>,
    dropped_requests: u64,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShadowGroup {
    provider_id: String,
    model: String,
    tokenizer_sha256: String,
    partition_id: String,
    regression: model_audit::ShadowRegressionReport,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReasonCount { reason: String, count: usize }

pub(crate) fn analyze_database(db: &Database, filter: &UsageFilter) -> Result<ShadowAnalysis, String> {
    let (where_clause, mut parameters) = build_filter_clause(filter);
    parameters.push(rusqlite::types::Value::Integer((SAMPLE_LIMIT + 1) as i64));
    let sql = format!(
        "SELECT s.provider_id,s.model,s.tokenizer_sha256,s.result_json
         FROM shadow_audit_samples s JOIN (SELECT * FROM request_events {where_clause}) e ON e.id=s.event_id
         WHERE e.route <> 'health_check' ORDER BY s.created_at DESC,s.id DESC LIMIT ?"
    );
    let rows = {
        let arc = db.conn();
        let connection = arc.lock().map_err(|error| error.to_string())?;
        let mut statement = connection.prepare(&sql).map_err(|error| error.to_string())?;
        let rows = statement.query_map(rusqlite::params_from_iter(parameters), |row| {
            Ok((row.get::<_, String>(0)?,row.get::<_, String>(1)?,row.get::<_, String>(2)?,row.get::<_, String>(3)?))
        }).map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|error| error.to_string())?
    };
    let mut report = ShadowAnalysis {
        sample_limit:SAMPLE_LIMIT, sample_count:rows.len().min(SAMPLE_LIMIT), pair_count:0, baseline_count:0, excluded_count:0,
        truncated:rows.len()>SAMPLE_LIMIT, groups:vec![], reasons:vec![],
        dropped_requests:db.shadow.statuses().iter().map(|status| status.dropped_requests).max().unwrap_or(0),
        notes:vec![
            "只分析启用后新采集的多轮业务请求；未配置或未产生配对时为未测到，不能从旧日志还原正文。".into(),
            "影子配对在有限内存和有效期内进行。变更历史、工具、多模态、缓存、失败或缺失用量会被排除；正文和配对用的内容哈希不写入数据库。".into(),
            "x 为本地分词器对新增文本的计数，y 为相邻轮输入 token 差；分组隔离渠道、模型、分词器和请求上下文。".into(),
            "3 对是回归的代数下限，达到它不代表证据充足：还需至少 5 个不同的本地增量取值、至少 32 token 的增量跨度，且截距在样本前后两段之间保持稳定。不存在固定等待 20 对的规则。".into(),
            "判读取决于斜率 95% 区间的整体位置：整体高于 1.05 或整体低于 0.95 记为统计偏离，整体落在 0.95～1.05 记为当前样本内未见偏离，跨越边界则保持未测到并继续采样。斜率偏离不能独立证明虚报：尚未排除上游模板、系统提示词重写和输入处理变化。".into(),
            "队列丢弃计数覆盖本次应用进程；配置变更或重启会重新建立内存配对分组。".into(),
        ],
    };
    let mut reasons:BTreeMap<String,usize> = BTreeMap::new();
    let mut groups:BTreeMap<(String,String,String,String),Vec<model_audit::ShadowPair>> = BTreeMap::new();
    // Retain chronological order for the intercept-drift check, after selecting
    // the newest bounded set. Never fit across unrelated context partitions.
    for (provider,model,sha,raw) in rows.into_iter().take(SAMPLE_LIMIT).collect::<Vec<_>>().into_iter().rev() {
        let parsed=serde_json::from_str::<serde_json::Value>(&raw).ok();
        let status=parsed.as_ref().and_then(|value|value["status"].as_str());
        if status==Some("paired") {
            let pair=parsed.as_ref().and_then(|value|serde_json::from_value::<model_audit::ShadowPair>(value["pair"].clone()).ok());
            if let Some(pair)=pair {
                report.pair_count+=1;
                groups.entry((provider,model,sha,pair.partition_id.clone())).or_default().push(pair);
                continue;
            }
        }
        if status==Some("baseline") {report.baseline_count+=1;} else {report.excluded_count+=1;}
        let reason=parsed.as_ref().and_then(|value|value["reason"].as_str()).filter(|reason|!reason.is_empty()).unwrap_or("shadow.invalid_record");
        *reasons.entry(reason.to_owned()).or_default()+=1;
    }
    for ((provider_id,model,tokenizer_sha256,partition_id),pairs) in groups {
        report.groups.push(ShadowGroup {provider_id,model,tokenizer_sha256,partition_id,regression:model_audit::regress_shadow(&pairs)});
    }
    report.groups.sort_by_key(|group|std::cmp::Reverse(group.regression.sample_count));
    if report.groups.len()>200 {report.groups.truncate(200);report.notes.push("仅展示样本最多的 200 个分组；计数仍覆盖本次选取的全部记录。".into());}
    report.reasons=reasons.into_iter().map(|(reason,count)|ReasonCount{reason,count}).collect();
    Ok(report)
}

#[tauri::command]
pub async fn get_shadow_audit_settings(db:State<'_,Database>)->Result<Vec<ShadowStatus>,String> {
    Ok(db.shadow.statuses())
}

#[tauri::command]
pub async fn save_shadow_audit_setting(request:ShadowSettings,db:State<'_,Database>)->Result<ShadowStatus,String> {
    if db.get_provider(&request.provider_id)?.is_none() {return Err("未找到该渠道，请使用现有渠道 ID。".into());}
    let runtime=db.shadow.clone();
    tauri::async_runtime::spawn_blocking(move||runtime.configure(request)).await.map_err(|error|error.to_string())?
}

#[tauri::command]
pub async fn analyze_shadow_audit(filter:UsageFilter,db:State<'_,Database>)->Result<ShadowAnalysis,String> {
    let db=db.inner().clone();
    tauri::async_runtime::spawn_blocking(move||analyze_database(&db,&filter)).await.map_err(|error|error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn shadow_audit_query_filters_exclusions_and_keeps_partitions_separate() {
        let path=std::env::temp_dir().join(format!("xy-shadow-query-{}.db",uuid::Uuid::new_v4()));
        let db=Database::open(&path).unwrap();
        db.save_provider(&serde_json::from_value(json!({"id":"p","name":"p","model":"native","endpoint":"http://127.0.0.1:1","appGroup":"Codex","enabled":true})).unwrap()).unwrap();
        {
            let arc=db.conn();let conn=arc.lock().unwrap();
            for index in 0..46_i64 {
                let route=if index==45{"health_check"}else{"/v1/chat/completions"};
                conn.execute("INSERT INTO request_events(provider_id,route,success,created_at,model) VALUES('p',?1,1,?2,'native')",rusqlite::params![route,index]).unwrap();
                let event_id=conn.last_insert_rowid();
                let partition=if index<20{"context-a"}else{"context-b"};
                let x=40+index*10;
                let outcome=if index==40 {json!({"status":"baseline","reason":"shadow.no_baseline","pair":null})}
                    else if index==41 {json!({"status":"excluded","reason":"shadow.cache_present","pair":null})}
                    else {json!({"status":"paired","reason":null,"pair":{"partitionId":partition,"xLocalTokens":x,"yInputDelta":x+8}})};
                let raw=if index==42{"malformed".into()}else{outcome.to_string()};
                conn.execute("INSERT INTO shadow_audit_samples(event_id,provider_id,model,tokenizer_sha256,created_at,result_json) VALUES(?1,'p','native','fixture-sha',?2,?3)",rusqlite::params![event_id,index,raw]).unwrap();
            }
        }
        let report=analyze_database(&db,&UsageFilter::default()).unwrap();
        assert_eq!(report.sample_count,45);
        assert_eq!(report.pair_count,42);
        assert_eq!(report.baseline_count,1);
        assert_eq!(report.excluded_count,2);
        assert_eq!(report.groups.len(),2);
        assert!(report.groups.iter().all(|group|group.regression.status=="consistent"));
        assert_eq!(report.notes.len(),6);
        let notes=report.notes.join("\n");
        for required in ["3 对是回归的代数下限", "至少 5 个不同的本地增量取值", "至少 32 token 的增量跨度", "斜率 95% 区间", "1.05", "0.95", "跨越边界则保持未测到"] {
            assert!(notes.contains(required),"missing adaptive report guidance: {required}");
        }
        assert!(!notes.contains("至少 20 对"));
        let filtered=analyze_database(&db,&UsageFilter{provider_id:Some("different".into()),..Default::default()}).unwrap();
        assert_eq!(filtered.sample_count,0);
        assert!(filtered.groups.is_empty());
        assert_eq!(filtered.notes,report.notes);
        println!("SHADOW_AUDIT_RESPONSE_JSON={}",json!({"populated":report,"empty":filtered}));
        drop(db);std::fs::remove_file(path).unwrap();
    }
}
