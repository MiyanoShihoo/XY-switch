import argparse
import contextlib
import hashlib
import itertools
import json
import os
from pathlib import Path
import sqlite3
import sys
import time


APPLICATION_ID = 0x58594D50
SCHEMA_VERSION = 1
TABLE_ORDER = {
    "users": "id",
    "sessions": "token_hash",
    "login_attempts": "username_hash",
    "signing_keys": "id",
    "plugins": "id",
    "submissions": "id",
    "releases": "plugin_id,version",
    "withdrawals": "plugin_id,version",
    "reports": "id",
    "audit": "sequence",
}
DURABLE_TABLES = tuple(table for table in TABLE_ORDER if table not in ("sessions", "login_attempts"))
REQUIRED_TRIGGERS = {
    "audit_no_update", "audit_no_delete", "submissions_no_delete", "submissions_immutable",
    "submissions_transitions", "releases_approved_insert", "releases_no_delete", "releases_immutable",
    "withdrawals_no_update", "withdrawals_no_delete", "keys_no_update", "keys_no_delete",
    "plugins_no_update", "plugins_no_delete",
}


class BackupError(Exception):
    pass


class SafeParser(argparse.ArgumentParser):
    def error(self, message):
        raise BackupError("invalid_request")


@contextlib.contextmanager
def readonly_database(path):
    path = Path(path)
    if not path.is_file() or path.is_symlink():
        raise BackupError("invalid_database")
    connection = sqlite3.connect(path.resolve().as_uri() + "?mode=ro", uri=True, timeout=5)
    try:
        connection.execute("PRAGMA query_only=ON")
        connection.execute("BEGIN")
        connection.execute("SELECT count(*) FROM sqlite_schema").fetchone()
        yield connection
    finally:
        connection.close()


