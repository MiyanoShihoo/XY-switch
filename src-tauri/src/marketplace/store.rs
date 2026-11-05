use super::auth::{self, PasswordHash};
use super::requests::{CatalogQuery, ListQuery, ReportRequest, ResolveReportRequest, page_bounds};
use super::{AuditEvent, AuthSession, CatalogEntry, DecisionRequest, MarketplaceError, MarketplaceInner,
    Page, PluginReport, PluginWithdrawal, PublishedPlugin, ReviewDecision, ReviewerConfig, SessionUser,
    Submission, SubmissionSummary, SubmissionStatus, SubmitRequest, VersionRequest, WithdrawRequest, checked_package,
    privacy, schema, signature_payload, verify_published};
use crate::plugins::{PluginPackage, PluginPermissions};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, path::{Path, PathBuf}, time::Duration};
use uuid::Uuid;

pub(super) struct MarketStore { path: PathBuf }

struct StoredSubmission {
    id: String,
    plugin_id: String,
    version: String,
    author_id: String,
    author_display_name: String,
    package_json: String,
    digest: String,
    status: String,
    approved_by: Option<String>,
    approved_version: Option<String>,
    approved_digest: Option<String>,
    created_at: i64,
    updated_at: i64,
    review_note: Option<String>,
}

const SUBMISSION_COLUMNS: &str = "id,plugin_id,version,author_id,author_display_name,package_json,digest,status,approved_by,approved_version,approved_digest,created_at,updated_at,review_note";

impl MarketStore {
    pub fn existing(path: &Path) -> Result<Self, MarketplaceError> {
        if !path.is_file() { return Err(MarketplaceError::InvalidConfiguration); }
        Ok(Self { path: path.to_path_buf() })
    }

    pub fn open(path: &Path, reviewers: &[ReviewerConfig], keys: &BTreeMap<String, Vec<u8>>) -> Result<Self, MarketplaceError> {
        let parent = path.parent().ok_or(MarketplaceError::InvalidConfiguration)?;
        std::fs::create_dir_all(parent).map_err(|_| MarketplaceError::Unavailable)?;
        let store = Self { path: path.to_path_buf() };
        let mut connection = store.connection()?;
        check_database_identity(&connection)?;
        {
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
            let is_new = check_database_identity(&transaction)?;
            if is_new {
                transaction.execute_batch(schema::SQL).map_err(db_error)?;
                transaction.pragma_update(None, "application_id", schema::APPLICATION_ID).map_err(db_error)?;
                transaction.pragma_update(None, "user_version", schema::SCHEMA_VERSION).map_err(db_error)?;
            }
            let required_keys: Vec<String> = {
                let mut statement = transaction.prepare("SELECT DISTINCT signing_key_id FROM releases UNION SELECT DISTINCT signing_key_id FROM withdrawals").map_err(db_error)?;
                statement.query_map([], |row| row.get(0)).map_err(db_error)?.collect::<Result<_,_>>().map_err(db_error)?
            };
            if required_keys.iter().any(|id| !keys.contains_key(id)) { return Err(MarketplaceError::InvalidConfiguration); }
            for (id, key) in keys {
                let previous: Option<Vec<u8>> = transaction.query_row("SELECT public_key FROM signing_keys WHERE id=?1", [id], |row| row.get(0)).optional().map_err(db_error)?;
                match previous {
                    Some(previous) if previous != *key => return Err(MarketplaceError::InvalidConfiguration),
                    Some(_) => {},
                    None => { transaction.execute("INSERT INTO signing_keys(id,public_key) VALUES(?1,?2)", params![id,key]).map_err(db_error)?; }
                }
            }
            reconcile_reviewers(&transaction, reviewers, auth::now()?)?;
            transaction.commit().map_err(db_error)?;
        }
        connection.pragma_update(None, "journal_mode", "WAL").map_err(db_error)?;
        let check: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0)).map_err(db_error)?;
        if check != "ok" { return Err(MarketplaceError::Unavailable); }
        Ok(store)
    }

    fn connection(&self) -> Result<Connection, MarketplaceError> {
        let connection = Connection::open_with_flags(&self.path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(db_error)?;
        connection.busy_timeout(Duration::from_secs(5)).map_err(db_error)?;
        connection.pragma_update(None, "foreign_keys", "ON").map_err(db_error)?;
        connection.pragma_update(None, "synchronous", "FULL").map_err(db_error)?;
        Ok(connection)
    }

    fn write<T>(&self, operation: impl FnOnce(&Transaction<'_>) -> Result<T, MarketplaceError>) -> Result<T, MarketplaceError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
        let result = operation(&transaction)?;
        transaction.commit().map_err(db_error)?;
        Ok(result)
    }

    fn read<T>(&self, operation: impl FnOnce(&Transaction<'_>) -> Result<T, MarketplaceError>) -> Result<T, MarketplaceError> {
        let mut connection = Connection::open_with_flags(&self.path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(db_error)?;
        connection.busy_timeout(Duration::from_secs(5)).map_err(db_error)?;
        connection.pragma_update(None, "query_only", "ON").map_err(db_error)?;
        let transaction = connection.transaction().map_err(db_error)?;
        let result = operation(&transaction)?;
        transaction.commit().map_err(db_error)?;
        Ok(result)
    }

    pub fn ready(&self) -> Result<(), MarketplaceError> {
        self.read(|transaction| {
            if check_database_identity(transaction)? { return Err(MarketplaceError::Unavailable); }
            Ok(())
        })
    }

    pub fn verify_state(&self, inner: &MarketplaceInner) -> Result<(), MarketplaceError> {
        self.read(|transaction| {
            if check_database_identity(transaction)? { return Err(MarketplaceError::Unavailable); }
            verify_schema(transaction)?;
            let integrity: Vec<String> = transaction.prepare("PRAGMA integrity_check").map_err(db_error)?
                .query_map([], |row| row.get(0)).map_err(db_error)?
                .collect::<Result<_, _>>().map_err(db_error)?;
            if integrity != ["ok"] || transaction.prepare("PRAGMA foreign_key_check").map_err(db_error)?
                .exists([]).map_err(db_error)? { return Err(MarketplaceError::Unavailable); }
            let ids: Vec<String> = transaction.prepare("SELECT id FROM submissions").map_err(db_error)?
                .query_map([], |row| row.get(0)).map_err(db_error)?
                .collect::<Result<_, _>>().map_err(db_error)?;
            for id in ids {
                let stored = stored_submission(transaction, &id)?;
                if stored.approved_by.as_deref().is_some_and(|reviewer| reviewer == stored.author_id)
                    || stored.approved_by.is_some() && (stored.approved_digest.as_deref() != Some(stored.digest.as_str())
                        || stored.approved_version.as_deref() != Some(stored.version.as_str())) {
                    return Err(MarketplaceError::Unavailable);
                }
                if stored.status == "published" {
                    release(transaction, inner, &stored.plugin_id, &stored.version)
                        .map_err(|_| MarketplaceError::Unavailable)?;
                }
                submission_view(transaction, inner, stored)?;
            }
            let releases: Vec<(String, String, String)> = transaction.prepare("SELECT plugin_id,version,status FROM releases").map_err(db_error)?
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).map_err(db_error)?
                .collect::<Result<_, _>>().map_err(db_error)?;
            for (id, version, status) in releases {
                if !matches!(status.as_str(), "published" | "withdrawn") { return Err(MarketplaceError::Unavailable); }
                release_in_state(transaction, inner, &id, &version, &status)
                    .map_err(|_| MarketplaceError::Unavailable)?;
                if status == "withdrawn" {
                    withdrawal_notice(transaction, inner, &id, &version)
                        .map_err(|_| MarketplaceError::Unavailable)?;
                } else if transaction.query_row("SELECT EXISTS(SELECT 1 FROM withdrawals WHERE plugin_id=?1 AND version=?2)",
                    params![id, version], |row| row.get::<_, bool>(0)).map_err(db_error)? {
                    return Err(MarketplaceError::Unavailable);
                }
            }
            let notices: Vec<(String, String)> = transaction.prepare("SELECT plugin_id,version FROM withdrawals").map_err(db_error)?
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?))).map_err(db_error)?
                .collect::<Result<_, _>>().map_err(db_error)?;
            for (id, version) in notices {
                withdrawal_notice(transaction, inner, &id, &version)
                    .map_err(|_| MarketplaceError::Unavailable)?;
            }
            Ok(())
        })
    }
}

