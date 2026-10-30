# 桌面平台构建、验证与分发

[English](packaging-en.md) | 简体中文 · [用户手册](user-guide-zh.md) · [1.1.0 发布说明](release-notes-1.1.0.md)

当前仅构建和维护 **Windows、macOS、Linux**。iOS 与 Android 构建、原生验收及签名入口自 2026-10-30 起移除；历史发布产物不追溯删除。版本号来自 `package.json`，桌面打包时检查其与 `src-tauri/tauri.conf.json` 一致。

## 通用准备

安装 Node.js 22.12+ 的兼容版本、pnpm 10、Rust stable 和 Git，在源码根目录执行：

```bash
pnpm install --frozen-lockfile
pnpm lint
pnpm test
pnpm test:packaging
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

Windows 与 WSL 不应共用同一个 `node_modules`。在独立的源码副本安装 Linux 依赖，不要为构建删除原项目、改动用户 Git 暂存区或覆盖应用数据。

### 工具与输出目录

Windows/Linux/macOS 包装脚本接受 `--tools-dir`、`--out-dir`、`--bundles`、`--dry-run`。参数以独立 argv 传给程序，不拼接 shell 命令。

- 默认工具目录：项目根目录的 `.build-tools/`。
- 环境变量：`XY_SWITCH_TOOLS_DIR`；命令行 `--tools-dir` 优先。
- 编译缓存：尊重 `CARGO_TARGET_DIR`，默认 `src-tauri/target/`。
- 安装器工具：`target/.tauri` 通过目录链接指向选定的工具目录。已有不同缓存时拒绝替换；可以指定全新的 `CARGO_TARGET_DIR`。
- 默认输出：`release/<version>/<platform>-<architecture>/`。
- Rust crates、npm 包、SDK 和操作系统包使用各自的缓存配置，不由 `--tools-dir` 重定向。

本机新工具统一放在 `E:\AI tools`，实际命令如下，不需要修改公开脚本：

```powershell
$env:XY_SWITCH_TOOLS_DIR = 'E:\AI tools\xy-switch\windows\bundlers'
pnpm build:windows
```

在 WSL 中使用 Windows E 盘：

```bash
export XY_SWITCH_TOOLS_DIR='/mnt/e/AI tools/xy-switch/linux/bundlers'
pnpm build:linux
```

脚本完整调用 Tauri build，为本次新生成的安装包计算 SHA-256。不会只打包旧 EXE 或旧 `.wixobj`。manifest 不声称已经验证代码签名。

## Windows

最终安装/窗口/卸载检查由 `scripts/verify-windows-bundles.ps1` 在可丢弃的 GitHub-hosted Windows runner 中执行。release 构建忽略调试数据目录覆盖，因此脚本拒绝在普通用户账户运行，不能为了验收而启动程序碰触真实数据库。


需要 Microsoft C++ Build Tools、Windows SDK 和 WebView2 Runtime。在能找到 MSVC 链接器的开发者终端执行：

```powershell
pnpm build:windows --tools-dir 'E:\AI tools\xy-switch\windows\bundlers'
# 只生成当前用户安装器
pnpm build:windows --bundles nsis
# 查看构建命令，不开始构建
pnpm build:windows --dry-run
```

x64 默认输出：

- `XY-Switch-<version>-windows-x64-setup.exe`：NSIS 当前用户安装器，中/英安装界面。
- `XY-Switch-<version>-windows-x64.msi`：MSI 安装包。
- `manifest.json`、`SHA256SUMS.txt`。

应用界面支持简体中文和英文，可在设置中选择跟随系统；语言选择与安装器语言独立。Windows ARM64 入口默认只使用 NSIS，不等于该架构已经过当前发布验证。未配置代码签名证书的包是未签名安装包，系统可能提示未知发布者；核对来源和校验和，不通过关闭系统防护解决签名提示。

凭据往返测试只使用随机测试 ID，并在结束时清理：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked stores_reads_and_deletes_windows_credential -- --ignored --test-threads=1
```

## Linux

`tauri.linux.conf.json` 显式启用 `tauri/tray-icon`：该依赖在 Cargo 中仅对桌面目标启用，仍需让打包器识别，才能自动写入 Debian 依赖并把 AppIndicator 动态库及其依赖一起打入 AppImage。不能只以 Rust 编译成功或 `ldd` 主程序无缺失作为打包通过的依据。

构建后执行 `bash scripts/verify-linux-bundles.sh`，检查 Debian 元数据、AppImage 的动态托盘库，以及两种包在独立 HOME、D-Bus、Xvfb 环境中的实际启动。CI 自动执行此检查。Linux 中文显示需要系统 CJK 字体，Debian 包推荐 `fonts-noto-cjk`；最小化 WSL 环境若未安装中文字体，会显示方框，这不等于前端资源缺失。


