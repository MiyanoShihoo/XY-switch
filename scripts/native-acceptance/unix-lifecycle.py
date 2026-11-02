import hashlib
import json
import os
import platform
import re
import secrets
import signal
import socket
import sqlite3
import subprocess
import sys
import threading
import time
import traceback
import urllib.error
import urllib.request
from contextlib import closing, contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from unix_gui import NativeGui, contained_path, file_sha256, phrase_boxes, require_runner
from unix_packages import CredentialVault, Installation


def wait_for(predicate, message, timeout=30):
    deadline = time.monotonic() + timeout
    last_error = None
    while time.monotonic() < deadline:
        try:
            if predicate():
                return
        except (OSError, sqlite3.Error, ValueError) as error:
            last_error = error
        time.sleep(0.15)
    raise RuntimeError(message + (f": {last_error}" if last_error else ""))


def listener_running(port):
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=0.3):
            return True
    except OSError:
        return False


def free_port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


class SyntheticUpstream:
    def __init__(self, api_key, model):
        self.requests = []
        self.errors = []
        owner = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_arguments):
                pass

            def do_POST(self):
                try:
                    length = int(self.headers.get("Content-Length", "0"))
                    if not 0 < length < 1024 * 1024 or self.path != "/v1/chat/completions":
                        raise RuntimeError("Unexpected synthetic upstream request path or size")
                    matched = self.headers.get("Authorization") == "Bearer " + api_key
                    if not matched:
                        raise RuntimeError("Application did not read the synthetic native API credential")
                    body = json.loads(self.rfile.read(length))
                    if body.get("model") != model:
                        raise RuntimeError("Application forwarded a different upstream model")
                    owner.requests.append({"path": self.path, "model": body["model"], "authorizationMatched": matched})
                    payload = {"id": "native-upstream", "object": "chat.completion", "model": model,
                               "choices": [{"index": 0, "message": {"role": "assistant", "content": "NATIVE_CREDENTIAL_OK"}, "finish_reason": "stop"}],
                               "usage": {"prompt_tokens": 4, "completion_tokens": 3, "total_tokens": 7}}
                    data = json.dumps(payload).encode()
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(data)))
                    self.end_headers()
                    self.wfile.write(data)
                except Exception as error:
                    owner.errors.append(str(error))
                    self.send_response(500)
                    self.end_headers()
                    self.wfile.write(b'{"error":"Synthetic upstream validation failed"}')

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.endpoint = f"http://127.0.0.1:{self.server.server_port}/v1"

    def stop(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)
        if self.thread.is_alive():
            raise RuntimeError("Synthetic upstream did not stop")