fn verify_schema(connection: &Connection) -> Result<(), MarketplaceError> {
    let expected = Connection::open_in_memory().map_err(db_error)?;
    expected.execute_batch(schema::SQL).map_err(db_error)?;
    let definitions = |database: &Connection| -> Result<Vec<(String, String, String)>, MarketplaceError> {
        database.prepare("SELECT type,name,sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name").map_err(db_error)?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).map_err(db_error)?
            .collect::<Result<_, _>>().map_err(db_error)
    };
    if definitions(connection)? != definitions(&expected)? { return Err(MarketplaceError::Unavailable); }
    Ok(())
}

fn check_database_identity(connection: &Connection) -> Result<bool, MarketplaceError> {
    let application_id: i32 = connection.pragma_query_value(None, "application_id", |row| row.get(0)).map_err(db_error)?;
    let version: i32 = connection.pragma_query_value(None, "user_version", |row| row.get(0)).map_err(db_error)?;
    if application_id == schema::APPLICATION_ID && version == schema::SCHEMA_VERSION { return Ok(false); }
    let tables: i64 = connection.query_row("SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'", [], |row| row.get(0)).map_err(db_error)?;
    if application_id == 0 && version == 0 && tables == 0 { return Ok(true); }
    Err(MarketplaceError::InvalidConfiguration)
}

fn db_error(error: rusqlite::Error) -> MarketplaceError {
    match error {
        rusqlite::Error::SqliteFailure(failure, _) if failure.code == rusqlite::ErrorCode::ConstraintViolation => MarketplaceError::Conflict,
        _ => MarketplaceError::Unavailable,
    }
}

fn session_user(connection: &Connection, token: &str, timestamp: i64) -> Result<SessionUser, MarketplaceError> {
    connection.query_row("SELECT u.id,u.username,u.display_name,u.is_reviewer FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=?1 AND s.expires_at>?2 AND u.active=1",
        params![auth::token_hash(token)?,timestamp], |row| Ok(SessionUser {
            user_id: row.get(0)?, username: row.get(1)?, display_name: row.get(2)?, reviewer: row.get(3)?,
        })).optional().map_err(db_error)?.ok_or(MarketplaceError::AuthenticationRequired)
}

fn require_reviewer(user: &SessionUser) -> Result<(), MarketplaceError> {
    if !user.reviewer { return Err(MarketplaceError::Forbidden); }
    Ok(())
}

fn stored_submission(connection: &Connection, id: &str) -> Result<StoredSubmission, MarketplaceError> {
    if Uuid::parse_str(id).is_err() { return Err(MarketplaceError::NotFound); }
    connection.query_row(&format!("SELECT {SUBMISSION_COLUMNS} FROM submissions WHERE id=?1"), [id], |row| Ok(StoredSubmission {
        id: row.get(0)?, plugin_id: row.get(1)?, version: row.get(2)?, author_id: row.get(3)?,
        author_display_name: row.get(4)?, package_json: row.get(5)?, digest: row.get(6)?, status: row.get(7)?,
        approved_by: row.get(8)?, approved_version: row.get(9)?, approved_digest: row.get(10)?,
        created_at: row.get(11)?, updated_at: row.get(12)?, review_note: row.get(13)?,
    })).optional().map_err(db_error)?.ok_or(MarketplaceError::NotFound)
}

fn authorize_submission(user: &SessionUser, stored: &StoredSubmission) -> Result<(), MarketplaceError> {
    if stored.author_id != user.user_id && !user.reviewer { return Err(MarketplaceError::NotFound); }
    Ok(())
}

fn matches_version(stored: &StoredSubmission, expected: &VersionRequest) -> Result<(), MarketplaceError> {
    expected.validate()?;
    if stored.version != expected.version || stored.digest != expected.digest { return Err(MarketplaceError::Conflict); }
    Ok(())
}

