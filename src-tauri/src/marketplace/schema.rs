pub const APPLICATION_ID: i32 = 0x5859_4d50;
pub const SCHEMA_VERSION: i32 = 1;

pub const SQL: &str = r#"
CREATE TABLE users (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    is_reviewer INTEGER NOT NULL CHECK (is_reviewer IN (0,1)),
    provisioned_reviewer INTEGER NOT NULL CHECK (provisioned_reviewer IN (0,1)),
    active INTEGER NOT NULL CHECK (active IN (0,1)),
    created_at INTEGER NOT NULL,
    CHECK (is_reviewer <= provisioned_reviewer)
) STRICT;
CREATE TABLE sessions (
    token_hash BLOB PRIMARY KEY CHECK (length(token_hash)=32),
    user_id TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL CHECK (expires_at>created_at)
) STRICT;
CREATE INDEX sessions_user ON sessions(user_id,created_at);
CREATE INDEX sessions_expiry ON sessions(expires_at);
CREATE TABLE login_attempts (
    username_hash BLOB PRIMARY KEY CHECK (length(username_hash)=32),
    window_start INTEGER NOT NULL,
    failures INTEGER NOT NULL CHECK (failures>=0),
    blocked_until INTEGER NOT NULL
) STRICT;
CREATE TABLE signing_keys (
    id TEXT PRIMARY KEY,
    public_key BLOB NOT NULL CHECK (length(public_key)=32)
) STRICT;
CREATE TABLE plugins (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL
) STRICT;
CREATE TABLE submissions (
    id TEXT PRIMARY KEY,
    plugin_id TEXT NOT NULL REFERENCES plugins(id),
    version TEXT NOT NULL,
    author_id TEXT NOT NULL REFERENCES users(id),
    author_display_name TEXT NOT NULL,
    package_json TEXT NOT NULL,
    digest TEXT NOT NULL CHECK (length(digest)=64),
    status TEXT NOT NULL CHECK (status IN ('submitted','changes_requested','rejected','approved','published','withdrawn')),
    replaces_id TEXT REFERENCES submissions(id),
    approved_by TEXT REFERENCES users(id),
    approved_version TEXT,
    approved_digest TEXT,
    approved_at INTEGER,
    review_note TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(plugin_id,version),
    CHECK (approved_by IS NULL OR approved_by<>author_id),
    CHECK (status NOT IN ('approved','published') OR
        (approved_by IS NOT NULL AND approved_version IS version AND approved_digest IS digest AND approved_at IS NOT NULL))
) STRICT;
CREATE INDEX submissions_owner ON submissions(author_id,created_at,id);
CREATE INDEX submissions_queue ON submissions(status,created_at,id);
CREATE TABLE releases (
    submission_id TEXT NOT NULL UNIQUE REFERENCES submissions(id),
    plugin_id TEXT NOT NULL REFERENCES plugins(id),
    version TEXT NOT NULL,
    author_display_name TEXT NOT NULL,
    package_json TEXT NOT NULL,
    digest TEXT NOT NULL CHECK (length(digest)=64),
    signature TEXT NOT NULL CHECK (length(signature)=128),
    signing_key_id TEXT NOT NULL REFERENCES signing_keys(id),
    status TEXT NOT NULL CHECK (status IN ('published','withdrawn')),
    published_at INTEGER NOT NULL,
    PRIMARY KEY(plugin_id,version)
) STRICT;
CREATE INDEX releases_catalog ON releases(status,published_at,submission_id);
CREATE TRIGGER releases_approved_insert BEFORE INSERT ON releases
WHEN NOT EXISTS(SELECT 1 FROM submissions s WHERE s.id=NEW.submission_id AND s.plugin_id=NEW.plugin_id
    AND s.version=NEW.version AND s.author_display_name=NEW.author_display_name AND s.package_json=NEW.package_json
    AND s.digest=NEW.digest AND s.status='approved' AND s.approved_digest=s.digest AND s.approved_version=s.version
    AND s.approved_by IS NOT NULL AND s.approved_by<>s.author_id)