class Lifecycle:
    def __init__(self, kind, input_path, previous_path, output, temporary):
        require_runner(kind)
        self.kind = kind
        self.temporary = contained_path(os.environ["RUNNER_TEMP"], temporary)
        if Path(os.environ["HOME"]).resolve() != self.temporary / "home" or Path(os.environ["CODEX_HOME"]).resolve() != self.temporary / "codex":
            raise RuntimeError("Native acceptance HOME and CODEX_HOME were not isolated")
        self.output = Path(output).resolve(strict=True)
        self.input = json.loads(Path(input_path).read_text())
        self.previous = json.loads((Path(previous_path) / "previous-release.json").read_text(encoding="utf-8-sig"))
        if self.previous.get("verified") is not True or self.previous.get("version") != "1.0.0" or self.previous.get("platform") != kind:
            raise RuntimeError("Previous official release evidence is invalid")
        if self.previous.get("commit") != "1f0c8c197b9aab301bb3ac39683c1e5871e2045e" or self.previous.get("tag") != "v1.0.0":
            raise RuntimeError("Previous official release source was not bound")
        expected_platform = {"linux": "linux-x64", "macos": "macos-universal"}[kind]
        current_version = json.loads((Path(__file__).resolve().parents[2] / "package.json").read_text())["version"]
        if self.input.get("platform") != expected_platform or self.input.get("version") != current_version:
            raise RuntimeError("Current native package platform or version is invalid")
        for bundle in [self.input, self.previous]:
            for package in bundle["files"]:
                size = package.get("bytes")
                if not isinstance(size, int) or isinstance(size, bool) or size <= 0 or Path(package["path"]).stat().st_size != size:
                    raise RuntimeError("Package size changed before lifecycle verification")
                if not re.fullmatch(r"[0-9a-f]{64}", package["sha256"]) or file_sha256(package["path"]) != package["sha256"]:
                    raise RuntimeError("Package checksum changed before lifecycle verification")
        for variable in ["NATIVE_SOURCE_COMMIT", "GITHUB_SHA"]:
            if not re.fullmatch(r"[0-9a-f]{40}", os.environ.get(variable, "")):
                raise RuntimeError("Native lifecycle requires a bound source and harness commit")
        if not re.fullmatch(r"[1-9][0-9]*", os.environ.get("NATIVE_SOURCE_RUN", "")):
            raise RuntimeError("Native lifecycle source run was not bound")
        data_root = Path(os.environ["XDG_DATA_HOME"]) if kind == "linux" else self.temporary / "home/Library/Application Support"
        self.app_data = contained_path(self.temporary, data_root / "com.xyswitch.desktop")
        if self.app_data.exists():
            raise RuntimeError("Refusing pre-existing lifecycle application data")
        self.database = self.app_data / "xy-switch.db"
        self.codex = self.temporary / "codex"
        self.original_config = b'model = "original-model"\n# Synthetic lifecycle original\n'
        self.original_auth = b'{"tokens":{"fixture":"original"}}\n'
        self.results = []
        self.secret_values = set()
        self.vault = CredentialVault(kind, self.temporary, self.secret_values)
        self.installation = None
        self.application = None
        self.gui = None
        self.guis = []
        self.upstream = None
        self.records = []
        self.active = "setup"
        self.port = None
        self.failed = False
        self.report = {"passed": False, "platform": kind, "operatingSystem": platform.platform(),
                       "runnerArchitecture": platform.machine(), "sourceCommit": os.environ["NATIVE_SOURCE_COMMIT"],
                       "harnessCommit": os.environ["GITHUB_SHA"], "sourceRun": os.environ["NATIVE_SOURCE_RUN"],
                       "previousCommit": self.previous["commit"], "previousVersion": self.previous["version"],
                       "currentVersion": self.input["version"], "apiCallsCharged": 0,
                       "scope": "Real disposable CI installation, upgrade, uninstall and reinstall; screenshots and OS pointer clicks in the verified native GUI; native credential read via production loopback routing; forced termination and cold-start restoration. Provider/prompt fixtures and SQLite backup restoration are prepared offline, not GUI import. macOS covers the CI runner architecture, not both Universal slices executing."}

    def scrub(self, value):
        text = str(value)
        for secret in self.secret_values:
            text = text.replace(secret, "[redacted]")
        return text

    def step(self, name, action):
        self.active = name
        print("RUN " + name, flush=True)
        started = time.monotonic()
        try:
            detail = action()
            result = {"name": name, "passed": True, "elapsedMs": round((time.monotonic() - started) * 1000)}
            if detail is not None:
                result["evidence"] = detail
            self.results.append(result)
            print("PASS " + name, flush=True)
        except Exception:
            self.failed = True
            self.results.append({"name": name, "passed": False, "error": self.scrub(traceback.format_exc())})
            raise

    @contextmanager
    def connection(self, writable=False):
        connection = sqlite3.connect(self.database if writable else f"file:{self.database.as_posix()}?mode=ro", uri=not writable, timeout=10)
        connection.row_factory = sqlite3.Row
        connection.execute("PRAGMA foreign_keys=ON")
        try:
            with connection:
                yield connection
        finally:
            connection.close()

    def setting(self, key):
        with self.connection() as connection:
            row = connection.execute("SELECT value FROM app_settings WHERE key=?", (key,)).fetchone()
            return row[0] if row else None

    def verify_fixture(self):
        with self.connection() as connection:
            if connection.execute("PRAGMA integrity_check").fetchone()[0] != "ok":
                raise RuntimeError("Lifecycle configuration database integrity failed")
            row = connection.execute("SELECT * FROM providers WHERE id=?", (self.provider["id"],)).fetchone()
            prompt = connection.execute("SELECT * FROM prompts WHERE id=?", (self.prompt["id"],)).fetchone()
            if row is None or prompt is None:
                raise RuntimeError("Upgrade or restoration lost the provider or prompt fixture")
            for key in ["name", "endpoint", "model", "enabled"]:
                if row[key] != self.provider[key]:
                    raise RuntimeError("Lifecycle provider field was not preserved: " + key)
            if prompt["content"] != self.prompt["content"]:
                raise RuntimeError("Lifecycle prompt content was not preserved")
        return {"providerPreserved": True, "promptPreserved": True, "databaseIntegrity": "ok"}

    def prepare_fixture(self):
        with self.connection(True) as connection:
            connection.execute("INSERT INTO providers(id,name,provider_type,endpoint,model,app_group,enabled,sort_order,advanced_json) VALUES(?,?,?,?,?,?,?,?,?)",
                               (self.provider["id"], self.provider["name"], "custom", self.provider["endpoint"], self.provider["model"], "Codex", 1, 0, json.dumps({"upstreamFormat": "openai_chat"})))
            connection.execute("INSERT INTO prompts(id,name,content,description,enabled) VALUES(?,?,?,?,?)",
                               (self.prompt["id"], "Native lifecycle prompt", self.prompt["content"], "Disposable CI fixture", 1))
            for key, value in [("current_provider", self.provider["id"]), ("interface_language", "en"), ("interface_language_resolved", "en"),
                               ("proxy_settings", json.dumps({"enabled": False, "showInHome": True, "host": "127.0.0.1", "port": self.port, "routeTargets": ["codex"]}))]:
                connection.execute("INSERT INTO app_settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value", (key, value))
        return self.verify_fixture()

    def launch(self, label):
        log_path = contained_path(self.temporary, self.temporary / ("application-" + label + ".log"))
        with log_path.open("wb") as log:
            self.application = subprocess.Popen([str(self.installation.executable)], stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT,
                                                cwd=self.installation.executable.parent, env=dict(os.environ), start_new_session=True)
        self.gui = NativeGui(self.kind, self.temporary, self.output, self.application)
        self.guis.append(self.gui)
        self.gui.ready(label)
        wait_for(lambda: self.database.is_file(), "Native application did not create the isolated database")
        return {"pid": self.application.pid, "executableSha256": file_sha256(self.installation.executable), "isolatedDatabase": True}

    def stop(self, force=False):
        if self.application is not None:
            process = self.application
            try:
                os.killpg(process.pid, signal.SIGKILL if force else signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=5)
            self.application = None
            self.gui = None
        if self.port is not None:
            wait_for(lambda: not listener_running(self.port), "Application routing listener survived termination", timeout=10)

    def restored(self):
        if (self.codex / "config.toml").read_bytes() != self.original_config or (self.codex / "auth.json").read_bytes() != self.original_auth:
            return False
        if (self.codex / "xy-switch-model-catalog.json").exists():
            return False
        return not any((self.app_data / "takeover-backups" / (name + suffix)).exists()
                       for name in ["codex-config", "codex-auth", "codex-model-catalog"] for suffix in [".json", ".bin"])

    def taken_over(self):
        for name, target, original in [("codex-config", "config.toml", self.original_config), ("codex-auth", "auth.json", self.original_auth),
                                       ("codex-model-catalog", "xy-switch-model-catalog.json", None)]:
            path = self.codex / target
            metadata_path = self.app_data / "takeover-backups" / (name + ".json")
            if not path.is_file() or not metadata_path.is_file():
                return False
            metadata = json.loads(metadata_path.read_text())
            if Path(metadata["targetPath"]).resolve() != path.resolve() or metadata["managedSha256"] != file_sha256(path):
                return False
            if metadata["existed"] != (original is not None):
                return False
            if original is not None and (self.app_data / "takeover-backups" / (name + ".bin")).read_bytes() != original:
                return False
        return (self.codex / "config.toml").read_bytes() != self.original_config and (self.codex / "auth.json").read_bytes() != self.original_auth

    def forwarded(self):
        token = json.loads((self.codex / "auth.json").read_text())["OPENAI_API_KEY"]
        if not isinstance(token, str) or not token:
            raise RuntimeError("Managed Codex routing token is missing")
        self.secret_values.add(token)
        before = len(self.upstream.requests)
        request = urllib.request.Request(f"http://127.0.0.1:{self.port}/v1/responses", method="POST",
                                         headers={"Authorization": "Bearer " + token, "Content-Type": "application/json"},
                                         data=json.dumps({"model": self.provider["model"], "input": "Synthetic native credential check", "stream": False}).encode())
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                body = response.read()
                if response.status != 200 or b"NATIVE_CREDENTIAL_OK" not in body:
                    raise RuntimeError("Application routing returned an unexpected response")
        except urllib.error.HTTPError as error:
            raise RuntimeError("Application routing HTTP failure: " + str(error.code) + " " + self.scrub(error.read().decode(errors="replace"))) from error
        if self.upstream.errors or len(self.upstream.requests) != before + 1:
            raise RuntimeError("Native upstream evidence is invalid: " + str(self.upstream.errors))
        return {"responseSha256": hashlib.sha256(body).hexdigest(), "upstream": self.upstream.requests[-1], "chargedApiCalls": 0}

    def fixture_visible(self, label):
        self.verify_fixture()
        _info, image, _width, _height, words = self.gui.capture(label)
        if not phrase_boxes(words, self.provider["name"]):
            raise RuntimeError("Restored provider fixture is not visible in the real native window")
        return {"fixtureVisible": True, "screenshot": image.name}

    def start_routing(self):
        self.gui.routing_toggle(False)
        wait_for(lambda: listener_running(self.port), "Native GUI did not start the routing listener")
        if not json.loads(self.setting("proxy_settings"))["enabled"]:
            raise RuntimeError("Native GUI did not persist the enabled routing setting")
        return {"listeningOnLoopback": True, "port": self.port}

    def connect_codex(self):
        self.gui.takeover_toggle(False)
        wait_for(self.taken_over, "Native GUI did not produce valid Codex takeover files and backups")
        return {"managedFiles": 3, "originalBackupBytesMatch": True}

    def restore_codex(self):
        self.gui.takeover_toggle(True)
        wait_for(self.restored, "Native GUI did not restore the original Codex bytes")
        if not listener_running(self.port):
            raise RuntimeError("Restoring Codex unexpectedly stopped routing")
        return {"originalConfigurationSha256": hashlib.sha256(self.original_config).hexdigest(),
                "originalAuthSha256": hashlib.sha256(self.original_auth).hexdigest(), "backupMetadataRemoved": True}

    def stop_routing(self):
        self.gui.routing_toggle(True)
        wait_for(self.restored, "Stopping routing did not restore the original Codex files")
        wait_for(lambda: not listener_running(self.port), "Stopping routing retained the listener")
        if json.loads(self.setting("proxy_settings"))["enabled"]:
            raise RuntimeError("Stopping routing retained the enabled preference")
        return {"originalBytesRestored": True, "backupMetadataRemoved": True, "listenerStopped": True}

    def cold_start(self, package_format):
        self.start_routing()
        self.connect_codex()
        self.stop(force=True)
        launched = self.launch(package_format + "-cold-start")
        wait_for(self.restored, "Cold startup did not recover the forced-termination takeover")
        if listener_running(self.port) or json.loads(self.setting("proxy_settings"))["enabled"]:
            raise RuntimeError("Cold startup automatically restarted routing")
        self.verify_fixture()
        launched.update({"forcedTermination": True, "originalBytesRestored": True, "listenerStopped": True, "enabledPreference": False})
        return launched

    def cold_credential(self):
        self.gui.routing()
        self.start_routing()
        self.connect_codex()
        evidence = self.forwarded()
        self.stop_routing()
        return evidence

    def backup_configuration(self, package_format):
        self.stop()
        backup = self.output / (package_format + "-configuration-backup.sqlite3")
        with self.connection() as connection, closing(sqlite3.connect(backup)) as destination:
            connection.backup(destination)
        data = backup.read_bytes()
        if any(secret.encode() in data for secret in self.secret_values):
            raise RuntimeError("Native credential appeared in the SQLite configuration backup")
        return {"backup": backup.name, "sha256": file_sha256(backup), "containsNativeCredentials": False}

    def uninstall_preserving_configuration(self):
        self.installation.uninstall()
        evidence = self.verify_fixture()
        if not self.restored():
            raise RuntimeError("Uninstall changed the restored CLI configuration")
        evidence.update({"deployedExecutableRemoved": True, "appDataPreserved": True})
        return evidence

    def restore_configuration(self, package_format):
        self.stop()
        with self.connection(True) as connection:
            connection.execute("DELETE FROM providers WHERE id=?", (self.provider["id"],))
            connection.execute("DELETE FROM prompts WHERE id=?", (self.prompt["id"],))
        with self.connection() as connection:
            if connection.execute("SELECT COUNT(*) FROM providers WHERE id=?", (self.provider["id"],)).fetchone()[0]:
                raise RuntimeError("Offline restore fixture was not removed")
        backup = self.output / (package_format + "-configuration-backup.sqlite3")
        with closing(sqlite3.connect(f"file:{backup.as_posix()}?mode=ro", uri=True)) as source, self.connection(True) as destination:
            source.backup(destination)
        self.verify_fixture()
        self.launch(package_format + "-backup-restored")
        if not self.restored():
            raise RuntimeError("Reinstallation and backup restore changed the original CLI bytes")
        return self.fixture_visible(package_format + "-restored-provider-visible")

    def scenario(self, package_format):
        self.installation = Installation(self.kind, package_format, self.temporary, self.output)
        suffix = {"deb": ".deb", "appimage": ".AppImage", "dmg": ".dmg"}[package_format]
        old = next(package for package in self.previous["files"] if package["name"].endswith(suffix))
        fresh = next(package for package in self.input["files"] if package["format"] == package_format)
        self.port = free_port()
        api_key = secrets.token_urlsafe(32)
        self.secret_values.add(api_key)
        identifier = "lifecycle-" + package_format + "-" + secrets.token_hex(6)
        self.provider = {"id": identifier, "name": "Native lifecycle " + package_format, "model": "native-lifecycle-model", "enabled": 1}
        self.prompt = {"id": identifier + "-prompt", "content": "Synthetic installation upgrade recovery fixture"}
        self.upstream = SyntheticUpstream(api_key, self.provider["model"])
        self.provider["endpoint"] = self.upstream.endpoint
        (self.codex / "config.toml").write_bytes(self.original_config)
        (self.codex / "auth.json").write_bytes(self.original_auth)
        self.step(package_format + "-official-old-install", lambda: self.installation.install(old, self.previous["version"]))
        self.step(package_format + "-old-native-gui", lambda: self.launch(package_format + "-v1.0.0"))
        self.stop()
        self.step(package_format + "-offline-old-configuration-fixture", self.prepare_fixture)
        self.step(package_format + "-actual-upgrade", lambda: self.installation.install(fresh, self.input["version"]))
        self.step(package_format + "-upgraded-native-gui", lambda: self.launch(package_format + "-v" + self.input["version"]))
        self.step(package_format + "-upgraded-configuration-preserved", lambda: self.fixture_visible(package_format + "-upgraded-provider-visible"))
        self.step(package_format + "-native-credential-fixture", lambda: self.vault.set(identifier, api_key, self.installation.executable))
        self.gui.routing()
        self.step(package_format + "-native-gui-start-routing", self.start_routing)
        self.step(package_format + "-native-gui-connect-codex", self.connect_codex)
        self.step(package_format + "-production-routing-native-credential-read", self.forwarded)
        self.step(package_format + "-native-gui-restore-codex", self.restore_codex)
        self.connect_codex()
        self.step(package_format + "-native-gui-stop-and-restore", self.stop_routing)
        self.step(package_format + "-forced-termination-cold-start", lambda: self.cold_start(package_format))
        self.step(package_format + "-cold-start-production-credential-read", self.cold_credential)
        self.step(package_format + "-offline-configuration-backup", lambda: self.backup_configuration(package_format))
        self.step(package_format + "-actual-uninstall-config-preservation", self.uninstall_preserving_configuration)
        self.step(package_format + "-actual-reinstall", lambda: self.installation.install(fresh, self.input["version"]))
        self.step(package_format + "-reinstalled-native-gui", lambda: self.launch(package_format + "-reinstalled"))
        self.step(package_format + "-reinstalled-config-preserved", self.verify_fixture)
        self.step(package_format + "-offline-backup-restore-and-native-gui", lambda: self.restore_configuration(package_format))
        self.stop()
        self.step(package_format + "-final-uninstall", self.uninstall_preserving_configuration)
        self.vault.clear(identifier)
        self.records.extend(self.installation.records)
        self.installation = None
        self.upstream.stop()
        self.upstream = None
        archived = contained_path(self.temporary, self.temporary / (package_format + "-app-data"))
        self.app_data.rename(archived)
        self.port = None

    def run(self):
        formats = ["deb", "appimage"] if self.kind == "linux" else ["dmg"]
        try:
            self.step("isolated-native-credential-store", self.vault.start)
            for package_format in formats:
                self.scenario(package_format)
        except Exception:
            if not self.failed:
                self.results.append({"name": self.active, "passed": False, "error": self.scrub(traceback.format_exc())})
            self.failed = True
        finally:
            for name, action in [("process-cleanup", self.stop),
                                 ("upstream-cleanup", lambda: self.upstream.stop() if self.upstream else None),
                                 ("installer-cleanup", lambda: self.installation.uninstall() if self.installation else None),
                                 ("credential-store-restoration", self.vault.restore)]:
                try:
                    action()
                except Exception:
                    self.failed = True
                    self.results.append({"name": name, "passed": False, "error": self.scrub(traceback.format_exc())})
            if self.installation:
                self.records.extend(self.installation.records)
            for log in self.temporary.glob("application-*.log"):
                (self.output / log.name).write_text(self.scrub(log.read_text(errors="replace")))
            self.report.update({"passed": not self.failed and self.vault.restored, "results": self.results, "installations": self.records,
                                "credentialStoreRestored": self.vault.restored, "clicks": [click for gui in self.guis for click in gui.clicks],
                                "macosPermissions": [gui.permissions for gui in self.guis if gui.permissions is not None]})
            (self.output / "lifecycle-results.json").write_text(self.scrub(json.dumps(self.report, indent=2)) + "\n")
        return 0 if self.report["passed"] else 1


if __name__ == "__main__":
    if len(sys.argv) != 6:
        raise SystemExit("Specify platform, package input, previous directory, output and temporary directory")
    require_runner(sys.argv[1])
    raise SystemExit(Lifecycle(*sys.argv[1:]).run())