Ubuntu / Debian 构建依赖：

```bash
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev librsvg2-dev libssl-dev libdbus-1-dev \
  libxdo-dev patchelf gnome-keyring dbus-x11 xvfb
pnpm build:linux
bash scripts/test-linux-keyring.sh
```

密钥库测试使用临时 HOME、临时 Keyring 和独立 D-Bus，不读取用户已有密钥库。普通 Rust 回归默认忽略需要真实密钥库的测试，不能将“忽略”算作“通过”。

产物为 `.deb` 和 `.AppImage`。GitHub Linux 构建基线为 Ubuntu 22.04；在更新系统上自行构建的包可能需要更新的 glibc，不能仅改文件名扩大兼容范围。

在下载附件的目录中：

```bash
sha256sum -c SHA256SUMS.txt
sudo apt install ./XY-Switch-*-linux-x64.deb
# 或使用 AppImage
chmod +x ./XY-Switch-*-linux-x64.AppImage
./XY-Switch-*-linux-x64.AppImage
```

运行需要兼容的系统库、图形会话和已解锁的 Secret Service。无系统托盘时正常关闭窗口，不会隐藏失联。本版本 Linux 暂不提供自启设置。无 FUSE 的受限构建环境可用 `APPIMAGE_EXTRACT_AND_RUN=1` 运行打包器，这不意味着应用没有运行依赖。

## macOS

在安装 Xcode Command Line Tools 的 Mac 上执行：

```bash
pnpm build:macos:universal
bash scripts/verify-macos-bundles.sh universal
```

`--universal` 生成 `arm64` + `x86_64` 的 Universal 二进制，并输出 `.dmg` 和 `.app.zip`。只构建当前 Mac 架构时使用 `pnpm build:macos`，验收脚本的参数对应 `arm64` 或 `x64`。构建目标最低为 macOS 11；CI 只覆盖其实际宿主版本和架构，不代表全部旧系统/Intel 硬件已经运行验证。

未设置 `APPLE_SIGNING_IDENTITY` 时使用 ad-hoc 签名，以便校验包完整性，但**没有 Developer ID 身份和 Apple 公证**。`codesign --verify` 成功并不代表 Gatekeeper 一定允许打开。正式分发应提供自己的签名身份，按 Tauri 官方流程完成公证；不要把证书或密码提交到仓库，也不要为测试全局关闭 Gatekeeper。

安装时挂载 DMG，将 `XY Switch.app` 复制到应用目录。默认未公证包可能被系统阻止，具体状态以该版本说明为准；只有确认来源和校验值后才按系统提供的方式批准运行，或选择自行从源码构建。

验收检查 Universal 架构、Bundle ID/版本、代码签名完整性、DMG 内与 App ZIP 内的同一程序，并在临时 HOME 下启动和保存真实窗口截图。Keychain 测试另用临时密钥库：

```bash
# 仅在可丢弃的 macOS 测试账户执行；脚本结束会恢复默认密钥库与搜索列表。
XY_SWITCH_ISOLATED_KEYCHAIN_TEST=1 bash scripts/test-macos-keychain.sh
```

macOS 支持桌面配置管理和系统托盘；本程序暂不提供 macOS 开机自启设置。

## 发布检查

1. 确认同一提交、同一 CI run 的前端、Windows、Linux、macOS 四项检查 均实际通过；默认忽略的凭据测试需单独运行。
2. 检查产物版本、架构、manifest 和 SHA-256。
3. 在发布说明中写明 Windows 未签名、macOS ad-hoc/未公证或实际提供的签名状态，以及各桌面平台的真实验证级别。
4. 源码通过 Git 提交；安装包放在 Releases，不塞进源码历史。
5. 检查最终暂存树没有密钥、数据库、日志、个人配置和签名材料。

官方参考：[环境准备](https://v2.tauri.app/start/prerequisites/)、[Windows 安装包](https://v2.tauri.app/distribute/windows-installer/)、[Debian 包](https://v2.tauri.app/distribute/debian/)。

### Linux 托盘动态依赖

Linux 配置通过 Cargo 中命名的 `linux-tray = ["tauri/tray-icon"]` feature 启用托盘打包。不能只把原始依赖 feature `tauri/tray-icon` 写入 `build.features`：当前锁定的 Tauri CLI 会展开 crate feature 名称，直接依赖 feature 虽能被 Cargo 接受，却不会进入打包器的动态库清单。CI 会保存 DEB control 元数据、检查 AppIndicator 依赖和 AppImage 中的实际动态库，并分别隔离启动两种包；失败时也保留产物和诊断，不可仅以编译成功判定可发布。
