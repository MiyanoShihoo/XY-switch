# 多平台构建、验证与分发

构建脚本分别处理 Windows、Linux、macOS、Android 与 iOS，不把桌面托盘或跨应用目录权限伪装成移动端功能。版本号来自 `package.json`，桌面打包时检查其与 `src-tauri/tauri.conf.json` 一致。

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

应用界面本身为简体中文。Windows ARM64 入口默认只使用 NSIS，不等于该架构已经过当前发布验证。未配置代码签名证书的包是未签名安装包，系统可能提示未知发布者；核对来源和校验和，不通过关闭系统防护解决签名提示。

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

## Android

### 工具链与 APK

需要 JDK 21、Android SDK Platform 36、Build Tools 36.0.0、NDK 27.2.12479018、Rust 和 Node.js。Windows 还需要可用的 MSVC 编译环境来构建 Rust 的宿主工具，以及文件符号链接权限（Tauri 会链接原生库）。构建脚本会先检查符号链接能力，但不会自动开启 Windows 开发者模式或更改系统策略；没有该权限时可改用 Linux/macOS 构建。`JAVA_HOME` 指向 JDK，`ANDROID_HOME` 指向 SDK，`NDK_HOME` 指向 SDK 内的 `ndk/27.2.12479018`。使用 SDK command-line tools 安装依赖：

```bash
sdkmanager 'platforms;android-36' 'build-tools;36.0.0' 'ndk;27.2.12479018'
```

本机将新工具安装在 `E:\AI tools`，不要求更改公开源码中的绝对路径。已配置上述环境变量后，在开发者 PowerShell 中执行：

```powershell
pnpm build:android --tools-dir 'E:\AI tools\xy-switch\android'
# 查看计划，不生成密钥或下载组件
pnpm build:android --dry-run
```

默认输出包含 `arm64-v8a` 和 `x86_64` 两个 ABI 的通用 release APK，支持 Android 7.0/API 24+。`--targets aarch64` 可只构建 ARM64，实际 ABI 以 manifest 为准；仅有 Windows/macOS 桌面 EXE/App 不能安装到 Android。

脚本按需初始化 Git 忽略的 Android 工程，生成或使用签名，构建并独立检查包名、版本、SDK、非调试属性、备份策略、签名、ZIP 对齐与所有原生库的 16 KB ELF 页对齐。16 KB 静态对齐通过不等于所有 16 KB 真机已运行测试。

### 签名与安装

默认在工具目录的 `signing/` 内保存随机开发签名私钥和口令，APK 文件名明确带 `development-signed`。它是可安装的测试分发包，**不是 Google Play 正式发行版**。CI 使用可丢弃的开发证书；不同构建的证书可能不同，因此不能保证相互覆盖升级。卸载会丢失应用沙盒和 Keystore 中的凭据，替换证书前应先迁移普通配置并准备重新录入密钥。

正式签名需要同时提供四个环境变量，私钥文件必须放在源码工作区之外：

| 变量 | 内容 |
| --- | --- |
| `XY_ANDROID_KEYSTORE_PATH` | 外部 keystore 的绝对路径 |
| `XY_ANDROID_STORE_PASSWORD` | keystore 口令 |
| `XY_ANDROID_KEY_ALIAS` | 签名别名 |
| `XY_ANDROID_KEY_PASSWORD` | 私钥口令 |

不要把口令写进命令行、Gradle 文件、仓库或发布附件。SHA-256 和证书指纹会写入产物清单，私钥与口令不会。

将对应 Release 的 APK 复制到自己的 Android 设备，核对校验值后通过系统安装器安装。手机可能需要针对所用文件管理器/浏览器允许安装外部来源应用；本项目不要求修改全局系统安全设置。

### 真正调用 Keystore 的测试

凭据使用 AndroidKeyStore 管理的 AES-256-GCM 密钥和应用私有 `noBackupFilesDir` 密文记录，记录绑定凭据作用域；不声明每台设备都具备硬件背书。损坏记录或密钥丢失会明确失败，不悄悄回退到明文。私有原生插件的读/写/删入口拒绝 JavaScript 调用，只允许 Rust 后端使用。

构建测试 APK 后，使用**专用、可丢弃**且名称为 `xy-switch-ci-<唯一标识>` 的模拟器：

```bash
pnpm build:android --instrumentation
node scripts/smoke-android-emulator.mjs --serial emulator-5554
```

验收脚本同时检查明确的 emulator serial、QEMU 标识与专用 AVD 名称，拒绝真实手机和个人模拟器。测试包含 4 项原生加密存储测试以及 1 项最终 release APK 的 WebView → Rust → Keystore 集成测试，覆盖保存/读取/更新/删除、重建、篡改、密钥丢失与私有 JS 桥拒绝。凭据读取通过模拟器回环 HTTP fixture 验证，不调用真实模型、不产生付费请求。另保存启动 PID、聚焦窗口非空白截图、测试输出和设备页大小。当前自动化目标为 x86_64 模拟器；ARM64 的静态/打包检查不等于 ARM64 真机验收。