fn record_audit(connection: &Connection, user: Option<&SessionUser>, stored: Option<&StoredSubmission>,
    action: &str, from: Option<&str>, to: Option<&str>, note: Option<&str>, timestamp: i64) -> Result<(), MarketplaceError> {
    connection.execute("INSERT INTO audit(submission_id,actor_id,actor_display_name,action,from_status,to_status,version,digest,note,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![stored.map(|item| item.id.as_str()),user.map(|actor| actor.user_id.as_str()),
            user.map(|actor| actor.display_name.as_str()).unwrap_or("[deployment]"),action,from,to,
            stored.map(|item| item.version.as_str()),stored.map(|item| item.digest.as_str()),note,timestamp]).map_err(db_error)?;
    Ok(())
}

fn reconcile_reviewers(transaction: &Transaction<'_>, configured: &[ReviewerConfig], timestamp: i64) -> Result<(), MarketplaceError> {
    let names: BTreeSet<&str> = configured.iter().map(|reviewer| reviewer.username.as_str()).collect();
    let existing: Vec<(String, String)> = {
        let mut statement = transaction.prepare("SELECT id,username FROM users WHERE provisioned_reviewer=1 AND active=1").map_err(db_error)?;
        statement.query_map([], |row| Ok((row.get(0)?,row.get(1)?))).map_err(db_error)?.collect::<Result<_,_>>().map_err(db_error)?
    };
    for (id, username) in existing {
        if !names.contains(username.as_str()) {
            transaction.execute("DELETE FROM sessions WHERE user_id=?1", [&id]).map_err(db_error)?;
            transaction.execute("UPDATE users SET active=0,is_reviewer=0 WHERE id=?1", [&id]).map_err(db_error)?;
            record_audit(transaction, None, None, "reviewer_removed", None, None, None, timestamp)?;
        }
    }
    for reviewer in configured {
        let hash_json = serde_json::to_string(&reviewer.password_hash).map_err(|_| MarketplaceError::InvalidConfiguration)?;
        let previous: Option<(String,bool,String,String,bool)> = transaction.query_row(
            "SELECT id,provisioned_reviewer,password_hash,display_name,active FROM users WHERE username=?1", [&reviewer.username],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))).optional().map_err(db_error)?;
        match previous {
            Some((_, false, _, _, _)) => return Err(MarketplaceError::InvalidConfiguration),
            Some((id, true, old_hash, old_name, active)) if old_hash != hash_json || old_name != reviewer.display_name || !active => {
                transaction.execute("DELETE FROM sessions WHERE user_id=?1", [&id]).map_err(db_error)?;
                transaction.execute("UPDATE users SET password_hash=?1,display_name=?2,is_reviewer=1,active=1 WHERE id=?3",
                    params![hash_json,reviewer.display_name,id]).map_err(db_error)?;
                record_audit(transaction, None, None, "reviewer_reconfigured", None, None, None, timestamp)?;
            },
            Some(_) => {},
            None => {
                transaction.execute("INSERT INTO users(id,username,display_name,password_hash,is_reviewer,provisioned_reviewer,active,created_at) VALUES(?1,?2,?3,?4,1,1,1,?5)",
                    params![Uuid::new_v4().to_string(),reviewer.username,reviewer.display_name,hash_json,timestamp]).map_err(db_error)?;
                record_audit(transaction, None, None, "reviewer_provisioned", None, None, None, timestamp)?;
            }
        }
    }
    Ok(())
}

impl MarketStore {
    pub fn register(&self, username: &str, display_name: &str, password: &str, ttl: i64) -> Result<AuthSession, MarketplaceError> {
        let username = auth::normalize_username(username)?;
        privacy::inspect_display_name(display_name)?;
        let password_hash = auth::hash_password_for_deployment(password)?;
        let hash_json = serde_json::to_string(&password_hash).map_err(|_| MarketplaceError::Unavailable)?;
        self.write(|transaction| {
            let timestamp = auth::now()?;
            let user = SessionUser { user_id: Uuid::new_v4().to_string(), username: username.clone(), display_name: display_name.to_owned(), reviewer: false };
            transaction.execute("INSERT INTO users(id,username,display_name,password_hash,is_reviewer,provisioned_reviewer,active,created_at) VALUES(?1,?2,?3,?4,0,0,1,?5)",
                params![user.user_id,user.username,user.display_name,hash_json,timestamp]).map_err(db_error)?;
            record_audit(transaction, Some(&user), None, "registered", None, None, None, timestamp)?;
            create_session(transaction, user, timestamp, ttl)
        })
    }

