# 调试缓存手动删除清单（2026-10-16）

**仅处理下面剩余的 11 项：5 个目录、6 个文件。本次只提供文档，未执行新的删除，也未把旧清理记录改成完成。**

原批准清单共 35 项，其中 24 项已经删除，不需要再次处理。剩余 11 项均位于 `E:\XY switch\src-tauri\target\debug`，截至本次只读检查仍存在。此前删除遇到沙盒 `Access denied`，两次沙盒外执行审核均在进程启动前超时；这不代表已经确认必须使用 Windows 管理员权限。

## 1. 可以删除的精确路径

这些是可重建的 Cargo 调试中间缓存、静态库、链接辅助文件和调试符号。删除后，下一次调试构建需要重建相应内容；删除 PDB 后，旧构建对应的调试符号不再可用。清理不是源码变更，且不应删除下节保留的可运行 EXE/DLL。

| 序号 | 完整绝对路径 | 类型 | 文件数 | 逻辑大小（字节） |
| --- | --- | --- | ---: | ---: |
| 1 | `E:\XY switch\src-tauri\target\debug\incremental` | 目录，删除整个目录 | 4,443 | 6,442,845,775 |
| 2 | `E:\XY switch\src-tauri\target\debug\deps` | 目录，删除整个目录 | 3,368 | 7,969,362,035 |
| 3 | `E:\XY switch\src-tauri\target\debug\build` | 目录，删除整个目录 | 1,004 | 908,657,521 |
| 4 | `E:\XY switch\src-tauri\target\debug\.fingerprint` | 隐藏目录，删除整个目录 | 3,535 | 857,012 |
| 5 | `E:\XY switch\src-tauri\target\debug\examples` | 空目录，删除整个目录 | 0 | 0 |
| 6 | `E:\XY switch\src-tauri\target\debug\libxy_switch_lib.rlib` | 单个文件 | 1 | 430,735,766 |
| 7 | `E:\XY switch\src-tauri\target\debug\xy_switch_lib.dll.exp` | 单个文件 | 1 | 612 |
| 8 | `E:\XY switch\src-tauri\target\debug\xy_switch_lib.dll.lib` | 单个文件 | 1 | 1,680 |
| 9 | `E:\XY switch\src-tauri\target\debug\xy_switch_lib.lib` | 单个文件 | 1 | 1,078,928,780 |
| 10 | `E:\XY switch\src-tauri\target\debug\xy_switch_lib.pdb` | 单个文件 | 1 | 109,359,104 |
| 11 | `E:\XY switch\src-tauri\target\debug\xy_switch.pdb` | 单个文件 | 1 | 205,107,200 |
| **合计** | **只限上述 11 项** | **5 目录 + 6 文件** | **12,356** | **17,145,855,485** |

合计约 **15.968 GiB 逻辑量**。硬链接可能使多个文件名重复计入逻辑大小，因此不能承诺实际释放同样大小的磁盘空间；应看删除前后 E: 实测可用空间的差值，该差值也可能包含同期其他磁盘活动。

## 2. 必须保留的内容

| 路径或范围 | 要求 |
| --- | --- |
| `E:\XY switch\src-tauri\target\debug\xy-switch.exe` | 保留，当前记录大小 30,832,640 字节 |
| `E:\XY switch\src-tauri\target\debug\xy_switch_lib.dll` | 保留，当前记录大小 2,140,160 字节 |
| `E:\XY switch\src-tauri\target\release` | 全部保留 |
| `E:\XY switch\artifacts` | 全部保留，包括验收证据、日志、截图、SQLite 夹具、备份、源码 ZIP、EXE/MSI |
| `E:\XY switch` 下的源码、锁文件、`.git`、`node_modules` 及工具链 | 不在本次删除范围，保留 |
| `E:\XY switch\src-tauri\target\debug` 内未列入第 1 节的其他内容 | 保留 |

**`.dll.exp` 和 `.dll.lib` 可删，但 `.dll` 本身必须保留。**

不要删除整个 `E:\XY switch\src-tauri\target\debug`、整个 `E:\XY switch\src-tauri\target` 或整个工作区；不要使用 `cargo clean`、通配符批量删除或重跑原 35 项清理脚本。旧残余续跑脚本绑定的是文档更新前的保护哈希，已过期且会拒绝执行，不作为本次手动操作入口。

