import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest


MODULE_PATH = Path(__file__).with_name("sqlite_backup.py")
SPEC = importlib.util.spec_from_file_location("marketplace_backup", MODULE_PATH)
backup = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(backup)
SCHEMA_SOURCE = Path(__file__).resolve().parents[2] / "src-tauri/src/marketplace/schema.rs"
SCHEMA_SQL = SCHEMA_SOURCE.read_text(encoding="utf-8").split('r#"', 1)[1].rsplit('"#;', 1)[0]


class MarketplaceBackupTests(unittest.TestCase):
    def setUp(self):
        target = Path(__file__).with_name("verify") / "target"
        target.mkdir(exist_ok=True)
        self.target = target.resolve()
        self.temporary = tempfile.TemporaryDirectory(prefix="backup-test-", dir=self.target)
        self.directory = Path(self.temporary.name).resolve()
        self.database = self.directory / "current.sqlite3"
        self.create_database(self.database)

    def tearDown(self):
        self.assertEqual(self.directory.parent, self.target)
        self.assertTrue(self.directory.name.startswith("backup-test-"))
        self.temporary.cleanup()

    def create_database(self, path):
        with contextlib.closing(sqlite3.connect(path)) as connection:
            connection.executescript(SCHEMA_SQL)
            connection.execute("PRAGMA application_id=" + str(backup.APPLICATION_ID))
            connection.execute("PRAGMA user_version=" + str(backup.SCHEMA_VERSION))
            connection.executemany("INSERT INTO users VALUES(?,?,?,?,?,?,?,?)", [
                ("author-id", "synthetic_author", "Synthetic Author", "synthetic-test-hash", 0, 0, 1, 1),
                ("reviewer-id", "synthetic_reviewer", "Synthetic Reviewer", "synthetic-test-hash", 1, 1, 1, 1),
                ("reader-id", "synthetic_reader", "Synthetic Reader", "synthetic-test-hash", 0, 0, 1, 1),
            ])
            connection.execute("INSERT INTO sessions VALUES(?,?,?,?)", (hashlib.sha256(b"synthetic-session").digest(), "author-id", 1, 99999))
            connection.execute("INSERT INTO signing_keys VALUES(?,?)", ("synthetic-publisher", hashlib.sha256(b"synthetic-key").digest()))
            connection.execute("INSERT INTO plugins VALUES(?,?,?)", ("backup-example", "author-id", 1))
            connection.commit()
        self.add_submission(path, "1.0.0", "first-submission")

    def add_submission(self, path, version, submission_id):
        package = {
            "manifest": {"schemaVersion": 1, "id": "backup-example", "name": "Backup example", "description": "Synthetic backup fixture",
                         "author": "Synthetic Author", "version": version, "minimumHostVersion": "1.0.0", "kind": "skill", "entry": "SKILL.md",
                         "clients": ["codex-desktop"], "permissions": {"network": [], "readPaths": [], "writePaths": [], "process": False, "credentials": []}, "dependencies": []},
            "files": {"SKILL.md": "# Synthetic example\nSummarize selected text.\n"},
        }
        package_json = json.dumps(package, ensure_ascii=False, separators=(",", ":"))
        digest = hashlib.sha256(package_json.encode("utf-8")).hexdigest()
        with contextlib.closing(sqlite3.connect(path)) as connection, connection:
            connection.execute("INSERT INTO submissions(id,plugin_id,version,author_id,author_display_name,package_json,digest,status,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?)",
                               (submission_id, "backup-example", version, "author-id", "Synthetic Author", package_json, digest, "submitted", 1, 1))
            self.audit(connection, submission_id, "submitted")

    def audit(self, connection, submission_id, action):
        connection.execute("INSERT INTO audit(submission_id,actor_id,actor_display_name,action,version,digest,created_at) SELECT id,'author-id','Synthetic Author',?,version,digest,1 FROM submissions WHERE id=?",
                           (action, submission_id))

    def approve(self, path):
        with contextlib.closing(sqlite3.connect(path)) as connection, connection:
            connection.execute("UPDATE submissions SET status='approved',approved_by='reviewer-id',approved_version=version,approved_digest=digest,approved_at=2 WHERE id='first-submission'")
            self.audit(connection, "first-submission", "approved")

    def publish(self, path):
        self.approve(path)
        with contextlib.closing(sqlite3.connect(path)) as connection, connection:
            connection.execute("INSERT INTO releases SELECT id,plugin_id,version,author_display_name,package_json,digest,?,'synthetic-publisher','published',3 FROM submissions WHERE id='first-submission'", ("10" * 64,))
            connection.execute("UPDATE submissions SET status='published' WHERE id='first-submission'")
            self.audit(connection, "first-submission", "published")

    def withdraw(self, path):
        with contextlib.closing(sqlite3.connect(path)) as connection, connection:
            connection.execute("INSERT INTO withdrawals SELECT plugin_id,version,digest,'privacy',signing_key_id,?,4 FROM releases", ("20" * 64,))
            connection.execute("UPDATE releases SET status='withdrawn'")
            connection.execute("UPDATE submissions SET status='withdrawn' WHERE id='first-submission'")
            self.audit(connection, "first-submission", "withdrawn")

    def assert_rejected_without_output(self, snapshot, code="snapshot_loses_current_state"):
        output = self.directory / "rejected.sqlite3"
        with self.assertRaisesRegex(backup.BackupError, code):
            backup.restore_database(snapshot, self.database, output)
        self.assertFalse(output.exists())

    def test_online_backup_captures_wal_and_keeps_source_sessions(self):
        with contextlib.closing(sqlite3.connect(self.database)) as writer:
            self.assertEqual(writer.execute("PRAGMA journal_mode=WAL").fetchone()[0], "wal")
            writer.execute("BEGIN")
            writer.execute("SELECT count(*) FROM submissions").fetchone()
            self.add_submission(self.database, "1.1.0", "wal-submission")
            self.assertGreater(Path(str(self.database) + "-wal").stat().st_size, 0)
            snapshot = self.directory / "wal-backup"
            manifest = backup.backup_database(self.database, snapshot)
            self.assertEqual(manifest["tableCounts"]["submissions"], 2)
            self.assertEqual(manifest["tableCounts"]["sessions"], 1)
            self.assertFalse(manifest["signatureVerificationPerformed"])
            self.assertEqual(backup.inspect_snapshot(snapshot), manifest)
            self.assertEqual(writer.execute("SELECT count(*) FROM sessions").fetchone()[0], 1)
            self.assertFalse((snapshot / "marketplace.sqlite3-wal").exists())

    def test_restore_preserves_withdrawals_and_invalidates_sessions(self):
        self.publish(self.database)
        self.withdraw(self.database)
        snapshot = self.directory / "withdrawn-backup"
        manifest = backup.backup_database(self.database, snapshot)
        with contextlib.closing(sqlite3.connect(self.database)) as current, current:
            current.execute("INSERT INTO login_attempts VALUES(?,?,?,?)", (hashlib.sha256(b"synthetic-login").digest(), 5, 4, 900))
        output = self.directory / "restored.sqlite3"
        result = backup.restore_database(snapshot, self.database, output)
        self.assertTrue(result["nativeVerificationRequired"])
        self.assertFalse(result["signatureVerificationPerformed"])
        self.assertEqual(result["tableCounts"]["sessions"], 0)
        self.assertEqual(result["sessionsInvalidated"], 1)
        self.assertEqual(result["tableCounts"]["login_attempts"], 1)
        self.assertEqual(result["tableCounts"]["withdrawals"], 1)
        self.assertEqual(result["auditSequence"], manifest["auditSequence"])
        with backup.readonly_database(output) as restored, backup.readonly_database(self.database) as current:
            backup.compare_durable_state(restored, current)
            self.assertEqual(current.execute("SELECT count(*) FROM sessions").fetchone()[0], 1)
            self.assertEqual(restored.execute("SELECT status,approved_version,approved_digest=digest FROM submissions").fetchone(), ("withdrawn", "1.0.0", 1))

    def test_stale_snapshot_cannot_remove_approval(self):
        snapshot = self.directory / "before-approval"
        backup.backup_database(self.database, snapshot)
        self.approve(self.database)
        self.assert_rejected_without_output(snapshot)

    def test_stale_snapshot_cannot_resurrect_withdrawn_release(self):
        self.publish(self.database)
        snapshot = self.directory / "before-withdrawal"
        backup.backup_database(self.database, snapshot)
        self.withdraw(self.database)
        self.assert_rejected_without_output(snapshot)

    def test_stale_snapshot_cannot_lose_new_version_or_audit(self):
        snapshot = self.directory / "before-version"
        backup.backup_database(self.database, snapshot)
        self.add_submission(self.database, "1.1.0", "updated-submission")
        self.assert_rejected_without_output(snapshot)

    def test_stale_snapshot_cannot_restore_previous_identity_permissions(self):
        snapshot = self.directory / "before-account-change"
        backup.backup_database(self.database, snapshot)
        with contextlib.closing(sqlite3.connect(self.database)) as current, current:
            current.execute("UPDATE users SET active=0 WHERE id='reader-id'")
        self.assert_rejected_without_output(snapshot)

    def test_reference_and_existing_destination_are_protected(self):
        snapshot = self.directory / "reference-backup"
        backup.backup_database(self.database, snapshot)
        output = self.directory / "existing.sqlite3"
        output.write_bytes(b"synthetic protected output")
        with self.assertRaisesRegex(backup.BackupError, "destination_exists"):
            backup.restore_database(snapshot, self.database, output)
        self.assertEqual(output.read_bytes(), b"synthetic protected output")
        candidate = self.directory / "candidate.sqlite3"
        with self.assertRaisesRegex(backup.BackupError, "independent_reference_required"):
            backup.restore_database(snapshot, snapshot / "marketplace.sqlite3", candidate)
        with self.assertRaisesRegex(backup.BackupError, "independent_reference_required"):
            backup.restore_database(snapshot, self.directory / "missing.sqlite3", candidate)
        self.assertFalse(candidate.exists())

    def test_backup_refuses_overwrite_foreign_database_and_corrupt_metadata(self):
        snapshot = self.directory / "protected-backup"
        manifest = backup.backup_database(self.database, snapshot)
        with self.assertRaisesRegex(backup.BackupError, "destination_exists"):
            backup.backup_database(self.database, snapshot)
        self.assertEqual(backup.inspect_snapshot(snapshot), manifest)
        foreign = self.directory / "foreign.sqlite3"
        with contextlib.closing(sqlite3.connect(foreign)) as connection:
            connection.execute("CREATE TABLE unrelated(value TEXT)")
        with self.assertRaisesRegex(backup.BackupError, "invalid_database"):
            backup.backup_database(foreign, self.directory / "foreign-backup")
        sentinel = "synthetic-secret-never-echoed"
        (snapshot / "manifest.json").write_text(sentinel, encoding="utf-8")
        output = io.StringIO()
        errors = io.StringIO()
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
            code = backup.main(["inspect", "--snapshot", str(snapshot)])
        self.assertEqual(code, 1)
        self.assertEqual(output.getvalue(), "")
        self.assertEqual(errors.getvalue(), "xy-marketplace-backup: invalid_snapshot\n")
        self.assertNotIn(sentinel, errors.getvalue())

    def test_backup_bytes_and_missing_immutable_trigger_are_rejected(self):
        snapshot = self.directory / "tampered-backup"
        backup.backup_database(self.database, snapshot)
        target = snapshot / "marketplace.sqlite3"
        with target.open("ab") as stream:
            stream.write(b"synthetic tampered bytes")
        with self.assertRaisesRegex(backup.BackupError, "snapshot_digest_mismatch"):
            backup.inspect_snapshot(snapshot)
        with contextlib.closing(sqlite3.connect(self.database)) as current, current:
            current.execute("DROP TRIGGER audit_no_update")
        with self.assertRaisesRegex(backup.BackupError, "unsupported_schema"):
            backup.backup_database(self.database, self.directory / "missing-trigger-backup")


if __name__ == "__main__":
    unittest.main()
