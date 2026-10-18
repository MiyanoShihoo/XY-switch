# XY Switch 原生 EXE 构建与发布记录

日期：2026-10-17（Asia/Shanghai）  
工作区：`E:\XY switch`

## 结论

**已从当前工作区源码成功生成新的 Windows x64 release EXE。** Windows SDK 缺失问题已解决；没有拉取远端代码、升级项目依赖、修改版本号或覆盖旧发布文件。这里的“新版”指本轮七页工作台改版对应的源码构建，应用版本号仍为 1.0.0。

本次完成编译及静态产物检查；**尚未启动生产 EXE，也未验证真实 Tauri IPC 或真实网站/Key 请求**。浏览器 fixture 测试不等同于原生实机测试。

## 1. 交付文件

| 项目 | 结果 |
| --- | --- |
| EXE | [XY-Switch-1.0.0-workbench-20261017.exe](E:/XY%20switch/release/XY-Switch-1.0.0-workbench-20261017.exe) |
| 构建结束 | 2026-10-17T20:37:24.6831338+08:00 |
| 架构 | Windows x64 / PE32+ / GUI 子系统 |
| 文件版本 / 产品版本 | 1.0.0 / 1.0.0 |
| 大小 | 15,251,456 bytes（14.54 MiB） |
| 数字签名 | NotSigned；本次没有签名证书 |
| 校验文件 | [XY-Switch-1.0.0-workbench-20261017.exe.sha256](E:/XY%20switch/release/XY-Switch-1.0.0-workbench-20261017.exe.sha256) |
| 机器可读记录 | [构建清单](E:/XY%20switch/release/XY-Switch-1.0.0-workbench-20261017.build.json) |
| 运行说明 | [README](E:/XY%20switch/release/XY-Switch-1.0.0-workbench-20261017-README.txt) |

SHA-256：

```text
570A5FC354FB1A97D7523D6951EFDF1EA415A9DE4E8FA2CA87117AC2A2AB66E7
```

新产物不同于 2026-09-22 的旧 EXE：旧文件 14,356,992 bytes，SHA-256 为 `86FFD975625AC0D19EB4C805FEEC4B6864C4C7E73BD4A6D2158C3B77DEF9F7AE`。构建缓存中的旧输出被正常编译替换，`release` 目录已有发布文件不作覆盖。

## 2. 缺失工具已补齐

复用现有 Node（v24.15.0）、Rust（rustc 1.98.1 (48a229cea 2026-09-01)）、Cargo（cargo 1.98.1 (797e8a9bc 2026-08-05)）和 MSVC 14.44.35207。原生构建先前缺少 Windows SDK 的库、头文件和资源编译器；本次仅补齐 SDK。

工具目录：`E:\AI tools\WindowsSDK\10.0.26100.9169`。采用 26100 稳定系列以兼容现有工具链；没有将“最新版 EXE”理解成升级全部构建依赖。

| 官方 NuGet 包 | 版本 | 下载大小 |
| --- | --- | ---: |
| Microsoft.Windows.SDK.CPP | 10.0.26100.9169 | 160,512,239 bytes |
| Microsoft.Windows.SDK.CPP.x64 | 10.0.26100.9169 | 53,001,499 bytes |

包从微软官方 NuGet 平台获取，实际下载逐一匹配官方 catalog 提供的 SHA-512 与字节大小，并记录 SHA-256。解压前检查所有路径均落在对应目标目录内。使用的 x64 `rc.exe`、`rcdll.dll`、`mt.exe` 签名状态均为 Valid、签名者为 Microsoft Corporation。

完整来源、下载文件、摘要与工具签名记录见 [SDK installation.json](E:/AI%20tools/WindowsSDK/10.0.26100.9169/installation.json)。下载包和解压工具均保留在用户指定的 E 盘目录。没有安装 SDK 到 C 盘，没有修改全局 PATH、注册表或项目依赖。

## 3. 构建方式与验证

加载现有 VsDevCmd 后，仅在构建进程中补充：