## 3. 用文件资源管理器手动删除

1. 结束 Cargo/Tauri 开发构建，关闭正在使用这些调试产物的应用和调试器。删除至核对结束期间不要启动新的构建，也不要运行会重新生成缓存的命令。
2. 在文件资源管理器地址栏输入 `E:\XY switch\src-tauri\target\debug`。
3. 打开“隐藏的项目”和“文件扩展名”，以便看到 `.fingerprint` 并区分 `.dll`、`.dll.exp`、`.dll.lib`。
4. 逐项按第 1 节清单选中 **5 个目录 + 6 个文件**，尤其不要选中 `xy-switch.exe` 或 `xy_switch_lib.dll`。
5. 删除选中项。普通 Delete 通常先放入回收站，空间可能尚未归还；确认仅选中上述 11 项后，可用 Shift+Delete 永久删除。不要为了这次清理而清空回收站中其他无关内容。
6. 按第 5 节核对。如有拒绝访问或文件占用，先看第 6 节，不要扩大删除范围。

## 4. 可选：精确白名单 PowerShell 命令

下面是独立命令，不依赖旧续跑脚本。使用 **PowerShell 7（pwsh）**；本机已验证的可执行文件为 `C:\Users\xxy\.cache\codex-runtimes\codex-primary-runtime\dependencies\native\powershell\pwsh.exe`。

- **默认 `$Apply = $false`，只做检查和 `-WhatIf` 预览，不删除。**
- 确认预览仅包含上表路径后，在同一代码块中把 `$Apply = $false` 改为 `$Apply = $true`，再运行完整代码块进行实际删除。实际删除不进入回收站。
- 白名单固定为 5 个目录和 6 个文件，只有目录使用 `-Recurse`；所有删除使用 `-LiteralPath`，没有通配符。
- 先检查构建/应用进程、绝对路径、目录类型和重解析点（符号链接/目录联接等），并核对仍存在目标的文件数与逻辑大小。任何检查失败都在删除前停止；已经不存在的目标只跳过。
- 不要为绕过检查而修改路径或放宽范围。检查不能消除外部进程在检查后启动的竞态，操作期间仍需保持构建停止。

