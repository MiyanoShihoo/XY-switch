import json
import os
import plistlib
import secrets
import shlex
import shutil
from pathlib import Path

from unix_gui import contained_path, file_sha256, require_runner, run


class Installation:
    def __init__(self, kind, package_format, temporary, output):
        require_runner(kind)
        if package_format not in {"linux": {"deb", "appimage"}, "macos": {"dmg"}}[kind]:
            raise RuntimeError("Installer format does not match the native platform")
        self.kind = kind
        self.format = package_format
        self.temporary = Path(temporary)
        self.output = Path(output)
        self.directory = contained_path(temporary, Path(temporary) / package_format / "Applications")
        self.directory.mkdir(parents=True)
        self.executable = None
        self.installed = False
        self.records = []
        self.sequence = 0
        if package_format == "deb":
            status = run(["dpkg-query", "--show", "--showformat=${Status}", "xy-switch"], check=False)
            if "install ok installed" in status.stdout:
                raise RuntimeError("Refusing a pre-existing system XY Switch installation")

    def remove_path(self, target):
        if Path(target).is_symlink():
            raise RuntimeError("Refusing a linked lifecycle installation directory")
        target = contained_path(self.temporary, target)
        if target.is_dir():
            shutil.rmtree(target)
        elif target.exists():
            target.unlink()

    def install(self, package, version):
        require_runner(self.kind)
        source = Path(package["path"]).resolve(strict=True)
        if file_sha256(source) != package["sha256"]:
            raise RuntimeError("Installer changed after verification")
        self.sequence += 1
        record = {"format": self.format, "version": version, "package": source.name, "packageSha256": package["sha256"]}
        if self.format == "deb":
            name = run(["dpkg-deb", "--field", source, "Package"]).stdout.strip()
            actual_version = run(["dpkg-deb", "--field", source, "Version"]).stdout.strip()
            if name != "xy-switch" or actual_version != version:
                raise RuntimeError("Unexpected Debian package identity")
            self.installed = True
            result = run(["sudo", "dpkg", "-i", source], timeout=180)
            (self.output / f"deb-install-{self.sequence}.txt").write_text(result.stdout + result.stderr)
            self.executable = Path("/usr/bin/xy-switch")
            extracted = contained_path(self.temporary, self.temporary / f"deb-payload-{self.sequence}")
            run(["dpkg-deb", "-x", source, extracted])
            if file_sha256(self.executable) != file_sha256(extracted / "usr/bin/xy-switch"):
                raise RuntimeError("Installed Debian binary differs from the verified package payload")
            record["registeredVersion"] = run(["dpkg-query", "--show", "--showformat=${Version}", "xy-switch"]).stdout.strip()
            if record["registeredVersion"] != version:
                raise RuntimeError("Debian registered version differs after installation")
        elif self.format == "appimage":
            self.executable = contained_path(self.temporary, self.directory / "XY-Switch.AppImage")
            shutil.copy2(source, self.executable)
            self.executable.chmod(0o755)
            self.installed = True
            if file_sha256(self.executable) != package["sha256"]:
                raise RuntimeError("Installed AppImage differs from its verified package")
        else:
            mounted = contained_path(self.temporary, self.temporary / f"dmg-mount-{self.sequence}")
            mounted.mkdir()
            run(["hdiutil", "attach", source, "-readonly", "-nobrowse", "-mountpoint", mounted], timeout=120)
            failure = None
            try:
                apps = list(mounted.glob("*.app"))
                if len(apps) != 1 or apps[0].is_symlink():
                    raise RuntimeError("Expected one real application bundle in the verified DMG")
                bundle = apps[0]
                metadata = plistlib.loads((bundle / "Contents/Info.plist").read_bytes())
                if metadata["CFBundleIdentifier"] != "com.xyswitch.desktop" or metadata["CFBundleShortVersionString"] != version:
                    raise RuntimeError("DMG application identity differs from the requested version")
                destination = contained_path(self.temporary, self.directory / bundle.name)
                if destination.exists():
                    self.remove_path(destination)
                self.installed = True
                run(["ditto", bundle, destination], timeout=120)
                self.executable = destination / "Contents/MacOS" / metadata["CFBundleExecutable"]
                if file_sha256(self.executable) != file_sha256(bundle / "Contents/MacOS" / metadata["CFBundleExecutable"]):
                    raise RuntimeError("Installed macOS binary differs from the mounted DMG")
                run(["codesign", "--verify", "--deep", "--strict", destination])
                details = run(["codesign", "--display", "--verbose=4", destination]).stderr
                assessment = run(["spctl", "--assess", "--type", "execute", "--verbose=4", destination], check=False)
                architectures = run(["lipo", "-archs", self.executable]).stdout.split()
                if set(architectures) != {"x86_64", "arm64"}:
                    raise RuntimeError("macOS bundle is not Universal")
                record.update({"codesignIntegrity": "verified", "architectures": architectures,
                               "signing": "ad-hoc" if "Signature=adhoc" in details else "certificate",
                               "gatekeeper": "accepted" if assessment.returncode == 0 else "not-accepted"})
                (self.output / f"macos-signature-{self.sequence}.txt").write_text(details)
                (self.output / f"macos-gatekeeper-{self.sequence}.txt").write_text(assessment.stdout + assessment.stderr)
            except BaseException as error:
                failure = error
                raise
            finally:
                detached = run(["hdiutil", "detach", mounted], check=False, timeout=60)
                if detached.returncode and failure is None:
                    raise RuntimeError("Failed to detach the test DMG")
                if detached.returncode:
                    record["detachError"] = detached.stderr.strip()
        if not self.executable or not self.executable.is_file():
            raise RuntimeError("Installation did not deploy an executable")
        record["installedSha256"] = file_sha256(self.executable)
        self.records.append(record)

    def uninstall(self):
        if not self.installed:
            return
        require_runner(self.kind)
        if self.format == "deb":
            result = run(["sudo", "dpkg", "-r", "xy-switch"], timeout=120)
            (self.output / f"deb-uninstall-{self.sequence}.txt").write_text(result.stdout + result.stderr)
        else:
            target = self.executable if self.format == "appimage" else self.executable.parents[2]
            self.remove_path(target)
        if self.executable.exists():
            raise RuntimeError("Uninstallation retained the deployed executable")
        self.installed = False