def schema_fingerprint(connection):
    objects = connection.execute(
        "SELECT type,name,sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name"
    ).fetchall()
    encoded = json.dumps(objects, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def inspect_database(connection):
    if connection.execute("PRAGMA application_id").fetchone()[0] != APPLICATION_ID:
        raise BackupError("invalid_database")
    if connection.execute("PRAGMA user_version").fetchone()[0] != SCHEMA_VERSION:
        raise BackupError("unsupported_schema")
    if connection.execute("PRAGMA integrity_check").fetchall() != [("ok",)]:
        raise BackupError("invalid_database")
    if connection.execute("PRAGMA foreign_key_check").fetchone() is not None:
        raise BackupError("invalid_database")
    tables = {row[0] for row in connection.execute("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'")}
    triggers = {row[0] for row in connection.execute("SELECT name FROM sqlite_schema WHERE type='trigger'")}
    if tables != set(TABLE_ORDER) or triggers != REQUIRED_TRIGGERS:
        raise BackupError("unsupported_schema")
    for plugin_id, version, author_id, author_name, package_json, digest, status, reviewer, approved_version, approved_digest in connection.execute(
        "SELECT plugin_id,version,author_id,author_display_name,package_json,digest,status,approved_by,approved_version,approved_digest FROM submissions"
    ):
        try:
            package = json.loads(package_json)
            manifest = package["manifest"]
            matches = manifest["id"] == plugin_id and manifest["version"] == version and manifest["author"] == author_name
        except (ValueError, KeyError, TypeError):
            raise BackupError("invalid_package_state") from None
        if not matches or hashlib.sha256(package_json.encode("utf-8")).hexdigest() != digest:
            raise BackupError("invalid_package_state")
        if reviewer is not None and (reviewer == author_id or approved_version != version or approved_digest != digest):
            raise BackupError("invalid_approval_state")
        if status in ("approved", "published") and reviewer is None:
            raise BackupError("invalid_approval_state")
    mismatch = connection.execute(
        "SELECT 1 FROM releases r LEFT JOIN submissions s ON s.id=r.submission_id "
        "WHERE s.id IS NULL OR r.plugin_id<>s.plugin_id OR r.version<>s.version "
        "OR r.author_display_name<>s.author_display_name OR r.package_json<>s.package_json OR r.digest<>s.digest "
        "OR r.status<>s.status OR s.approved_version IS NOT s.version OR s.approved_digest IS NOT s.digest "
        "OR s.approved_by IS NULL OR s.approved_by=s.author_id LIMIT 1"
    ).fetchone()
    missing_release = connection.execute(
        "SELECT 1 FROM submissions s LEFT JOIN releases r ON r.submission_id=s.id WHERE s.status='published' AND r.submission_id IS NULL LIMIT 1"
    ).fetchone()
    missing_notice = connection.execute(
        "SELECT 1 FROM releases r LEFT JOIN withdrawals w ON w.plugin_id=r.plugin_id AND w.version=r.version "
        "WHERE r.status='withdrawn' AND w.plugin_id IS NULL LIMIT 1"
    ).fetchone()
    invalid_notice = connection.execute(
        "SELECT 1 FROM withdrawals w LEFT JOIN releases r ON r.plugin_id=w.plugin_id AND r.version=w.version "
        "WHERE r.plugin_id IS NULL OR r.status<>'withdrawn' OR w.digest<>r.digest LIMIT 1"
    ).fetchone()
    if mismatch or missing_release or missing_notice or invalid_notice:
        raise BackupError("invalid_release_state")
    return {
        "applicationId": APPLICATION_ID,
        "schemaVersion": SCHEMA_VERSION,
        "schemaSha256": schema_fingerprint(connection),
        "tableCounts": {table: connection.execute("SELECT count(*) FROM " + table).fetchone()[0] for table in TABLE_ORDER},
        "auditSequence": connection.execute("SELECT coalesce(max(sequence),0) FROM audit").fetchone()[0],
        "signatureVerificationPerformed": False,
    }


def file_digest(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def sync_file(path):
    with Path(path).open("r+b") as stream:
        os.fsync(stream.fileno())


def sync_directory(path):
    if os.name != "nt":
        descriptor = os.open(path, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)


def create_database_file(path):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    os.close(descriptor)


def remove_created_database(path):
    for filename in (path, Path(str(path) + "-journal"), Path(str(path) + "-wal"), Path(str(path) + "-shm")):
        filename.unlink(missing_ok=True)


def backup_database(database, output):
    output = Path(output)
    with readonly_database(database) as source:
        inspect_database(source)
        try:
            output.mkdir(mode=0o700)
        except FileExistsError:
            raise BackupError("destination_exists") from None
        target = output / "marketplace.sqlite3"
        metadata = output / "manifest.json"
        try:
            create_database_file(target)
            with contextlib.closing(sqlite3.connect(target)) as destination:
                source.backup(destination, pages=256, sleep=0.05)
                destination.execute("PRAGMA journal_mode=DELETE")
                destination.execute("PRAGMA synchronous=FULL")
                summary = inspect_database(destination)
            sync_file(target)
            manifest = {"formatVersion": 1, "createdAt": int(time.time()), "databaseSha256": file_digest(target), **summary}
            descriptor = os.open(metadata, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
                json.dump(manifest, stream, ensure_ascii=False, separators=(",", ":"))
                stream.flush()
                os.fsync(stream.fileno())
            sync_directory(output)
            sync_directory(output.parent)
            return manifest
        except BaseException:
            remove_created_database(target)
            metadata.unlink(missing_ok=True)
            output.rmdir()
            raise


@contextlib.contextmanager
def verified_snapshot(snapshot):
    snapshot = Path(snapshot)
    target = snapshot / "marketplace.sqlite3"
    metadata = snapshot / "manifest.json"
    if snapshot.is_symlink() or not snapshot.is_dir() or metadata.is_symlink() or not metadata.is_file() or metadata.stat().st_size > 16384:
        raise BackupError("invalid_snapshot")
    try:
        manifest = json.loads(metadata.read_text(encoding="utf-8"))
    except (ValueError, UnicodeError):
        raise BackupError("invalid_snapshot") from None
    if not isinstance(manifest, dict) or manifest.get("formatVersion") != 1 or manifest.get("signatureVerificationPerformed") is not False:
        raise BackupError("invalid_snapshot")
    if not target.is_file() or target.is_symlink() or file_digest(target) != manifest.get("databaseSha256"):
        raise BackupError("snapshot_digest_mismatch")
    with readonly_database(target) as source:
        summary = inspect_database(source)
        if any(manifest.get(key) != value for key, value in summary.items()):
            raise BackupError("invalid_snapshot")
        yield source, manifest


def inspect_snapshot(snapshot):
    with verified_snapshot(snapshot) as (_, manifest):
        return manifest


def compare_durable_state(snapshot, reference):
    if schema_fingerprint(snapshot) != schema_fingerprint(reference):
        raise BackupError("reference_schema_mismatch")
    sentinel = object()
    for table in DURABLE_TABLES:
        query = "SELECT * FROM " + table + " ORDER BY " + TABLE_ORDER[table]
        for candidate, current in itertools.zip_longest(snapshot.execute(query), reference.execute(query), fillvalue=sentinel):
            if candidate != current:
                raise BackupError("snapshot_loses_current_state")
    candidate = snapshot.execute("SELECT seq FROM sqlite_sequence WHERE name='audit'").fetchall()
    current = reference.execute("SELECT seq FROM sqlite_sequence WHERE name='audit'").fetchall()
    if candidate != current:
        raise BackupError("snapshot_loses_current_state")


def restore_database(snapshot, reference, output):
    snapshot = Path(snapshot)
    reference = Path(reference)
    output = Path(output)
    if output.exists() or output.is_symlink():
        raise BackupError("destination_exists")
    source_file = snapshot / "marketplace.sqlite3"
    if not reference.is_file() or reference.resolve() == source_file.resolve() or os.path.samefile(reference, source_file):
        raise BackupError("independent_reference_required")
    with verified_snapshot(snapshot) as (source, _), readonly_database(reference) as current:
        inspect_database(current)
        compare_durable_state(source, current)
        try:
            create_database_file(output)
        except FileExistsError:
            raise BackupError("destination_exists") from None
        try:
            with contextlib.closing(sqlite3.connect(output)) as destination:
                source.backup(destination, pages=256, sleep=0.05)
                destination.execute("PRAGMA journal_mode=DELETE")
                destination.execute("PRAGMA foreign_keys=ON")
                destination.execute("PRAGMA synchronous=FULL")
                invalidated = destination.execute("SELECT count(*) FROM sessions").fetchone()[0]
                with destination:
                    destination.execute("DELETE FROM sessions")
                    destination.execute("DELETE FROM login_attempts")
                    destination.executemany("INSERT INTO login_attempts VALUES(?,?,?,?)", current.execute("SELECT * FROM login_attempts"))
                summary = inspect_database(destination)
            sync_file(output)
            sync_directory(output.parent)
            return {"restoredCopyCreated": True, "sessionsInvalidated": invalidated, "nativeVerificationRequired": True, **summary}
        except BaseException:
            remove_created_database(output)
            raise


def main(arguments=None):
    try:
        parser = SafeParser(description="Prepare private SQLite marketplace backups and isolated restore copies; does not verify Ed25519 signatures.")
        commands = parser.add_subparsers(dest="command", required=True, parser_class=SafeParser)
        backup = commands.add_parser("backup")
        backup.add_argument("--database", required=True)
        backup.add_argument("--output", required=True)
        inspect = commands.add_parser("inspect")
        inspect.add_argument("--snapshot", required=True)
        restore = commands.add_parser("restore")
        restore.add_argument("--snapshot", required=True)
        restore.add_argument("--reference", required=True)
        restore.add_argument("--output", required=True)
        options = parser.parse_args(arguments)
        if options.command == "backup":
            result = backup_database(options.database, options.output)
        elif options.command == "inspect":
            result = inspect_snapshot(options.snapshot)
        else:
            result = restore_database(options.snapshot, options.reference, options.output)
        print(json.dumps(result, ensure_ascii=False, separators=(",", ":")))
        return 0
    except BackupError as error:
        print("xy-marketplace-backup: " + str(error), file=sys.stderr)
        return 1
    except (OSError, sqlite3.Error, ValueError, TypeError, OverflowError):
        print("xy-marketplace-backup: backup_unavailable", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