移动版只管理自身沙盒中的配置，不能接管其他 App 的桌面 CLI，也不能读取其 Skills/会话目录。用户明确配置的本地/LAN HTTP 端点可用；密钥不会因明文 HTTP 端点自动获得传输加密。后台代理生命周期受 Android 系统管理，不保证常驻。

## iOS 模拟器

需要 macOS、Xcode、iOS Simulator runtime 和 CocoaPods。脚本根据 Mac 架构选择对应模拟器目标：

```bash
pnpm build:ios:simulator
bash scripts/smoke-ios-simulator.sh
```

脚本初始化被 Git 忽略的 `src-tauri/gen/apple/`，安装对应 Rust target，构建模拟器 App 并归档到 `release/<version>/ios-<architecture>-simulator/`。

启动测试创建独立模拟器，安装、启动 App、确认进程仍存在并保存截图，结束时清理。它验证模拟器安装/启动，不是 iPhone 真机稳定性、后台代理或 App Store 审核结果。

**模拟器 ZIP 不能安装到 iPhone/iPad，也不能改扩展名当作 IPA。** iOS 仅操作自身沙盒数据，不支持桌面客户端接管、其他 App 的 Skills/会话目录、托盘或开机自启。

## iOS 真机签名

独立工作流 `Build signed iOS device package` 不会在普通推送时自动运行。先配置 GitHub Actions Repository Secrets：

| Secret | 内容 |
| --- | --- |
| `APPLE_DEVELOPMENT_TEAM` | Apple 开发团队 ID |
| `IOS_CERTIFICATE` | `.p12` 签名证书的 Base64 |
| `IOS_CERTIFICATE_PASSWORD` | 证书密码 |
| `IOS_MOBILE_PROVISION` | 匹配的 `.mobileprovision` 的 Base64 |

这些变量由当前 Tauri CLI 原生读取并配置临时签名环境，不要把证书、私钥或其 Base64 提交到仓库。描述文件必须匹配 Bundle ID（当前 `com.xyswitch.desktop`）和所选导出方法。在已配置 Apple 签名的 Mac 终端也可以执行：

```bash
pnpm build:ios
```

`IOS_EXPORT_METHOD` 支持 `app-store-connect`、`release-testing`、`debugging`，默认 `app-store-connect`。App Store/TestFlight 产物需要通过 Apple 渠道分发；开发/测试 IPA 可安装的设备范围由描述文件决定，并非对所有 iPhone 通用。

真机脚本不使用 `--no-sign`，会检查 IPA 中的描述文件和代码签名。没有签名材料就明确失败，不伪造可安装 IPA。构建入口、模拟器验证和真机签名分发是三个不同交付状态。

## 发布检查

1. 确认同一提交、同一 CI run 的前端、Windows、Linux、macOS、Android、iOS 六个 job 均实际通过；默认忽略的凭据测试需单独运行。
2. 检查产物版本、架构、manifest 和 SHA-256。
3. 在发布说明中写明 Windows 未签名、macOS ad-hoc/未公证、Android 开发签名或实际提供的签名状态，以及 iOS 模拟器/真机的真实验证级别。
4. 源码通过 Git 提交；安装包放在 Releases，不塞进源码历史。
5. 检查最终暂存树没有密钥、数据库、日志、个人配置和签名材料。

官方参考：[环境准备](https://v2.tauri.app/start/prerequisites/)、[Windows 安装包](https://v2.tauri.app/distribute/windows-installer/)、[Debian 包](https://v2.tauri.app/distribute/debian/)、[iOS 分发](https://v2.tauri.app/distribute/app-store/)。

### iOS 模拟器界面验收

模拟器检查必须同时确认应用进程存活、界面内容已渲染以及保存实际截图。首次启动 WebKit 不采用固定五秒后直接判成功；脚本最多等待两分钟，并排除状态栏/底部系统区域后检查内容对比度。仅显示系统状态栏的空白截图会使 CI 失败。该检查不替代截图人工复核、完整交互测试或真机验证。

### Linux 托盘动态依赖

Linux 配置通过 Cargo 中命名的 `linux-tray = ["tauri/tray-icon"]` feature 启用托盘打包。不能只把原始依赖 feature `tauri/tray-icon` 写入 `build.features`：当前锁定的 Tauri CLI 会展开 crate feature 名称，直接依赖 feature 虽能被 Cargo 接受，却不会进入打包器的动态库清单。CI 会保存 DEB control 元数据、检查 AppIndicator 依赖和 AppImage 中的实际动态库，并分别隔离启动两种包；失败时也保留产物和诊断，不可仅以编译成功判定可发布。