```powershell
$ErrorActionPreference = 'Stop'
$Apply = $false  # 仅预览；确认后改为 $true，并重新运行完整代码块
if ($PSVersionTable.PSVersion.Major -lt 7) {
    throw '请使用 PowerShell 7（pwsh）运行此代码块。'
}

$debugRoot = 'E:\XY switch\src-tauri\target\debug'
$releaseRoot = 'E:\XY switch\src-tauri\target\release'
$reparseFlag = [System.IO.FileAttributes]::ReparsePoint
$targets = @(
    [pscustomobject]@{ Path = 'E:\XY switch\src-tauri\target\debug\incremental'; IsDirectory = $true; Files = 4443; Bytes = 6442845775L }
    [pscustomobject]@{ Path = 'E:\XY switch\src-tauri\target\debug\deps'; IsDirectory = $true; Files = 3368; Bytes = 7969362035L }
    [pscustomobject]@{ Path = 'E:\XY switch\src-tauri\target\debug\build'; IsDirectory = $true; Files = 1004; Bytes = 908657521L }
    [pscustomobject]@{ Path = 'E:\XY switch\src-tauri\target\debug\.fingerprint'; IsDirectory = $true; Files = 3535; Bytes = 857012L }
    [pscustomobject]@{ Path = 'E:\XY switch\src-tauri\target\debug\examples'; IsDirectory = $true; Files = 0; Bytes = 0L }
    [pscustomobject]@{ Path = 'E:\XY switch\src-tauri\target\debug\libxy_switch_lib.rlib'; IsDirectory = $false; Files = 1; Bytes = 430735766L }
    [pscustomobject]@{ Path = 'E:\XY switch\src-tauri\target\debug\xy_switch_lib.dll.exp'; IsDirectory = $false; Files = 1; Bytes = 612L }
    [pscustomobject]@{ Path = 'E:\XY switch\src-tauri\target\debug\xy_switch_lib.dll.lib'; IsDirectory = $false; Files = 1; Bytes = 1680L }
    [pscustomobject]@{ Path = 'E:\XY switch\src-tauri\target\debug\xy_switch_lib.lib'; IsDirectory = $false; Files = 1; Bytes = 1078928780L }
    [pscustomobject]@{ Path = 'E:\XY switch\src-tauri\target\debug\xy_switch_lib.pdb'; IsDirectory = $false; Files = 1; Bytes = 109359104L }
    [pscustomobject]@{ Path = 'E:\XY switch\src-tauri\target\debug\xy_switch.pdb'; IsDirectory = $false; Files = 1; Bytes = 205107200L }
)
$keepFiles = @(
    'E:\XY switch\src-tauri\target\debug\xy-switch.exe'
    'E:\XY switch\src-tauri\target\debug\xy_switch_lib.dll'
)

function Assert-NoBuildProcess {
    $names = @('cargo', 'rustc', 'rustdoc', 'link', 'lld-link', 'rust-lld', 'cl', 'tauri', 'xy-switch')
    $busy = @(Get-Process -Name $names -ErrorAction SilentlyContinue)
    if ($busy.Count -gt 0) {
        $details = ($busy | ForEach-Object { '{0}(PID {1})' -f $_.ProcessName, $_.Id }) -join ', '
        throw "请先结束构建/关闭相关应用，再重试：$details"
    }
}

function Assert-ExactPath([string]$Path, [bool]$IsDirectory) {
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    if ($item.PSIsContainer -ne $IsDirectory) { throw "路径类型不符：$Path" }
    if (($item.Attributes -band $reparseFlag) -ne 0) { throw "发现重解析点，停止：$Path" }
    $resolved = (Resolve-Path -LiteralPath $Path -ErrorAction Stop).ProviderPath
    if ($resolved -ine $Path) { throw "绝对路径解析发生变化，停止：$Path -> $resolved" }
    return $item
}

Assert-NoBuildProcess
$ancestors = @(
    'E:\XY switch'
    'E:\XY switch\src-tauri'
    'E:\XY switch\src-tauri\target'
    $debugRoot
    $releaseRoot
)
foreach ($ancestor in $ancestors) {
    Assert-ExactPath $ancestor $true | Out-Null
}
if ($targets.Count -ne 11 -or @($targets | Where-Object IsDirectory).Count -ne 5) {
    throw '白名单数量不符，停止。'
}
$keepHashes = @{}
foreach ($path in $keepFiles) {
    Assert-ExactPath $path $false | Out-Null
    $keepHashes[$path] = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
}

# 全部预检查通过后，才允许进入删除阶段。
$ready = @(foreach ($target in $targets) {
    $canonical = [System.IO.Path]::GetFullPath($target.Path)
    if ($canonical -ine $target.Path -or
        [System.IO.Path]::GetDirectoryName($canonical) -ine $debugRoot) {
        throw "目标不再是指定 debug 目录的直接子项，停止：$($target.Path)"
    }
    if (-not (Test-Path -LiteralPath $target.Path)) {
        Write-Host "已不存在，跳过：$($target.Path)"
        continue
    }
    $item = Assert-ExactPath $target.Path $target.IsDirectory
    if ($target.IsDirectory) {
        $children = @(Get-ChildItem -LiteralPath $target.Path -Recurse -Force -ErrorAction Stop)
        $redirects = @($children | Where-Object { ($_.Attributes -band $reparseFlag) -ne 0 })
        if ($redirects.Count -gt 0) { throw "目标内部存在重解析点，停止：$($target.Path)" }
        $files = @($children | Where-Object { -not $_.PSIsContainer })
        $fileCount = $files.Count
        $bytes = [long](($files | Measure-Object -Property Length -Sum).Sum)
    } else {
        $fileCount = 1
        $bytes = [long]$item.Length
    }
    if ($fileCount -ne $target.Files -or $bytes -ne $target.Bytes) {
        throw "文件数或逻辑大小与清单不符，停止并重新核对：$($target.Path)"
    }
    $target
})

if (-not $Apply) {
    foreach ($target in $ready) {
        if ($target.IsDirectory) {
            Remove-Item -LiteralPath $target.Path -Recurse -Force -WhatIf
        } else {
            Remove-Item -LiteralPath $target.Path -Force -WhatIf
        }
    }
    Write-Host '预览结束，未删除。确认后把 $Apply 改为 $true，再运行完整代码块。'
} else {
    Assert-NoBuildProcess
    $freeBefore = [System.IO.DriveInfo]::new('E:\').AvailableFreeSpace
    foreach ($target in $ready) {
        if ($target.IsDirectory) {
            Remove-Item -LiteralPath $target.Path -Recurse -Force -ErrorAction Stop
        } else {
            Remove-Item -LiteralPath $target.Path -Force -ErrorAction Stop
        }
    }
    $remaining = @($targets | Where-Object { Test-Path -LiteralPath $_.Path })
    if ($remaining.Count -gt 0) { throw '仍有清单目标存在，请逐项核对，不要扩大删除范围。' }
    foreach ($path in $keepFiles) {
        Assert-ExactPath $path $false | Out-Null
        if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $keepHashes[$path]) {
            throw "保留文件发生变化，请停止后续操作并检查：$path"
        }
    }
    Assert-ExactPath $releaseRoot $true | Out-Null
    $freeAfter = [System.IO.DriveInfo]::new('E:\').AvailableFreeSpace
    Write-Host '11 项均已不存在；保留 EXE/DLL 的 SHA-256 未变；release 目录仍在。'
    Write-Host ('E: 实测可用空间增量：{0:N0} 字节（{1:N3} GiB）；可能包含同期磁盘活动。' -f
        ($freeAfter - $freeBefore), (($freeAfter - $freeBefore) / 1GB))
}
```

