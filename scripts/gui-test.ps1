# GUI 单测脚本（`scripts/gui-test.ps1`）
#
# 为什么需要它：GUI 的正确性有三层，`cargo test` 只覆盖前两层。
#   1. **逻辑层** —— `metis-ui-egui` 的无头渲染测试（`cargo test -p metis-ui-egui`）
#   2. **装配层** —— `metis-gui` 的启动守卫 / 窗口参数 / 版本旗标（`cargo test -p metis-gui`）
#   3. **真机层** —— 窗口真的能开、真的画出了东西、截图不是全白
# 第 3 层只有真的起一个窗口才能验，而且**必须用截图字节数做断言**：
# 控件被挤出可视区不会 panic，测试不会红（`docs/PITFALLS.md` G37/G38）。
#
# 用法：
#   pwsh scripts/gui-test.ps1              # 跑全部三层
#   pwsh scripts/gui-test.ps1 -SkipLaunch  # 只跑前两层（无显示器的 CI）
#
# 退出码：0 = 全部通过，1 = 有失败。

[CmdletBinding()]
param(
    [switch]$SkipLaunch,
    [string]$OutDir = "target/gui-test"
)

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

# ─────────────────────────────────────────────────────────────
# 第 1 层：逻辑（无头渲染）
# ─────────────────────────────────────────────────────────────
Write-Host "`n▶ 第 1 层：ui-egui 无头渲染测试" -ForegroundColor Cyan
& cargo test -q -p metis-ui-egui 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) {
    Fail "cargo test -p metis-ui-egui 失败"
} else {
    Ok "无头渲染与交互测试通过"
}

# 一致性（I9）：CLI 与 GUI 走同一条路
& cargo test -q -p metis-ui-egui --test consistency 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) {
    Fail "I9 一致性测试失败（CLI 与 GUI 可能走了不同的实现）"
} else {
    Ok "CLI/GUI 一致性（I9）通过"
}

# ─────────────────────────────────────────────────────────────
# 第 2 层：装配（二进制自己的逻辑）
# ─────────────────────────────────────────────────────────────
Write-Host "`n▶ 第 2 层：metis-gui 装配测试" -ForegroundColor Cyan
& cargo test -q -p metis-gui 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) {
    Fail "cargo test -p metis-gui 失败"
} else {
    Ok "启动守卫 / 窗口参数 / 版本旗标测试通过"
}

& cargo build -q -p metis-gui 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) {
    Fail "metis-gui 构建失败"
    Write-Host "`n共 $($failures.Count) 项失败" -ForegroundColor Red
    exit 1
}

$exe = Join-Path $repo "target/debug/metis-gui.exe"
if (-not (Test-Path $exe)) { $exe = Join-Path $repo "target/debug/metis-gui" }
if (-not (Test-Path $exe)) {
    Fail "找不到 metis-gui 可执行文件"
    exit 1
}

# 版本旗标：打包脚本依赖它，必须不起窗口就返回
$versionOut = & $exe --version 2>&1
if ($LASTEXITCODE -eq 0 -and $versionOut -match '^metis-gui \d+\.\d+\.\d+') {
    Ok "--version 立即返回且格式正确（$versionOut）"
} else {
    Fail "--version 输出不符合预期：$versionOut"
}

