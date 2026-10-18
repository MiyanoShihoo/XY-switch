# 多平台构建、验证与分发

构建脚本分别处理 Windows、Linux 与 iOS，不把桌面托盘或跨应用目录权限伪装成移动端功能。版本号来自 `package.json`，桌面打包时检查其与 `src-tauri/tauri.conf.json` 一致。

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

Windows/Linux 包装脚本接受 `--tools-dir`、`--out-dir`、`--bundles`、`--dry-run`。参数以独立 argv 传给程序，不拼接 shell 命令。

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

1. 确认源码测试、凭据往返和本次构建成功。
2. 检查产物版本、架构、manifest 和 SHA-256。
3. 在发布说明中写明系统、签名情况和 iOS 实际验证级别。
4. 源码通过 Git 提交；安装包放在 Releases，不塞进源码历史。
5. 检查最终暂存树没有密钥、数据库、日志、个人配置和签名材料。

官方参考：[环境准备](https://v2.tauri.app/start/prerequisites/)、[Windows 安装包](https://v2.tauri.app/distribute/windows-installer/)、[Debian 包](https://v2.tauri.app/distribute/debian/)、[iOS 分发](https://v2.tauri.app/distribute/app-store/)。

### iOS 模拟器界面验收

模拟器检查必须同时确认应用进程存活、界面内容已渲染以及保存实际截图。首次启动 WebKit 不采用固定五秒后直接判成功；脚本最多等待两分钟，并排除状态栏/底部系统区域后检查内容对比度。仅显示系统状态栏的空白截图会使 CI 失败。该检查不替代截图人工复核、完整交互测试或真机验证。