- LIB：`E:\AI tools\WindowsSDK\10.0.26100.9169\x64\c\um\x64` 与 `...\x64\c\ucrt\x64`，并保留 MSVC lib。
- INCLUDE：`...\cpp\c\Include\10.0.26100.0` 下的 ucrt/shared/um/winrt/cppwinrt，并保留 MSVC include。
- RC：`E:\AI tools\WindowsSDK\10.0.26100.9169\cpp\c\bin\10.0.26100.0\x64\rc.exe`。
- Linker：`E:\build tools\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64\link.exe`。

复用本轮已经完成 TypeScript/Vite 构建和生产浏览器验证的 `dist`；其后没有前端源码改动，不重复构建或安装依赖。本次原生命令为：

```powershell
& 'E:\node.js\node.exe' 'E:\XY switch\node_modules\@tauri-apps\cli\tauri.js' build --no-bundle --ci --config '{"build":{"beforeBuildCommand":null}}' -- --offline --locked
```

命令退出码 **0**，release 优化构建耗时 **6 分 43 秒**。完整环境见 [native-build-environment.json](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/native-build-environment.json)；完整过程见 [native-build-sdk.log](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/native-build-sdk.log)。其中保留了未使用导入/死代码告警，以及生成项目库的 linker 信息，未通过自动修改代码来掩盖告警。

构建前后 268 个源码、资源、dist 与配置输入的 SHA-256 一致；Cargo.lock 和 pnpm-lock.yaml 未变化。用 PE 头与 dumpbin 检查了 AMD64 架构、GUI 子系统、1.0.0 版本和导入表；没有依赖单独的 `xy_switch_lib.dll`。这些检查不代表进程已经成功运行。

- [构建输入清单](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/native-build-inputs.json)
- [编译结果](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/native-build-result.json)
- [原生文件验证](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/native-exe-verification.json)
- [PE 头记录](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/native-exe-headers.txt)
- [DLL 导入记录](C:/Users/xxy/.codex/visualizations/2026/10/17/01a14986-ac85-7de2-b3f6-b1036a4eb014/native-exe-dependents.txt)

早期缺链接器、缺 kernel32.lib 和沙箱写入失败的日志均保留。目录写入受限后已通过正式提升完成构建，不改变其历史失败记录。

## 4. 已有功能验证与边界

沿用本聊天前序对同一份应用源码完成的验收，不把它们伪装成本次 SDK 补齐后的重复执行：

| 验证 | 结果 |
| --- | --- |
| TypeScript / Vite production | 通过 |
| 前端单元测试 | 21 文件，149 passed |
| Rust 单元测试 | 前序结果 198 passed、3 ignored；未为打包重跑，无单独日志 |
| 开发 renderer 浏览器 | 13 组通过；隔离 Chromium / IPC fixture |
| 生产 dist 浏览器 | 10 个适用组通过，3 个开发专用宿主不纳入生产组 |
| 原生 release 编译与文件检查 | 本次通过 |
| 生产 EXE 启动 / 真实 Tauri IPC | 未执行 |
| 真实网站与 Key、代理协议实机链路 | 未执行，等待用户提供测试参数 |

细项、截图和生产组排除理由见 [UI 验收报告](E:/XY%20switch/docs/workbench-ui-acceptance-20261017.md)。

## 5. 运行与后续测试

这是单文件 EXE 分发，不是新增安装器，也不等于所有数据写在 EXE 所在目录。运行依赖 WebView2 Runtime；本机已检测到 140.0.3485.94，本轮未升级它。发布文件未携带数据库、Key、测试 profile 或现有 CLI 配置。

生产构建不接受仅供 debug 使用的 `XY_SWITCH_TEST_DATA_DIR` / `XY_SWITCH_TEST_HOME_DIR`。其启动会访问 Windows 的真实应用数据/用户目录，并运行接管恢复逻辑。因此没有仅靠设置这些变量就启动生产 EXE，避免误写个人配置；本次未执行真实上游请求、凭据写入、自启设置或 CLI 接管。

收到用户提供的测试网站、Key（及需要时的模型/协议）后，再单独验证原生启动、真实 IPC、连接测试和代理流式/非流式链路。未验证的功能不因构建成功而自动判为通过。