    pub fn login(&self, username: &str, password: &str, ttl: i64) -> Result<AuthSession, MarketplaceError> {
        let username = auth::normalize_username(username).map_err(|_| MarketplaceError::InvalidCredentials)?;
        auth::validate_password(password).map_err(|_| MarketplaceError::InvalidCredentials)?;
        let username_hash = Sha256::digest(username.as_bytes()).to_vec();
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
        let timestamp = auth::now()?;
        transaction.execute("DELETE FROM login_attempts WHERE window_start<?1", [timestamp-86400]).map_err(db_error)?;
        let attempt: Option<(i64,i64,i64)> = transaction.query_row(
            "SELECT window_start,failures,blocked_until FROM login_attempts WHERE username_hash=?1", [&username_hash],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional().map_err(db_error)?;
        if attempt.is_some_and(|(_,_,blocked)| blocked>timestamp) { return Err(MarketplaceError::RateLimited); }
        if attempt.is_none() {
            let count: i64 = transaction.query_row("SELECT count(*) FROM login_attempts", [], |row| row.get(0)).map_err(db_error)?;
            if count >= 10_000 { return Err(MarketplaceError::RateLimited); }
        }
        let account: Option<(SessionUser,String,bool)> = transaction.query_row(
            "SELECT id,username,display_name,is_reviewer,password_hash,active FROM users WHERE username=?1", [&username], |row| Ok((SessionUser {
                user_id: row.get(0)?, username: row.get(1)?, display_name: row.get(2)?, reviewer: row.get(3)?,
            },row.get(4)?,row.get(5)?))).optional().map_err(db_error)?;
        let valid = if let Some((_, encoded, active)) = &account {
            let hash: PasswordHash = serde_json::from_str(encoded).map_err(|_| MarketplaceError::Unavailable)?;
            hash.verify(password)? && *active
        } else {
            let dummy = PasswordHash { algorithm: "pbkdf2-sha256".to_owned(), iterations: auth::PASSWORD_ITERATIONS,
                salt_hex: "00".repeat(32), hash_hex: "00".repeat(32) };
            let _ = dummy.verify(password)?;
            false
        };
        if !valid {
            let (window_start, failures) = match attempt {
                Some((start, count, _)) if start+900>timestamp => (start,count+1),
                _ => (timestamp,1),
            };
            let blocked_until = if failures>=5 { timestamp+900 } else { 0 };
            transaction.execute("INSERT INTO login_attempts(username_hash,window_start,failures,blocked_until) VALUES(?1,?2,?3,?4) ON CONFLICT(username_hash) DO UPDATE SET window_start=excluded.window_start,failures=excluded.failures,blocked_until=excluded.blocked_until",
                params![username_hash,window_start,failures,blocked_until]).map_err(db_error)?;
            transaction.commit().map_err(db_error)?;
            return Err(MarketplaceError::InvalidCredentials);
        }
        let (user,_,_) = account.ok_or(MarketplaceError::Unavailable)?;
        transaction.execute("DELETE FROM login_attempts WHERE username_hash=?1", [&username_hash]).map_err(db_error)?;
        record_audit(&transaction, Some(&user), None, "logged_in", None, None, None, timestamp)?;
        let session = create_session(&transaction, user, timestamp, ttl)?;
        transaction.commit().map_err(db_error)?;
        Ok(session)
    }

    pub fn me(&self, token: &str) -> Result<SessionUser, MarketplaceError> {
        self.read(|transaction| session_user(transaction, token, auth::now()?))
    }

    pub fn logout(&self, token: &str) -> Result<(), MarketplaceError> {
        self.write(|transaction| {
            let timestamp = auth::now()?;
            let user = session_user(transaction, token, timestamp)?;
            transaction.execute("DELETE FROM sessions WHERE token_hash=?1", [auth::token_hash(token)?]).map_err(db_error)?;
            record_audit(transaction, Some(&user), None, "logged_out", None, None, None, timestamp)
        })
    }
}

fn create_session(transaction: &Transaction<'_>, user: SessionUser, timestamp: i64, ttl: i64) -> Result<AuthSession, MarketplaceError> {
    let token = auth::encode_hex(&auth::random_bytes::<32>()?);
    let expires_at = timestamp.checked_add(ttl).ok_or(MarketplaceError::Unavailable)?;
    transaction.execute("DELETE FROM sessions WHERE expires_at<=?1", [timestamp]).map_err(db_error)?;
    transaction.execute("DELETE FROM sessions WHERE token_hash IN (SELECT token_hash FROM sessions WHERE user_id=?1 ORDER BY created_at DESC,token_hash DESC LIMIT -1 OFFSET 9)", [&user.user_id]).map_err(db_error)?;
    transaction.execute("INSERT INTO sessions(token_hash,user_id,created_at,expires_at) VALUES(?1,?2,?3,?4)",
        params![auth::token_hash(&token)?,user.user_id,timestamp,expires_at]).map_err(db_error)?;
    Ok(AuthSession { token, token_type: "Bearer", expires_at, user })
}

impl MarketStore {
    pub fn submit(&self, inner: &MarketplaceInner, token: &str, request: SubmitRequest) -> Result<Submission, MarketplaceError> {
        self.write(|transaction| {
            let timestamp = auth::now()?;
            let user = session_user(transaction, token, timestamp)?;
            let value = serde_json::to_value(&request.package).map_err(|_| MarketplaceError::InvalidPackage)?;
            privacy::inspect_package_with_secrets(&value, &[token, &user.user_id])?;
            let (json, digest) = checked_package(&request.package)?;
            let manifest = &request.package.manifest;
            if manifest.author != user.display_name { return Err(MarketplaceError::InvalidPackage); }
            let pending: i64 = transaction.query_row("SELECT count(*) FROM submissions WHERE author_id=?1 AND status IN ('submitted','approved')", [&user.user_id], |row| row.get(0)).map_err(db_error)?;
            if pending >= 20 { return Err(MarketplaceError::RateLimited); }
            let owner: Option<String> = transaction.query_row("SELECT owner_id FROM plugins WHERE id=?1", [&manifest.id], |row| row.get(0)).optional().map_err(db_error)?;
            match owner {
                Some(owner) if owner != user.user_id => return Err(MarketplaceError::Forbidden),
                Some(_) => {},
                None => { transaction.execute("INSERT INTO plugins(id,owner_id,created_at) VALUES(?1,?2,?3)", params![manifest.id,user.user_id,timestamp]).map_err(db_error)?; }
            }
            if let Some(previous_id) = &request.replaces_submission_id {
                let previous = stored_submission(transaction, previous_id)?;
                if previous.author_id != user.user_id || previous.plugin_id != manifest.id || previous.version == manifest.version
                    || !matches!(previous.status.as_str(), "changes_requested" | "rejected" | "published" | "withdrawn") {
                    return Err(MarketplaceError::Conflict);
                }
            }
            let id = Uuid::new_v4().to_string();
            transaction.execute("INSERT INTO submissions(id,plugin_id,version,author_id,author_display_name,package_json,digest,status,replaces_id,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,'submitted',?8,?9,?9)",
                params![id,manifest.id,manifest.version,user.user_id,user.display_name,json,digest,request.replaces_submission_id,timestamp]).map_err(db_error)?;
            let stored = stored_submission(transaction, &id)?;
            record_audit(transaction, Some(&user), Some(&stored), "submitted", None, Some("submitted"), None, timestamp)?;
            submission_view(transaction, inner, stored)
        })
    }

    pub fn submission(&self, inner: &MarketplaceInner, token: &str, id: &str) -> Result<Submission, MarketplaceError> {
        self.read(|transaction| {
            let user = session_user(transaction, token, auth::now()?)?;
            let stored = stored_submission(transaction, id)?;
            authorize_submission(&user, &stored)?;
            submission_view(transaction, inner, stored)
        })
    }