class CredentialVault:
    def __init__(self, kind, temporary, secret_values):
        self.kind = kind
        self.temporary = Path(temporary)
        self.secrets = secret_values
        self.keychain = self.temporary / "fixture.keychain-db"
        self.previous_default = None
        self.previous_search = None
        self.default_captured = False
        self.created = False
        self.entries = set()
        self.restored = False

    def start(self):
        require_runner(self.kind)
        if self.kind == "macos":
            previous_default = self.default_keychain()
            previous_search = shlex.split(run(["security", "list-keychains", "-d", "user"]).stdout)
            self.previous_default, self.previous_search = previous_default, previous_search
            self.default_captured = True
            password = secrets.token_hex(24)
            self.secrets.add(password)
            contained_path(self.temporary, self.keychain)
            run(["security", "create-keychain", "-p", password, self.keychain])
            self.created = True
            run(["security", "set-keychain-settings", "-lut", "3600", self.keychain])
            run(["security", "unlock-keychain", "-p", password, self.keychain])
            run(["security", "default-keychain", "-d", "user", "-s", self.keychain])
            run(["security", "list-keychains", "-d", "user", "-s", self.keychain])
            if self.default_keychain() != str(self.keychain.resolve()):
                raise RuntimeError("Isolated macOS default keychain was not persisted for new processes")
            if shlex.split(run(["security", "list-keychains", "-d", "user"]).stdout) != [str(self.keychain.resolve())]:
                raise RuntimeError("Isolated macOS keychain search list was not persisted for new processes")

    @staticmethod
    def default_keychain():
        result = run(["security", "default-keychain", "-d", "user"], check=False)
        if result.returncode:
            if result.returncode == 1 and "SecKeychainCopyDomainDefault user: A default keychain could not be found." in result.stderr:
                return None
            raise RuntimeError("Cannot read original macOS default keychain: " + result.stderr.strip())
        names = shlex.split(result.stdout)
        if len(names) > 1:
            raise RuntimeError("More than one macOS default keychain was returned")
        return names[0] if names else None

    @staticmethod
    def account(provider_id):
        return "XY Switch/provider/" + provider_id

    def set(self, provider_id, secret, executable):
        require_runner(self.kind)
        self.secrets.add(secret)
        account = self.account(provider_id)
        self.entries.add(provider_id)
        if self.kind == "linux":
            run(["secret-tool", "store", "--label=XY Switch lifecycle fixture", "service", "XY Switch",
                 "username", account, "target", "default", "application", "rust-keyring"], input_data=secret)
        else:
            run(["security", "add-generic-password", "-a", account, "-s", "XY Switch", "-w", secret,
                 "-T", "/usr/bin/security", "-T", executable, self.keychain])
        if self.read(provider_id) != secret:
            raise RuntimeError("Synthetic native credential was not stored correctly")

    def read(self, provider_id):
        if self.kind == "linux":
            result = run(["secret-tool", "lookup", "service", "XY Switch", "username", self.account(provider_id), "target", "default"], check=False)
        else:
            result = run(["security", "find-generic-password", "-a", self.account(provider_id), "-s", "XY Switch", "-w", self.keychain], check=False)
        if result.returncode:
            return None
        return result.stdout if self.kind == "linux" else result.stdout.removesuffix("\n")

    def clear(self, provider_id):
        require_runner(self.kind)
        if provider_id not in self.entries:
            return
        if self.kind == "linux":
            run(["secret-tool", "clear", "service", "XY Switch", "username", self.account(provider_id), "target", "default"], check=False)
        else:
            run(["security", "delete-generic-password", "-a", self.account(provider_id), "-s", "XY Switch", self.keychain], check=False)
        if self.read(provider_id) is not None:
            raise RuntimeError("Synthetic credential cleanup failed")
        self.entries.remove(provider_id)

    def restore(self):
        require_runner(self.kind)
        failures = []
        for provider_id in list(self.entries):
            try:
                self.clear(provider_id)
            except Exception as error:
                failures.append(str(error))
        if self.kind == "macos" and self.default_captured:
            settings = [["security", "list-keychains", "-d", "user", "-s", *self.previous_search]]
            if self.previous_default is not None:
                settings.insert(0, ["security", "default-keychain", "-d", "user", "-s", self.previous_default])
            for arguments in settings:
                try:
                    run(arguments)
                except Exception as error:
                    failures.append(str(error))
            if self.created:
                try:
                    run(["security", "delete-keychain", contained_path(self.temporary, self.keychain)])
                except Exception as error:
                    failures.append(str(error))
            try:
                if self.default_keychain() != self.previous_default:
                    failures.append("Original macOS default keychain was not restored")
            except Exception as error:
                failures.append(str(error))
            try:
                if shlex.split(run(["security", "list-keychains", "-d", "user"]).stdout) != self.previous_search:
                    failures.append("Original macOS keychain search list was not restored")
            except Exception as error:
                failures.append(str(error))
        self.restored = not failures
        if failures:
            raise RuntimeError("; ".join(failures))
