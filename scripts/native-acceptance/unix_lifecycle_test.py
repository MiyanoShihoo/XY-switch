import importlib.util
import json
import subprocess
import tempfile
import unittest
import urllib.error
import urllib.request
from pathlib import Path
from unittest.mock import patch

from unix_gui import (AmbiguousControl, MissingControl, Word, contained_path,
                      descendants, phrase_boxes, read_words, require_runner, select_control)
from unix_packages import CredentialVault, Installation

spec = importlib.util.spec_from_file_location("unix_lifecycle", Path(__file__).with_name("unix-lifecycle.py"))
lifecycle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lifecycle)


def word(text, left, top, line=1):
    return Word(text, left, top, len(text) * 8, 16, (1, 1, 1, line))


class RunnerAndPathTests(unittest.TestCase):
    def test_runner_requires_hosted_matching_platform(self):
        for environment in [{}, {"GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "self-hosted"}]:
            with self.assertRaises(RuntimeError):
                require_runner("linux", environment, "Linux")
        environment = {"GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "github-hosted"}
        require_runner("linux", environment, "Linux")
        require_runner("macos", environment, "Darwin")
        for kind, system in [("linux", "Darwin"), ("macos", "Linux"), ("unknown", "Windows")]:
            with self.assertRaises(RuntimeError):
                require_runner(kind, environment, system)

    def test_paths_cannot_equal_base_or_escape(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            self.assertEqual(contained_path(base, base / "nested" / "file"), base.resolve() / "nested" / "file")
            for target in [base, base / ".." / "outside", base / "nested" / ".."]:
                with self.assertRaises(RuntimeError):
                    contained_path(base, target)

    def test_installation_guards_precede_side_effects(self):
        with patch("unix_packages.require_runner", side_effect=RuntimeError("not hosted")), patch.object(Path, "mkdir") as mkdir:
            with self.assertRaisesRegex(RuntimeError, "not hosted"):
                Installation("linux", "deb", "missing", "missing")
            mkdir.assert_not_called()


class OcrTests(unittest.TestCase):
    def test_phrase_requires_same_ocr_line(self):
        words = [word("Start", 400, 100, 1), word("routing", 450, 150, 2)]
        self.assertEqual(phrase_boxes(words, "Start routing"), [])

    def test_control_requires_unique_in_bounds_match(self):
        words = [word("Start", 400, 100), word("routing", 450, 100)]
        self.assertEqual(select_control(words, "Start routing", 1000, 760).left, 400)
        with self.assertRaises(MissingControl):
            select_control(words, "Start routing", 1000, 760, (0, 0.5, 1, 1))
        with self.assertRaises(AmbiguousControl):
            select_control(words + [word("Start", 400, 200, 2), word("routing", 450, 200, 2)], "Start routing", 1000, 760)
        with self.assertRaises(MissingControl):
            select_control([word("Settings", 990, 100)], "Settings", 1000, 760)

    def test_codex_anchor_rejects_other_client_row(self):
        words = [word("Codex", 300, 300), word("Claude", 300, 500, 2),
                 word("Connect", 650, 500, 2), word("to", 715, 500, 2), word("routing", 740, 500, 2)]
        with self.assertRaises(MissingControl):
            select_control(words, "Connect to routing", 1000, 760, anchor="Codex")
        words += [word("Connect", 650, 300), word("to", 715, 300), word("routing", 740, 300)]
        self.assertEqual(select_control(words, "Connect to routing", 1000, 760, anchor="Codex").top, 300)

    def test_linux_window_ownership_includes_only_descendants(self):
        self.assertEqual(descendants(10, "10 1\n11 10\n12 11\n20 1\n21 20\n"), {10, 11, 12})

    def test_invalid_ocr_coordinates_rejected(self):
        header = "level\tpage_num\tblock_num\tpar_num\tline_num\tleft\ttop\twidth\theight\tconf\ttext\n"
        with self.assertRaisesRegex(RuntimeError, "coordinates"):
            read_words(header + "5\t1\t1\t1\t1\t-1\t0\t10\t10\t90\tStart\n")

    def test_upscaled_ocr_preserves_native_image_coordinates(self):
        header = "level\tpage_num\tblock_num\tpar_num\tline_num\tleft\ttop\twidth\theight\tconf\ttext\n"
        boxes = read_words(header + "5\t1\t1\t1\t1\t40\t20\t80\t32\t90\tSettings\n", scale=2)
        self.assertEqual((boxes[0].left, boxes[0].top, boxes[0].width, boxes[0].height), (20, 10, 40, 16))
        with self.assertRaisesRegex(RuntimeError, "scale"):
            read_words(header, scale=0)


class CredentialTests(unittest.TestCase):
    def test_linux_secret_bytes_have_no_appended_newline(self):
        vault = CredentialVault("linux", ".", set())
        with patch("unix_packages.require_runner"), patch("unix_packages.run") as run:
            run.return_value = subprocess.CompletedProcess([], 0, "fixture-key", "")
            vault.set("provider", "fixture-key", "ignored")
            self.assertEqual(run.call_args_list[0].kwargs["input_data"], "fixture-key")
            run.return_value = subprocess.CompletedProcess([], 0, "fixture-key\n", "")
            with self.assertRaisesRegex(RuntimeError, "not stored correctly"):
                vault.set("provider", "fixture-key", "ignored")

    def test_partial_macos_restoration_still_restores_search_and_deletes_fixture(self):
        with tempfile.TemporaryDirectory() as directory:
            vault = CredentialVault("macos", directory, set())
            vault.previous_default = "/original/login.keychain-db"
            vault.previous_search = [vault.previous_default]
            vault.default_captured = True
            vault.created = True
            calls = []

            def run(arguments, **_kwargs):
                calls.append(arguments)
                if arguments[1] == "default-keychain":
                    raise RuntimeError("original default restoration failed")
                if arguments[1] == "list-keychains" and "-s" not in arguments:
                    return subprocess.CompletedProcess(arguments, 0, '"/original/login.keychain-db"\n', "")
                return subprocess.CompletedProcess(arguments, 0, "", "")

            with patch("unix_packages.require_runner"), patch("unix_packages.run", side_effect=run):
                with self.assertRaisesRegex(RuntimeError, "original default restoration failed"):
                    vault.restore()
            self.assertTrue(any(arguments[1] == "list-keychains" and "-s" in arguments for arguments in calls))
            self.assertTrue(any(arguments[1] == "delete-keychain" for arguments in calls))
            self.assertFalse(vault.restored)

    def test_read_failure_before_changes_keeps_original_settings_unset(self):
        vault = CredentialVault("macos", ".", set())
        with patch("unix_packages.require_runner"), patch("unix_packages.run") as run:
            run.side_effect = [subprocess.CompletedProcess([], 0, '"/original/login.keychain-db"', ""), RuntimeError("search unavailable")]
            with self.assertRaisesRegex(RuntimeError, "search unavailable"):
                vault.start()
        self.assertIsNone(vault.previous_default)
        self.assertIsNone(vault.previous_search)
        self.assertFalse(vault.created)

    def test_macos_absent_default_is_restored_by_deleting_fixture_then_verified(self):
        with tempfile.TemporaryDirectory() as directory:
            vault = CredentialVault("macos", directory, set())
            state = {"default": None, "search": ["/Library/Keychains/System.keychain"]}
            original_search = list(state["search"])
            calls = []

            def run(arguments, **_kwargs):
                calls.append(arguments)
                if arguments[1] == "default-keychain":
                    if "-s" in arguments:
                        state["default"] = str(arguments[-1])
                    elif state["default"] is None:
                        return subprocess.CompletedProcess(arguments, 1, "", "security: SecKeychainCopyDomainDefault user: A default keychain could not be found.\n")
                    else:
                        return subprocess.CompletedProcess(arguments, 0, json.dumps(state["default"]), "")
                elif arguments[1] == "list-keychains":
                    if "-s" in arguments:
                        state["search"] = [str(value) for value in arguments[arguments.index("-s") + 1:]]
                    else:
                        return subprocess.CompletedProcess(arguments, 0, "\n".join(json.dumps(value) for value in state["search"]), "")
                elif arguments[1] == "delete-keychain":
                    state["default"] = None
                return subprocess.CompletedProcess(arguments, 0, "", "")

            with patch("unix_packages.require_runner"), patch("unix_packages.run", side_effect=run):
                vault.start()
                self.assertIsNone(vault.previous_default)
                self.assertTrue(vault.default_captured)
                vault.restore()
            self.assertEqual(state, {"default": None, "search": original_search})
            self.assertTrue(vault.restored)
            self.assertEqual(calls[-2][1], "default-keychain")
            self.assertEqual(calls[-3][1], "delete-keychain")

    def test_macos_other_default_keychain_errors_remain_failures(self):
        vault = CredentialVault("macos", ".", set())
        with patch("unix_packages.require_runner"), patch("unix_packages.run", return_value=subprocess.CompletedProcess([], 1, "", "permission denied")):
            with self.assertRaisesRegex(RuntimeError, "permission denied"):
                vault.start()
        self.assertFalse(vault.default_captured)
        self.assertFalse(vault.created)


class UpstreamTests(unittest.TestCase):
    def test_validation_errors_are_evidence_and_server_stops(self):
        upstream = lifecycle.SyntheticUpstream("fixture-key", "fixture-model")
        try:
            def request(key):
                return urllib.request.Request(upstream.endpoint + "/chat/completions", method="POST",
                                              headers={"Authorization": "Bearer " + key, "Content-Type": "application/json"},
                                              data=json.dumps({"model": "fixture-model"}).encode())
            with self.assertRaises(urllib.error.HTTPError) as failure:
                urllib.request.urlopen(request("incorrect"), timeout=5)
            self.assertEqual(failure.exception.code, 500)
            failure.exception.close()
            self.assertEqual(len(upstream.errors), 1)
            self.assertNotIn("fixture-key", upstream.errors[0])
            with urllib.request.urlopen(request("fixture-key"), timeout=5) as response:
                self.assertIn(b"NATIVE_CREDENTIAL_OK", response.read())
            self.assertEqual(upstream.requests[0]["authorizationMatched"], True)
        finally:
            upstream.stop()
        self.assertFalse(upstream.thread.is_alive())


if __name__ == "__main__":
    unittest.main()
