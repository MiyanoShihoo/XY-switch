# Build, verify, and distribute

English | [简体中文](packaging.md) · [User guide](user-guide-en.md) · [1.1.0 notes](release-notes-1.1.0.md)

Current development builds and maintains **Windows, macOS, and Linux only**. iOS and Android packaging, native acceptance, and signing entry points were removed on 2026-10-30; historical release assets remain unchanged. Windows installers are unsigned unless you supply signing; macOS defaults to ad-hoc signing without notarization. A successful build alone is not native acceptance.

## Prepare the source

Use a compatible Node.js 22.12+ release, pnpm 10, Rust, Git, and the platform prerequisites below. The main workflow pins Rust **1.98.1** and uses Node.js 24. From the repository root:

```bash
pnpm install --frozen-lockfile
pnpm lint
pnpm test
pnpm test:packaging
pnpm build:renderer
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

Use separate source/dependency directories for Windows and WSL; do not share one `node_modules` between them. Preserve application data and the user's Git index when preparing an isolated build.

### Tools and outputs

Desktop wrappers accept `--tools-dir`, `--out-dir`, `--bundles`, and `--dry-run`. Arguments are passed as separate argv values. `--tools-dir` overrides `XY_SWITCH_TOOLS_DIR`; its default is `.build-tools/`. Cargo respects `CARGO_TARGET_DIR`, defaulting to `src-tauri/target/`. The bundler's `.tauri` directory links to the selected tools directory and refuses to overwrite an incompatible existing cache; choose a fresh target directory instead.

Final output defaults to `release/<version>/<platform>-<architecture>/`, including a manifest and SHA-256 checksums. The wrapper builds fresh packages rather than merely wrapping an old executable. npm, Cargo, SDK and OS-package caches retain their own settings; `--tools-dir` does not redirect all of them. Signing keys are not disposable build caches.

## Windows

Use Windows with the MSVC C++ build tools, Windows SDK, Rust MSVC target, Node.js/pnpm and WebView2. Run from a developer PowerShell:

```powershell
pnpm build:windows
# Current-user NSIS installer only
pnpm build:windows --bundles nsis
# Inspect the command without starting a build
pnpm build:windows --dry-run
./scripts/verify-windows-bundles.ps1
cargo test --manifest-path src-tauri/Cargo.toml --locked stores_reads_and_deletes_windows_credential -- --ignored --test-threads=1
```

The x64 distribution is NSIS `.exe` and `.msi`, not a portable ZIP. NSIS supports English and Simplified Chinese; the app's language preference is independent. Without configured code signing, installers remain unsigned. An ARM64 build entry is not evidence of an ARM64 runtime test.

Native verification checks the packaged application, installer/uninstaller and actual rendered windows using isolated test data. Credential Manager round-trip tests are deliberately separate from the default Rust test run.

## Linux

The reference desktop runner is Ubuntu 22.04. Install build/runtime prerequisites on a compatible distribution:

```bash
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev librsvg2-dev libssl-dev libdbus-1-dev \
  libxdo-dev patchelf gnome-keyring dbus-x11 xvfb xdotool imagemagick
pnpm build:linux
bash scripts/test-linux-keyring.sh
bash scripts/verify-linux-bundles.sh
```

The distribution contains a DEB and AppImage. Runtime use needs compatible libraries, a graphical session and an unlocked Secret Service keyring. Tray behavior depends on desktop support; without a tray, closing the window must not leave an inaccessible hidden application. Linux autostart settings are not implemented. `APPIMAGE_EXTRACT_AND_RUN=1` helps restricted build environments without FUSE; it does not remove runtime dependencies.

The named Cargo feature `linux-tray = ["tauri/tray-icon"]` lets the locked Tauri CLI collect tray libraries for bundling. CI inspects DEB dependencies and the AppImage's actual libraries, then launches both packages in isolation. Compile success is not a substitute for these checks.

## macOS

On a Mac with Xcode Command Line Tools:

```bash
pnpm build:macos:universal
bash scripts/verify-macos-bundles.sh universal
# Use only in a disposable macOS test account
XY_SWITCH_ISOLATED_KEYCHAIN_TEST=1 bash scripts/test-macos-keychain.sh
```

Universal output includes `arm64` and `x86_64` in `.app.zip` and `.dmg`. `pnpm build:macos` targets the host architecture; pass `arm64` or `x64` to the verification script as appropriate. The configured deployment target is macOS 11, but CI covers its actual host OS/architecture, not every older system or Intel device.

Without `APPLE_SIGNING_IDENTITY`, the build uses ad-hoc signing: integrity checks can pass without Developer ID signing or notarization. `codesign --verify` does not guarantee Gatekeeper acceptance. Supply your own signing identity and follow the official distribution process for notarized releases; do not disable system protections to make a test pass.

Verification checks architectures, bundle ID/version, signatures, the program in both archives, and a rendered native window. Keychain tests create a temporary keychain and restore the default/search list. This app does not currently provide macOS autostart settings.

## Release gates

1. Require all four checks—frontend, Windows, Linux and macOS—to pass in **one run for one source commit**, including separately invoked credential tests.
2. Check package versions, architectures, identities, manifests and hashes; review native screenshots.
3. Publish the source through Git and packages through Releases, not binary blobs in source history.
4. Preserve actual signing/device limitations in release notes and ensure no keys, databases, personal configuration, logs or profiles enter the publication snapshot.
5. Re-download published assets anonymously and compare SHA-256 with the verified local copies.

Browser UI suites are synthetic and separate from native gates. With the renderer served on `http://127.0.0.1:1431` and a dedicated local Chrome CDP endpoint on port 9231, run `pnpm test:ui` and `pnpm test:ui:i18n`. `APP_URL`, `CDP_ENDPOINT`, `QA_OUTPUT` and `I18N_QA_OUTPUT` override the defaults. Do not attach these fixture-injecting suites to your personal browser or real application data.

Official references: [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/), [Windows installers](https://v2.tauri.app/distribute/windows-installer/), [Debian packages](https://v2.tauri.app/distribute/debian/).