    pub fn submissions(&self, inner: &MarketplaceInner, token: &str, query: &ListQuery, review: bool) -> Result<Page<SubmissionSummary>, MarketplaceError> {
        self.read(|transaction| {
            let user = session_user(transaction, token, auth::now()?)?;
            if review { require_reviewer(&user)?; }
            let (limit,offset) = page_bounds(query.limit,query.offset)?;
            let status = query.status.as_deref().or(if review { Some("submitted") } else { None });
            if let Some(status) = status { SubmissionStatus::parse(status).map_err(|_| MarketplaceError::InvalidRequest)?; }
            let ids: Vec<String> = {
                let mut statement = transaction.prepare("SELECT id FROM submissions WHERE (?1 IS NULL OR author_id=?1) AND (?2 IS NULL OR status=?2) ORDER BY created_at DESC,id DESC LIMIT ?3 OFFSET ?4").map_err(db_error)?;
                statement.query_map(params![if review { None } else { Some(user.user_id.as_str()) },status,limit+1,offset], |row| row.get(0)).map_err(db_error)?.collect::<Result<_,_>>().map_err(db_error)?
            };
            let mut items = Vec::new();
            for id in ids.iter().take(limit as usize) {
                let submission = submission_view(transaction, inner, stored_submission(transaction,id)?)?;
                let manifest = submission.package.manifest;
                items.push(SubmissionSummary {
                    submission_id: submission.submission_id, plugin_id: manifest.id, name: manifest.name, version: manifest.version,
                    digest: submission.digest, status: submission.status, author_display_name: submission.author_display_name,
                    created_at: submission.created_at, updated_at: submission.updated_at, review_note: submission.review_note,
                    previous_published_version: submission.previous_published_version, permission_changes: submission.permission_changes,
                    permissions: manifest.permissions, clients: manifest.clients,
                });
            }
            Ok(Page { items,limit,offset,has_more: ids.len()>limit as usize })
        })
    }

    pub fn decide(&self, inner: &MarketplaceInner, token: &str, id: &str, request: &DecisionRequest) -> Result<Submission, MarketplaceError> {
        self.write(|transaction| {
            let timestamp = auth::now()?;
            let user = session_user(transaction, token, timestamp)?;
            require_reviewer(&user)?;
            let stored = stored_submission(transaction, id)?;
            if stored.author_id == user.user_id { return Err(MarketplaceError::Forbidden); }
            matches_version(&stored, &VersionRequest { version: request.version.clone(),digest: request.digest.clone() })?;
            if stored.status != "submitted" { return Err(MarketplaceError::Conflict); }
            privacy::inspect_note_with_secrets(&request.note, &[token,&user.user_id])?;
            let (status,action) = match request.decision {
                ReviewDecision::Approve => ("approved","approved"),
                ReviewDecision::RequestChanges => ("changes_requested","changes_requested"),
                ReviewDecision::Reject => ("rejected","rejected"),
            };
            if status != "approved" && request.note.trim().is_empty() { return Err(MarketplaceError::InvalidRequest); }
            let approving = status == "approved";
            transaction.execute("UPDATE submissions SET status=?1,approved_by=?2,approved_version=?3,approved_digest=?4,approved_at=?5,review_note=?6,updated_at=?7 WHERE id=?8 AND status='submitted'",
                params![status,if approving { Some(user.user_id.as_str()) } else { None },
                    if approving { Some(stored.version.as_str()) } else { None },if approving { Some(stored.digest.as_str()) } else { None },
                    if approving { Some(timestamp) } else { None },request.note,timestamp,id]).map_err(db_error)?;
            record_audit(transaction,Some(&user),Some(&stored),action,Some("submitted"),Some(status),Some(&request.note),timestamp)?;
            submission_view(transaction,inner,stored_submission(transaction,id)?)
        })
    }
}

fn package_from_storage(json: &str, id: &str, version: &str, digest: &str) -> Result<PluginPackage, MarketplaceError> {
    let value = crate::plugins::validation::strict_json(json, MAX_STORED_BYTES).map_err(|_| MarketplaceError::Unavailable)?;
    let package: PluginPackage = serde_json::from_value(value).map_err(|_| MarketplaceError::Unavailable)?;
    let (canonical,actual) = checked_package(&package).map_err(|_| MarketplaceError::Unavailable)?;
    if canonical != json || actual != digest || package.manifest.id != id || package.manifest.version != version { return Err(MarketplaceError::Unavailable); }
    Ok(package)
}

const MAX_STORED_BYTES: usize = crate::plugins::MAX_PACKAGE_BYTES;

fn submission_view(connection: &Connection, inner: &MarketplaceInner, stored: StoredSubmission) -> Result<Submission, MarketplaceError> {
    let package = package_from_storage(&stored.package_json,&stored.plugin_id,&stored.version,&stored.digest)?;
    if package.manifest.author != stored.author_display_name { return Err(MarketplaceError::Unavailable); }
    let previous: Option<String> = connection.query_row("SELECT version FROM releases WHERE plugin_id=?1 AND version<>?2 AND status='published' ORDER BY rowid DESC LIMIT 1",
        params![stored.plugin_id,stored.version], |row| row.get(0)).optional().map_err(db_error)?;
    let previous_permissions = match &previous {
        Some(version) => release(connection,inner,&stored.plugin_id,version)?.package.manifest.permissions,
        None => PluginPermissions::default(),
    };
    let permission_changes = package.manifest.permissions.added_to(&previous_permissions);
    Ok(Submission { submission_id: stored.id,status: SubmissionStatus::parse(&stored.status)?,package,digest: stored.digest,
        author_display_name: stored.author_display_name,created_at: stored.created_at,updated_at: stored.updated_at,
        review_note: stored.review_note,previous_published_version: previous,permission_changes })
}

impl MarketStore {
    pub fn publish(&self, inner: &MarketplaceInner, token: &str, id: &str, expected: &VersionRequest) -> Result<PublishedPlugin, MarketplaceError> {
        self.write(|transaction| {
            let timestamp = auth::now()?;
            let user = session_user(transaction,token,timestamp)?;
            let stored = stored_submission(transaction,id)?;
            authorize_submission(&user,&stored)?;
            matches_version(&stored,expected)?;
            if stored.status != "approved" || stored.approved_digest.as_deref() != Some(stored.digest.as_str())
                || stored.approved_version.as_deref() != Some(stored.version.as_str())
                || stored.approved_by.as_deref() == Some(stored.author_id.as_str()) { return Err(MarketplaceError::Conflict); }
            let approved_by = stored.approved_by.as_deref().ok_or(MarketplaceError::Conflict)?;
            let reviewer_active: bool = transaction.query_row("SELECT active=1 AND is_reviewer=1 FROM users WHERE id=?1", [approved_by], |row| row.get(0)).map_err(db_error)?;
            if !reviewer_active { return Err(MarketplaceError::Conflict); }
            let package = package_from_storage(&stored.package_json,&stored.plugin_id,&stored.version,&stored.digest)?;
            privacy::inspect_package_with_secrets(&serde_json::to_value(&package).map_err(|_| MarketplaceError::Unavailable)?, &[token,&user.user_id])?;
            for dependency in &package.manifest.dependencies {
                let (dependency_id,dependency_version) = crate::plugins::validation::parse_dependency(dependency).map_err(|_| MarketplaceError::InvalidPackage)?;
                release(transaction,inner,dependency_id,dependency_version).map_err(|error|
                    if error == MarketplaceError::NotFound { MarketplaceError::Conflict } else { error })?;
            }
            let signature = auth::encode_hex(inner.signer.sign(&signature_payload(&stored.plugin_id,&stored.version,&stored.digest)).as_ref());
            let published = PublishedPlugin { package,digest: stored.digest.clone(),signature,signing_key_id: inner.signing_key_id.clone() };
            verify_published(inner,&published)?;
            transaction.execute("INSERT INTO releases(submission_id,plugin_id,version,author_display_name,package_json,digest,signature,signing_key_id,status,published_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'published',?9)",
                params![stored.id,stored.plugin_id,stored.version,stored.author_display_name,stored.package_json,stored.digest,published.signature,published.signing_key_id,timestamp]).map_err(db_error)?;
            transaction.execute("UPDATE submissions SET status='published',updated_at=?1 WHERE id=?2 AND status='approved'",params![timestamp,id]).map_err(db_error)?;
            record_audit(transaction,Some(&user),Some(&stored),"published",Some("approved"),Some("published"),None,timestamp)?;
            Ok(published)
        })
    }