if ($SkipLaunch) {
    Write-Host "`n已跳过第 3 层（-SkipLaunch）" -ForegroundColor Yellow
} else {
    # ─────────────────────────────────────────────────────────
    # 第 3 层：真机开窗 + 截图断言
    # ─────────────────────────────────────────────────────────
    Write-Host "`n▶ 第 3 层：真机开窗与截图" -ForegroundColor Cyan

    $outPath = Join-Path $repo $OutDir
    New-Item -ItemType Directory -Force -Path $outPath | Out-Null

    Add-Type -AssemblyName System.Drawing
    Add-Type @"
using System;
using System.Runtime.InteropServices;
public class MetisGuiProbe {
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool repaint);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    public struct RECT { public int Left, Top, Right, Bottom; }
}
"@

    # 用独立的临时 home，绝不碰真实 ~/.metis
    $tempHome = Join-Path ([System.IO.Path]::GetTempPath()) ("metis-gui-test-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
    New-Item -ItemType Directory -Force -Path $tempHome | Out-Null
    $env:METIS_HOME = $tempHome

    # 造一份人设，让界面有内容可画（空界面截图会误导判断）
    $metisExe = Join-Path $repo "target/debug/metis.exe"
    if (Test-Path $metisExe) {
        & $metisExe persona guitest --body "GUI 测试用正文。" --name "GUI 测试" 2>&1 | Out-Null
        & $metisExe use guitest 2>&1 | Out-Null
    }

    $cases = @(
        @{ Name = "default"; Width = 1280; Height = 860; Dark = $false },
        @{ Name = "min";     Width = 900;  Height = 620; Dark = $false },
        @{ Name = "dark";    Width = 1280; Height = 860; Dark = $true }
    )

    foreach ($case in $cases) {
        if ($case.Dark) { $env:METIS_DARK = "1" } else { Remove-Item Env:\METIS_DARK -ErrorAction SilentlyContinue }

        # eframe 的 storage 存在 AppData 下：每次都要清，否则上次的深色偏好会覆盖 METIS_DARK
        $storage = Join-Path $env:APPDATA "dev.metis.console\data"
        if (Test-Path $storage) { Remove-Item $storage -Force -ErrorAction SilentlyContinue }

        $proc = Start-Process -FilePath $exe -PassThru -WindowStyle Normal
        Start-Sleep -Seconds 6
        $proc.Refresh()

        if ($proc.HasExited) {
            Fail "[$($case.Name)] 窗口进程提前退出（code=$($proc.ExitCode)）—— 这是最糟的失败形态"
            continue
        }
        if ($proc.MainWindowHandle -eq [IntPtr]::Zero) {
            Fail "[$($case.Name)] 进程活着但拿不到窗口句柄"
            $proc.Kill(); $proc.WaitForExit()
            continue
        }

        [MetisGuiProbe]::MoveWindow($proc.MainWindowHandle, 0, 0, $case.Width, $case.Height, $true) | Out-Null
        Start-Sleep -Seconds 2

        $rect = New-Object MetisGuiProbe+RECT
        [MetisGuiProbe]::GetWindowRect($proc.MainWindowHandle, [ref]$rect) | Out-Null
        $w = $rect.Right - $rect.Left
        $h = $rect.Bottom - $rect.Top

        if ($w -lt 100 -or $h -lt 100) {
            Fail "[$($case.Name)] 窗口尺寸异常：${w}x${h}"
            $proc.Kill(); $proc.WaitForExit()
            continue
        }

        # `PrintWindow(..., PW_RENDERFULLCONTENT)` 才是可靠的：
        # 这台机器上 `CopyFromScreen` 抓出来是全白（`docs/PITFALLS.md` G27）。
        $bmp = New-Object System.Drawing.Bitmap $w, $h
        $graphics = [System.Drawing.Graphics]::FromImage($bmp)
        $hdc = $graphics.GetHdc()
        [MetisGuiProbe]::PrintWindow($proc.MainWindowHandle, $hdc, 2) | Out-Null
        $graphics.ReleaseHdc($hdc)
        $file = Join-Path $outPath "$($case.Name).png"
        $bmp.Save($file, [System.Drawing.Imaging.ImageFormat]::Png)
        $graphics.Dispose(); $bmp.Dispose()

        $proc.Kill(); $proc.WaitForExit()

        $bytes = (Get-Item $file).Length
        # 全白/全黑截图的经验值约 5 KB；真有内容的界面 150 KB 以上。
        # 这个阈值是 `PITFALLS.md` G27 记下来的实测结论。
        if ($bytes -lt 20000) {
            Fail "[$($case.Name)] 截图只有 $bytes 字节，界面很可能是空白（阈值 20000）"
        } else {
            Ok "[$($case.Name)] ${w}x${h}，截图 $bytes 字节 → $file"
        }
    }

    # 深色与浅色必须**不一样**：一样就说明 token 没生效（`PITFALLS.md` G51）
    $lightFile = Join-Path $outPath "default.png"
    $darkFile = Join-Path $outPath "dark.png"
    if ((Test-Path $lightFile) -and (Test-Path $darkFile)) {
        $lightHash = (Get-FileHash $lightFile -Algorithm MD5).Hash
        $darkHash = (Get-FileHash $darkFile -Algorithm MD5).Hash
        if ($lightHash -eq $darkHash) {
            Fail "深色与浅色截图完全相同（MD5 $lightHash）—— 主题没有生效"
        } else {
            Ok "深色与浅色确实不同（浅 $($lightHash.Substring(0,8)) / 深 $($darkHash.Substring(0,8)))"
        }
    }

    Remove-Item Env:\METIS_DARK -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force $tempHome -ErrorAction SilentlyContinue

    # 启动守卫会在 `~/.metis/starting` 留一个标记；脚本是 kill 掉进程的，
    # `opened()` 没机会执行。不清掉的话，下次真实启动会误报"上次崩溃"。
    $defaultHome = Join-Path $env:USERPROFILE ".metis"
    $staleMarker = Join-Path $defaultHome "starting"
    if (Test-Path $staleMarker) {
        Remove-Item $staleMarker -Force -ErrorAction SilentlyContinue
        # 如果这个目录本来是空的（只是因为我们的标记才存在），一并删掉。
        $left = Get-ChildItem $defaultHome -Force -ErrorAction SilentlyContinue
        if (-not $left) { Remove-Item $defaultHome -Recurse -Force -ErrorAction SilentlyContinue }
        Ok "清理了启动守卫残留标记"
    }
}

# ─────────────────────────────────────────────────────────────
Write-Host ""
if ($failures.Count -eq 0) {
    Write-Host "✓ GUI 测试全部通过" -ForegroundColor Green
    exit 0
} else {
    Write-Host "✗ 共 $($failures.Count) 项失败：" -ForegroundColor Red
    foreach ($f in $failures) { Write-Host "    $f" -ForegroundColor Red }
    exit 1
}
