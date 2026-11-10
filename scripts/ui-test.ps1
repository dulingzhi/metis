# UI 自动化测试（`scripts/ui-test.ps1`）
#
# 用 `agent-desktop` 的 **无障碍树** 驱动真实窗口：点按钮、切屏幕、开命令面板，
# 每一步都**回读状态**验证，而不是"命令返回 ok 就算过"。
#
# 与另外两套测试的分工：
#
# | 层 | 工具 | 证明什么 |
# |---|---|---|
# | 逻辑 | `cargo test -p metis-ui-egui` | 组件与屏幕渲染出东西、状态派生正确 |
# | 视觉 | `cargo xtask snapshot` | 像素与基线一致（改 2px 就红） |
# | **交互** | **本脚本** | **控件真的能点、点了真的会变、键盘真的生效** |
#
# 第三层是前两层都覆盖不到的：渲染测试里的"点击"是合成事件，
# 快照测试根本不交互。只有驱动真实窗口才能确认"用户点得动"。
#
# 用法：
#   pwsh scripts/ui-test.ps1
#   pwsh scripts/ui-test.ps1 -KeepOpen   # 失败时留着窗口方便人工看
#
# 退出码：0 = 全过，1 = 有失败。

[CmdletBinding()]
param([switch]$KeepOpen)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

$failures = @()
function Fail([string]$message) {
    $script:failures += $message
    Write-Host "  ✗ $message" -ForegroundColor Red
}
function Ok([string]$message) {
    Write-Host "  ✓ $message" -ForegroundColor Green
}

# ── 前置检查 ────────────────────────────────────────────────
Write-Host "`n▶ 前置检查" -ForegroundColor Cyan
$status = & agent-desktop status 2>&1 | Out-String
if ($status -notmatch '"ok":true') {
    Write-Host "agent-desktop 不可用：$status" -ForegroundColor Red
    exit 1
}
if ($status -notmatch '"accessibility":\{"state":"granted"\}') {
    Write-Host "缺 accessibility 权限" -ForegroundColor Red
    exit 1
}
Ok "agent-desktop 可用（accessibility 已授予）"

& cargo build -q -p metis 2>&1 | Out-Null
$exe = Join-Path $repo "target/debug/metis.exe"
$cli = Join-Path $repo "target/debug/metis.exe"
if (-not (Test-Path $exe)) { Fail "找不到 metis"; exit 1 }

# ── 辅助函数 ────────────────────────────────────────────────

# 取一次快照的原文。
#
# 用**窗口 ID**：进程可能同时拥有多个顶层窗口，`--app` 的指向不稳定。
function Snapshot {
    & agent-desktop snapshot --window-id $script:metisWindowId -i --compact 2>&1 | Out-String
}

# 把 GUI 窗口提到前台。
#
# `press --app` 只把按键送到**前台**窗口；键盘用例前必须聚焦。
function FocusApp {
    & agent-desktop focus-window --window-id $script:metisWindowId 2>&1 | Out-Null
    Start-Sleep -Milliseconds 200
}

# 按 Ctrl+K 打开命令面板。
#
# 按键只送前台窗口，偶尔会被别的窗口抢走（本机多显示器 + 后台任务时实测）。
# **只在面板仍然关着时重试一次**：如果第一次其实已经打开，再按一次会把它关掉，
# 所以必须先读一次状态。
function PressCtrlK {
    FocusApp
    & agent-desktop press ctrl+k --app "metis" 2>&1 | Out-Null
    Start-Sleep -Milliseconds 600
    if ((Snapshot) -notmatch '输入动作名') {
        FocusApp
        & agent-desktop press ctrl+k --app "metis" 2>&1 | Out-Null
    }
}

# 在快照里找一个具名节点的 ref 与 role。
function FindNode([string]$snapshot, [string]$name) {
    $m = [regex]::Match($snapshot, "\{[^{}]*""name"":""$([regex]::Escape($name))""[^{}]*\}")
    if (-not $m.Success) { return $null }
    return @{
        Ref  = ([regex]::Match($m.Value, '"ref_id":"(@[^"]+)"')).Groups[1].Value
        Role = ([regex]::Match($m.Value, '"role":"([^"]+)"')).Groups[1].Value
    }
}

# 点一个具名控件。返回是否点成功。
function ClickNamed([string]$name) {
    FocusApp
    $snap = Snapshot
    $node = FindNode $snap $name
    if (-not $node) { return $false }
    # PowerShell 会把裸 @token 当 splatting，必须加引号。
    $result = & agent-desktop click "$($node.Ref)" 2>&1 | Out-String
    return $result -match '"ok":true'
}