    pub fn withdraw(&self, inner: &MarketplaceInner, token: &str, id: &str, request: &WithdrawRequest) -> Result<Submission, MarketplaceError> {
        self.write(|transaction| {
            let timestamp = auth::now()?;
            let user = session_user(transaction,token,timestamp)?;
            let stored = stored_submission(transaction,id)?;
            authorize_submission(&user,&stored)?;
            matches_version(&stored,&VersionRequest { version: request.version.clone(),digest: request.digest.clone() })?;
            privacy::inspect_note_with_secrets(&request.note,&[token,&user.user_id])?;
            if stored.status == "withdrawn" { return Err(MarketplaceError::Conflict); }
            if stored.status == "published" {
                let published = release(transaction,inner,&stored.plugin_id,&stored.version)
                    .map_err(|_| MarketplaceError::Unavailable)?;
                if published.digest != stored.digest { return Err(MarketplaceError::Unavailable); }
                let mut notice = PluginWithdrawal { id: stored.plugin_id.clone(),version: stored.version.clone(),digest: published.digest,
                    reason: request.reason.clone(),signing_key_id: inner.signing_key_id.clone(),signature: String::new() };
                let payload = crate::plugins::validation::withdrawal_payload(&notice).map_err(|_| MarketplaceError::Unavailable)?;
                notice.signature = auth::encode_hex(inner.signer.sign(&payload).as_ref());
                verify_withdrawal(inner,&notice)?;
                transaction.execute("INSERT INTO withdrawals(plugin_id,version,digest,reason,signing_key_id,signature,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                    params![notice.id,notice.version,notice.digest,notice.reason.as_str(),notice.signing_key_id,notice.signature,timestamp]).map_err(db_error)?;
                let changed = transaction.execute("UPDATE releases SET status='withdrawn' WHERE submission_id=?1 AND status='published'",[id]).map_err(db_error)?;
                if changed != 1 { return Err(MarketplaceError::Unavailable); }
            }
            transaction.execute("UPDATE submissions SET status='withdrawn',updated_at=?1 WHERE id=?2",params![timestamp,id]).map_err(db_error)?;
            record_audit(transaction,Some(&user),Some(&stored),"withdrawn",Some(&stored.status),Some("withdrawn"),Some(&request.note),timestamp)?;
            submission_view(transaction,inner,stored_submission(transaction,id)?)
        })
    }

    pub fn download(&self, inner: &MarketplaceInner, id: &str, version: &str) -> Result<PublishedPlugin, MarketplaceError> {
        validate_locator(id,version)?;
        self.read(|transaction| release(transaction,inner,id,version))
    }

    pub fn withdrawal(&self, inner: &MarketplaceInner, id: &str, version: &str) -> Result<PluginWithdrawal, MarketplaceError> {
        validate_locator(id,version)?;
        self.read(|transaction| withdrawal_notice(transaction,inner,id,version))
    }

    pub fn catalog(&self, inner: &MarketplaceInner, query: &CatalogQuery, plugin_id: Option<&str>) -> Result<Page<CatalogEntry>, MarketplaceError> {
        query.validate()?;
        if let Some(id) = plugin_id { crate::plugins::validation::validate_id(id).map_err(|_| MarketplaceError::NotFound)?; }
        self.read(|transaction| {
            let (limit,offset) = page_bounds(query.limit,query.offset)?;
            let mut statement = transaction.prepare(
                "SELECT r.plugin_id,r.version,r.author_display_name,r.package_json,r.digest,r.signature,r.signing_key_id,r.published_at FROM releases r JOIN submissions s ON s.id=r.submission_id WHERE r.status='published' AND s.status='published'
                AND (?1 IS NULL OR r.plugin_id=?1) AND (?2 IS NULL OR json_extract(r.package_json,'$.manifest.kind')=?2)
                AND (?3 IS NULL OR EXISTS(SELECT 1 FROM json_each(r.package_json,'$.manifest.clients') WHERE value=?3))
                AND (?4 IS NULL OR instr(lower(json_extract(r.package_json,'$.manifest.name')||' '||json_extract(r.package_json,'$.manifest.description')),lower(?4))>0)
                AND (?1 IS NOT NULL OR NOT EXISTS(SELECT 1 FROM releases newer JOIN submissions ns ON ns.id=newer.submission_id WHERE newer.plugin_id=r.plugin_id AND newer.status='published' AND ns.status='published' AND newer.rowid>r.rowid))
                ORDER BY r.rowid DESC LIMIT ?5 OFFSET ?6").map_err(db_error)?;
            let mut rows = statement.query(params![plugin_id,query.kind,query.client,query.q,limit+1,offset]).map_err(db_error)?;
            let mut items = Vec::new();
            let mut has_more = false;
            while let Some(row) = rows.next().map_err(db_error)? {
                if items.len() == limit as usize { has_more=true; break; }
                let id: String = row.get(0).map_err(db_error)?;
                let version: String = row.get(1).map_err(db_error)?;
                let author: String = row.get(2).map_err(db_error)?;
                let json: String = row.get(3).map_err(db_error)?;
                let digest: String = row.get(4).map_err(db_error)?;
                let published = PublishedPlugin { package: package_from_storage(&json,&id,&version,&digest)?,digest,
                    signature: row.get(5).map_err(db_error)?,signing_key_id: row.get(6).map_err(db_error)? };
                if author != published.package.manifest.author { return Err(MarketplaceError::Unavailable); }
                verify_published(inner,&published)?;
                let manifest = published.package.manifest;
                items.push(CatalogEntry { id: manifest.id,name: manifest.name,description: manifest.description,author_display_name: author,
                    version: manifest.version,kind: serde_json::to_value(manifest.kind).map_err(|_| MarketplaceError::Unavailable)?,clients: manifest.clients,
                    digest: published.digest,signature: published.signature,signing_key_id: published.signing_key_id,published_at: row.get(7).map_err(db_error)? });
            }
            Ok(Page { items,limit,offset,has_more })
        })
    }
}

fn validate_locator(id: &str, version: &str) -> Result<(), MarketplaceError> {
    crate::plugins::validation::validate_id(id).map_err(|_| MarketplaceError::NotFound)?;
    crate::plugins::validation::parse_version(version).map_err(|_| MarketplaceError::NotFound)?;
    Ok(())
}

fn release(connection: &Connection, inner: &MarketplaceInner, id: &str, version: &str) -> Result<PublishedPlugin, MarketplaceError> {
    release_in_state(connection,inner,id,version,"published")
}

fn release_in_state(connection: &Connection, inner: &MarketplaceInner, id: &str, version: &str, status: &str) -> Result<PublishedPlugin, MarketplaceError> {
    let stored: Option<(String,String,String,String,String,String)> = connection.query_row(
        "SELECT package_json,digest,signature,signing_key_id,author_display_name,submission_id FROM releases WHERE plugin_id=?1 AND version=?2 AND status=?3",
        params![id,version,status], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?))).optional().map_err(db_error)?;
    let (json,digest,signature,signing_key_id,author,submission_id) = stored.ok_or(MarketplaceError::NotFound)?;
    let submission = stored_submission(connection,&submission_id).map_err(|_| MarketplaceError::Unavailable)?;
    if submission.plugin_id != id || submission.version != version || submission.status != status
        || submission.digest != digest || submission.package_json != json || submission.author_display_name != author
        || submission.approved_digest.as_deref() != Some(digest.as_str())
        || submission.approved_version.as_deref() != Some(version)
        || submission.approved_by.as_deref().is_none_or(|reviewer| reviewer == submission.author_id) {
        return Err(MarketplaceError::Unavailable);
    }
    let package = package_from_storage(&json,id,version,&digest)?;
    if package.manifest.author != author { return Err(MarketplaceError::Unavailable); }
    let published = PublishedPlugin { package,digest,signature,signing_key_id };
    verify_published(inner,&published)?;
    Ok(published)
}

fn withdrawal_notice(connection: &Connection, inner: &MarketplaceInner, id: &str, version: &str) -> Result<PluginWithdrawal, MarketplaceError> {
    let record: Option<(String,String,String,String)> = connection.query_row(
        "SELECT digest,reason,signing_key_id,signature FROM withdrawals WHERE plugin_id=?1 AND version=?2",
        params![id,version], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional().map_err(db_error)?;
    let (digest,reason,signing_key_id,signature) = record.ok_or(MarketplaceError::NotFound)?;
    let published = release_in_state(connection,inner,id,version,"withdrawn")
        .map_err(|_| MarketplaceError::Unavailable)?;
    if digest != published.digest { return Err(MarketplaceError::Unavailable); }
    let reason = serde_json::from_value(serde_json::Value::String(reason)).map_err(|_| MarketplaceError::Unavailable)?;
    let notice = PluginWithdrawal { id: id.to_owned(),version: version.to_owned(),digest,reason,signing_key_id,signature };
    verify_withdrawal(inner,&notice)?;
    Ok(notice)
}

fn verify_withdrawal(inner: &MarketplaceInner, notice: &PluginWithdrawal) -> Result<(), MarketplaceError> {
    let key = inner.verification_keys.get(&notice.signing_key_id).ok_or(MarketplaceError::Unavailable)?;
    let payload = crate::plugins::validation::withdrawal_payload(notice).map_err(|_| MarketplaceError::Unavailable)?;
    let signature = auth::decode_hex(&notice.signature,64).map_err(|_| MarketplaceError::Unavailable)?;
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519,key).verify(&payload,&signature).map_err(|_| MarketplaceError::Unavailable)
}

impl MarketStore {
    pub fn report(&self, token: &str, id: &str, version: &str, request: &ReportRequest) -> Result<PluginReport, MarketplaceError> {
        self.write(|transaction| {
            let timestamp = auth::now()?;
            let user = session_user(transaction,token,timestamp)?;
            validate_locator(id,version)?;
            if !matches!(request.reason.as_str(), "security" | "privacy" | "malware" | "copyright" | "other") { return Err(MarketplaceError::InvalidRequest); }
            privacy::inspect_note_with_secrets(&request.note,&[token,&user.user_id])?;
            let submission_id: Option<String> = transaction.query_row("SELECT submission_id FROM releases WHERE plugin_id=?1 AND version=?2",
                params![id,version], |row| row.get(0)).optional().map_err(db_error)?;
            let stored = stored_submission(transaction,&submission_id.ok_or(MarketplaceError::NotFound)?)?;
            let report_id = Uuid::new_v4().to_string();
            transaction.execute("INSERT INTO reports(id,plugin_id,version,reporter_id,reporter_display_name,reason,note,status,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,'open',?8)",
                params![report_id,id,version,user.user_id,user.display_name,request.reason,request.note,timestamp]).map_err(db_error)?;
            record_audit(transaction,Some(&user),Some(&stored),"report_submitted",None,Some("open"),None,timestamp)?;
            Ok(PluginReport { report_id,plugin_id: id.to_owned(),version: version.to_owned(),reporter_display_name: user.display_name,
                reason: request.reason.clone(),note: request.note.clone(),status: "open".to_owned(),resolution_note: None,created_at: timestamp,resolved_at: None })
        })
    }