BEGIN SELECT RAISE(ABORT,'approval'); END;
CREATE TABLE withdrawals (
    plugin_id TEXT NOT NULL,
    version TEXT NOT NULL,
    digest TEXT NOT NULL,
    reason TEXT NOT NULL CHECK (reason IN ('security','privacy','withdrawn')),
    signing_key_id TEXT NOT NULL REFERENCES signing_keys(id),
    signature TEXT NOT NULL CHECK (length(signature)=128),
    created_at INTEGER NOT NULL,
    PRIMARY KEY(plugin_id,version),
    FOREIGN KEY(plugin_id,version) REFERENCES releases(plugin_id,version)
) STRICT;
CREATE TABLE reports (
    id TEXT PRIMARY KEY,
    plugin_id TEXT NOT NULL,
    version TEXT NOT NULL,
    reporter_id TEXT NOT NULL REFERENCES users(id),
    reporter_display_name TEXT NOT NULL,
    reason TEXT NOT NULL CHECK (reason IN ('security','privacy','malware','copyright','other')),
    note TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('open','resolved','dismissed')),
    resolution_note TEXT,
    resolved_by TEXT REFERENCES users(id),
    created_at INTEGER NOT NULL,
    resolved_at INTEGER,
    UNIQUE(plugin_id,version,reporter_id),
    FOREIGN KEY(plugin_id,version) REFERENCES releases(plugin_id,version)
) STRICT;
CREATE TABLE audit (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    submission_id TEXT REFERENCES submissions(id),
    actor_id TEXT REFERENCES users(id),
    actor_display_name TEXT NOT NULL,
    action TEXT NOT NULL,
    from_status TEXT,
    to_status TEXT,
    version TEXT,
    digest TEXT,
    note TEXT,
    created_at INTEGER NOT NULL
) STRICT;
CREATE INDEX audit_submission ON audit(submission_id,sequence);
CREATE TRIGGER audit_no_update BEFORE UPDATE ON audit BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER audit_no_delete BEFORE DELETE ON audit BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER submissions_no_delete BEFORE DELETE ON submissions BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER submissions_immutable BEFORE UPDATE ON submissions
WHEN NEW.id<>OLD.id OR NEW.plugin_id<>OLD.plugin_id OR NEW.version<>OLD.version OR NEW.author_id<>OLD.author_id
    OR NEW.author_display_name<>OLD.author_display_name OR NEW.package_json<>OLD.package_json OR NEW.digest<>OLD.digest
    OR NEW.replaces_id IS NOT OLD.replaces_id OR NEW.created_at<>OLD.created_at
    OR (OLD.approved_by IS NOT NULL AND
        (NEW.approved_by IS NOT OLD.approved_by OR NEW.approved_digest IS NOT OLD.approved_digest
         OR NEW.approved_version IS NOT OLD.approved_version OR NEW.approved_at IS NOT OLD.approved_at))
BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER submissions_transitions BEFORE UPDATE ON submissions
WHEN NOT ((OLD.status='submitted' AND NEW.status IN ('changes_requested','rejected','approved','withdrawn'))
    OR (OLD.status='approved' AND NEW.status IN ('published','withdrawn'))
    OR (OLD.status IN ('changes_requested','rejected','published') AND NEW.status='withdrawn'))
BEGIN SELECT RAISE(ABORT,'transition'); END;
CREATE TRIGGER releases_no_delete BEFORE DELETE ON releases BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER releases_immutable BEFORE UPDATE ON releases
WHEN NEW.submission_id<>OLD.submission_id OR NEW.plugin_id<>OLD.plugin_id OR NEW.version<>OLD.version
    OR NEW.author_display_name<>OLD.author_display_name OR NEW.package_json<>OLD.package_json OR NEW.digest<>OLD.digest
    OR NEW.signature<>OLD.signature OR NEW.signing_key_id<>OLD.signing_key_id OR NEW.published_at<>OLD.published_at
    OR OLD.status<>'published' OR NEW.status<>'withdrawn'
BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER withdrawals_no_update BEFORE UPDATE ON withdrawals BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER withdrawals_no_delete BEFORE DELETE ON withdrawals BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER keys_no_update BEFORE UPDATE ON signing_keys BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER keys_no_delete BEFORE DELETE ON signing_keys BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER plugins_no_update BEFORE UPDATE ON plugins BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER plugins_no_delete BEFORE DELETE ON plugins BEGIN SELECT RAISE(ABORT,'immutable'); END;
"#;