代码块仅做了静态语法与白名单检查，**未在本轮执行（包括预览分支）**。它不修改 ACL/所有权、不自动结束进程、不重跑构建或验收，也不改写旧清理证据。

## 5. 删除后的核对

- 第 1 节列出的 11 个路径都不存在；已经不存在的项目不需要重删。
- `E:\XY switch\src-tauri\target\debug\xy-switch.exe` 和 `E:\XY switch\src-tauri\target\debug\xy_switch_lib.dll` 仍在。
- `E:\XY switch\src-tauri\target\release` 及其内容保留；源码、`E:\XY switch\artifacts` 等保留范围不变。
- 若运行上面的实际删除分支，还会对比保留 EXE/DLL 删除前后的 SHA-256，并输出 E: 可用空间增量。脚本只检查 release 根目录存在，不是对全部 release 文件重新做全量哈希验收。
- 删除过程如果部分成功后遇到报错，已删除项不会自动恢复；排除占用后，仍按同一 11 项白名单核对或重试，不重跑原 35 项计划。
- 调试缓存日后重新生成是正常现象，不代表这次删除失败；不要为核对清理效果立即运行调试构建。

**此文档尚不能证明用户已完成手动删除。原清理记录继续表示部分完成，保留历史真实性。**

## 6. 拒绝访问或文件占用时

1. 先关闭构建终端、Tauri 开发任务、XY Switch 调试程序和相关调试器，确认没有 Cargo/rustc/link/cl 等构建进程。不要强行关闭不相关程序。
2. 若普通终端或资源管理器仍拒绝访问，再由你按需打开“以管理员身份运行”的 PowerShell 7，只操作第 1 节白名单。
3. 如果仍失败，记录完整报错和具体路径后停止。不要对整个工作区执行 `takeown`/ACL 重置，不要用更宽的删除命令绕过错误。

## 7. 证据与范围来源

| 本地证据 | 说明 | 对应操作 |
| --- | --- | --- |
| [残余 11 项清理计划](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup/approved-execution-20261016/resume-permission-retry/cleanup-plan.json>) | 记录精确路径、目录/文件类型、文件数及逻辑字节数 | 仅处理第 1 节和代码块中的 11 项 |
| [原部分执行结果](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup/cleanup-result.json>) | 原清理未全部完成，`completed=false` | 不把历史结果改成成功，不重删已成功项 |
| [批准后完成摘要](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup/approved-execution-20261016/completion-summary.json>) | 已删 24/35 项；实测 E: 空间增量 16,099,254,272 字节，约 14.994 GiB；原保护检查 5,915 文件无变化 | 保留原清理成果与验收证据 |
| [执行审核超时记录](<E:/XY switch/artifacts/model-audit-20261009/logs/application-acceptance-20261016/debug-cache-cleanup/approved-execution-20261016/resume-permission-review-timeouts.json>) | 两次审核超时，沙盒外进程未启动 | 剩余项交给用户手动操作，不等同于已经删除 |

本次只新增独立的手动操作文档，不更新旧交接快照、保护哈希或验收结论。