    pub fn reports(&self, token: &str, query: &ListQuery) -> Result<Page<PluginReport>, MarketplaceError> {
        self.read(|transaction| {
            let user = session_user(transaction,token,auth::now()?)?;
            require_reviewer(&user)?;
            let (limit,offset) = page_bounds(query.limit,query.offset)?;
            let status = query.status.as_deref().unwrap_or("open");
            if !matches!(status,"open" | "resolved" | "dismissed") { return Err(MarketplaceError::InvalidRequest); }
            let mut statement = transaction.prepare("SELECT id,plugin_id,version,reporter_display_name,reason,note,status,resolution_note,created_at,resolved_at FROM reports WHERE status=?1 ORDER BY created_at DESC,id DESC LIMIT ?2 OFFSET ?3").map_err(db_error)?;
            let mut items = statement.query_map(params![status,limit+1,offset],report_row).map_err(db_error)?.collect::<Result<Vec<_>,_>>().map_err(db_error)?;
            let has_more = items.len()>limit as usize;
            items.truncate(limit as usize);
            Ok(Page { items,limit,offset,has_more })
        })
    }

    pub fn resolve_report(&self, token: &str, report_id: &str, request: &ResolveReportRequest) -> Result<PluginReport, MarketplaceError> {
        self.write(|transaction| {
            let timestamp = auth::now()?;
            let user = session_user(transaction,token,timestamp)?;
            require_reviewer(&user)?;
            if Uuid::parse_str(report_id).is_err() { return Err(MarketplaceError::NotFound); }
            if !matches!(request.resolution.as_str(),"resolved" | "dismissed") || request.note.trim().is_empty() { return Err(MarketplaceError::InvalidRequest); }
            privacy::inspect_note_with_secrets(&request.note,&[token,&user.user_id])?;
            let record: Option<(String,String,String,String)> = transaction.query_row(
                "SELECT r.reporter_id,s.author_id,s.id,r.status FROM reports r JOIN releases v ON v.plugin_id=r.plugin_id AND v.version=r.version JOIN submissions s ON s.id=v.submission_id WHERE r.id=?1",
                [report_id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional().map_err(db_error)?;
            let (reporter,author,submission_id,status) = record.ok_or(MarketplaceError::NotFound)?;
            if reporter == user.user_id || author == user.user_id { return Err(MarketplaceError::Forbidden); }
            if status != "open" { return Err(MarketplaceError::Conflict); }
            transaction.execute("UPDATE reports SET status=?1,resolution_note=?2,resolved_by=?3,resolved_at=?4 WHERE id=?5 AND status='open'",
                params![request.resolution,request.note,user.user_id,timestamp,report_id]).map_err(db_error)?;
            let stored = stored_submission(transaction,&submission_id)?;
            record_audit(transaction,Some(&user),Some(&stored),"report_resolved",Some("open"),Some(&request.resolution),None,timestamp)?;
            transaction.query_row("SELECT id,plugin_id,version,reporter_display_name,reason,note,status,resolution_note,created_at,resolved_at FROM reports WHERE id=?1", [report_id],report_row).map_err(db_error)
        })
    }

    pub fn audit(&self, token: &str, submission_id: Option<&str>, query: &ListQuery) -> Result<Page<AuditEvent>, MarketplaceError> {
        self.read(|transaction| {
            let user = session_user(transaction,token,auth::now()?)?;
            if let Some(id) = submission_id { authorize_submission(&user,&stored_submission(transaction,id)?)?; }
            else { require_reviewer(&user)?; }
            if query.status.is_some() { return Err(MarketplaceError::InvalidRequest); }
            let (limit,offset) = page_bounds(query.limit,query.offset)?;
            let mut statement = transaction.prepare("SELECT sequence,submission_id,actor_display_name,action,from_status,to_status,version,digest,note,created_at FROM audit WHERE (?1 IS NULL OR submission_id=?1) ORDER BY sequence DESC LIMIT ?2 OFFSET ?3").map_err(db_error)?;
            let mut items = statement.query_map(params![submission_id,limit+1,offset], |row| Ok(AuditEvent {
                sequence: row.get(0)?,submission_id: row.get(1)?,actor_display_name: row.get(2)?,action: row.get(3)?,
                from_status: row.get(4)?,to_status: row.get(5)?,version: row.get(6)?,digest: row.get(7)?,note: row.get(8)?,created_at: row.get(9)?,
            })).map_err(db_error)?.collect::<Result<Vec<_>,_>>().map_err(db_error)?;
            let has_more = items.len()>limit as usize;
            items.truncate(limit as usize);
            Ok(Page { items,limit,offset,has_more })
        })
    }
}

fn report_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PluginReport> {
    Ok(PluginReport { report_id: row.get(0)?,plugin_id: row.get(1)?,version: row.get(2)?,reporter_display_name: row.get(3)?,
        reason: row.get(4)?,note: row.get(5)?,status: row.get(6)?,resolution_note: row.get(7)?,created_at: row.get(8)?,resolved_at: row.get(9)? })
}