# 等某个文字出现（最多 N 秒）。返回是否出现。
function WaitForText([string]$text, [int]$seconds = 5) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        if ((Snapshot) -match [regex]::Escape($text)) { return $true }
        Start-Sleep -Milliseconds 300
    }
    return $false
}

# 等某段文字消失。
function WaitForTextGone([string]$text, [int]$seconds = 5) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        if ((Snapshot) -notmatch [regex]::Escape($text)) { return $true }
        Start-Sleep -Milliseconds 300
    }
    return $false
}

# ── 起一个干净的应用实例 ────────────────────────────────────
$tempHome = Join-Path ([System.IO.Path]::GetTempPath()) ("metis-ui-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
New-Item -ItemType Directory -Force -Path $tempHome | Out-Null
$env:METIS_HOME = $tempHome

# 固定的测试数据：两份人设，选其中一份。
& $cli persona alpha --body "用简体中文协作。" --name "甲人设" 2>&1 | Out-Null
& $cli persona beta --body "严格按用户要求。" --name "乙人设" 2>&1 | Out-Null
& $cli use alpha 2>&1 | Out-Null

Get-Process metis -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1
# 无参数 → GUI（同一个进程，不再拉起子进程）。
$app = Start-Process -FilePath $exe -PassThru
Start-Sleep -Seconds 8

if ($app.HasExited) {
    Fail "窗口进程提前退出（最糟的失败形态）"
    $failures | ForEach-Object { Write-Host "    $_" -ForegroundColor Red }
    exit 1
}
if ($app.MainWindowHandle -eq [IntPtr]::Zero) {
    Fail "进程活着但拿不到窗口"
    $app | Stop-Process -Force
    exit 1
}
Ok "窗口已打开"

# 钉住 GUI 窗口 ID：后续快照与聚焦都用这个 ID（进程可能有多个顶层窗口）。
$win = (agent-desktop list-windows 2>&1 | ConvertFrom-Json).data |
    Where-Object { $_.pid -eq $app.Id -and $_.title -eq 'Metis' } |
    Select-Object -First 1
if (-not $win) {
    Fail "找不到 GUI 窗口（进程 $($app.Id)）"
    $app | Stop-Process -Force
    exit 1
}
$script:metisWindowId = $win.id

# `press --app` 只把按键送到**前台**窗口：先把 GUI 窗口提到前台。
FocusApp
Start-Sleep -Milliseconds 300

try {
    # ── 用例 1：启动时命令面板是关着的 ──────────────────────
    Write-Host "`n▶ 命令面板" -ForegroundColor Cyan
    $snap = Snapshot
    if ($snap -match '输入动作名') {
        Fail "启动时命令面板就打开了（应该是关着的）"
    } else {
        Ok "启动时命令面板关着"
    }

    # ── 用例 2：点顶栏入口能打开 ────────────────────────────
    if (-not (ClickNamed "搜索动作…  Ctrl+K")) {
        # 名字里可能有空格差异，退一步用正则找
        $node = FindNode $snap "搜索动作…  Ctrl+K"
        Fail "点不到顶栏的搜索入口（ref=$($node.Ref)）"
    } elseif (WaitForText "输入动作名") {
        Ok "点顶栏入口 → 面板打开"
    } else {
        Fail "点了入口但面板没出现"
    }

    # ── 用例 3：Esc 能关掉（这是用户报过的 bug）─────────────
    FocusApp
    & agent-desktop press escape --app "metis" 2>&1 | Out-Null
    if (WaitForTextGone "输入动作名") {
        Ok "Esc → 面板关闭"
    } else {
        Fail "Esc 关不掉面板（用户报过的 bug 复发了）"
    }

    # ── 用例 4：Ctrl+K 能重新打开 ──────────────────────────
    PressCtrlK
    if (WaitForText "输入动作名") {
        Ok "Ctrl+K → 面板打开"
    } else {
        Fail "Ctrl+K 打不开面板"
    }
    FocusApp
    & agent-desktop press escape --app "metis" 2>&1 | Out-Null
    WaitForTextGone "输入动作名" | Out-Null

    # ── 用例 5：侧栏四项都是可点击的按钮 ───────────────────
    Write-Host "`n▶ 侧栏导航" -ForegroundColor Cyan
    $snap = Snapshot
    foreach ($label in @('控制台', '人设库', '验证与诊断', '关于')) {
        $node = FindNode $snap $label
        if (-not $node) {
            Fail "侧栏「$label」在无障碍树里不可见"
        } elseif ($node.Role -ne 'button') {
            Fail "侧栏「$label」的 role 是 $($node.Role)，应为 button"
        } else {
            Ok "侧栏「$label」是 button 且可寻址"
        }
    }

    # ── 用例 6：点「人设库」真的会切屏 ──────────────────────
    if (ClickNamed "人设库") {
        if (WaitForText "全部人设") {
            Ok "点「人设库」→ 屏幕切换（见到「全部人设」）"
        } else {
            Fail "点了「人设库」但内容没变"
        }
    } else {
        Fail "点不到「人设库」"
    }

    # ── 用例 7：切到「关于」并看到版本号 ───────────────────
    if (ClickNamed "关于") {
        if (WaitForText "备份占用") {
            Ok "点「关于」→ 见到「备份占用」"
        } else {
            Fail "点了「关于」但没见到备份那一块"
        }
    } else {
        Fail "点不到「关于」"
    }

    # ── 用例 8：点回「控制台」 ─────────────────────────────
    if (ClickNamed "控制台") {
        if (WaitForText "人设与模式") {
            Ok "点「控制台」→ 见到「人设与模式」"
        } else {
            Fail "点「控制台」但没见到核心卡片"
        }
    } else {
        Fail "点不到「控制台」"
    }

    # ── 用例 9：主题切换按钮的文字会跟着状态变 ──────────────
    #
    # 按钮显示的是**"切到哪"**而不是"当前是什么"：深色时显示「浅色」。
    # 所以不能写死找「深色」—— 深色开关本身会被持久化，上一次运行的残留
    # 会让这次一启动就是深色。按当前实际状态找。
    Write-Host "`n▶ 主题切换" -ForegroundColor Cyan
    $snap = Snapshot
    if ($snap -match '"name":"深色"') {
        $from = "深色"; $to = "浅色"
    } elseif ($snap -match '"name":"浅色"') {
        $from = "浅色"; $to = "深色"
    } else {
        $from = $null; $to = $null
        Fail "顶栏找不到主题切换按钮"
    }
    if ($from) {
        if (ClickNamed $from) {
            if (WaitForText $to) {
                Ok "点「$from」→ 按钮变成「$to」"
            } else {
                Fail "点了「$from」但按钮文字没变成「$to」"
            }
            # 切回去，别让测试改变持久化偏好。
            ClickNamed $to | Out-Null
            WaitForText $from | Out-Null
        } else {
            Fail "点不到主题切换按钮「$from」"
        }
    }

    # ── 用例 10：命令面板执行一个动作后会关闭 ───────────────
    Write-Host "`n▶ 面板执行动作" -ForegroundColor Cyan
    PressCtrlK
    if (WaitForText "输入动作名") {
        & agent-desktop press enter --app "metis" 2>&1 | Out-Null
        if (WaitForTextGone "输入动作名") {
            Ok "面板里按 Enter → 执行并关闭"
        } else {
            Fail "面板里按 Enter 后没关闭"
        }
    } else {
        Fail "Ctrl+K 没打开面板，无法测试 Enter"
    }
}
finally {
    if (-not $KeepOpen -or $failures.Count -eq 0) {
        Get-Process metis -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
        Remove-Item -Recurse -Force $tempHome -ErrorAction SilentlyContinue
        # GUI 的启动守卫会留一个标记；不清掉下次真启动会误报"上次崩溃"。
        $marker = Join-Path $env:USERPROFILE ".metis\starting"
        if (Test-Path $marker) { Remove-Item $marker -Force -ErrorAction SilentlyContinue }
        $left = Get-ChildItem (Join-Path $env:USERPROFILE ".metis") -Force -ErrorAction SilentlyContinue
        if (-not $left) { Remove-Item (Join-Path $env:USERPROFILE ".metis") -Recurse -Force -ErrorAction SilentlyContinue }
    } else {
        Write-Host "`n窗口保留在 PID $($app.Id)（-KeepOpen）" -ForegroundColor Yellow
    }
}

Write-Host ""
if ($failures.Count -eq 0) {
    Write-Host "✓ UI 自动化测试全部通过" -ForegroundColor Green
    exit 0
} else {
    Write-Host "✗ 共 $($failures.Count) 项失败：" -ForegroundColor Red
    $failures | ForEach-Object { Write-Host "    $_" -ForegroundColor Red }
    exit 1
}
