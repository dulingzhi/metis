# 打发布包（`scripts/package.ps1`）。
#
# 产物（都在 `dist/` 下）：
#   metis-<版本>-windows-x64.zip      便携包：单个 metis.exe（CLI + GUI）+ LICENSE + README
#   metis-<版本>-windows-x64.zip.sha256
#
# 便携包（zip）而不是安装器，理由：
# - 不写注册表、不需要管理员权限，用户解压即用；唯一例外：GUI 启动时若启用
#   接管命令且 shim 目录不在 PATH，会写**用户级** PATH（HKCU\Environment）；
# - 卸载就是删目录（但**撤接管要单独做**，见下面的提醒）；
# - 安装器（MSI/Inno）需要额外的工具链与签名，等真有用户要求再说。
#
# 用法：
#   pwsh scripts/package.ps1              # 用 Cargo.toml 里的版本
#   pwsh scripts/package.ps1 -Version 0.2.0

[CmdletBinding()]
param(
    # 覆盖版本号（默认从 Cargo.toml 读）。CI 里由 tag 提供。
    [string]$Version,
    # 跳过构建，直接打包已有的 target/release。
    [switch]$NoBuild
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

function Fail([string]$message) {
    Write-Host "✗ $message" -ForegroundColor Red
    exit 1
}

# ── 版本 ────────────────────────────────────────────────────
# **唯一真相**是 Cargo.toml；这里只读，不另存一份（M7.3 就是为此存在的）。
if (-not $Version) {
    $line = Select-String -Path "Cargo.toml" -Pattern '^version = "(.+)"' | Select-Object -First 1
    if (-not $line) { Fail "Cargo.toml 里找不到 version" }
    $Version = [regex]::Match($line.Line, '"(.+)"').Groups[1].Value
}
Write-Host "▶ 打包 Metis $Version" -ForegroundColor Cyan

# ── 构建 ────────────────────────────────────────────────────
if (-not $NoBuild) {
    & cargo build --release -p metis
    if ($LASTEXITCODE -ne 0) { Fail "构建失败" }
}

# 单可执行文件：CLI 与桌面控制台是同一个 exe（M10.7）。
$cli = "target/release/metis.exe"
if (-not (Test-Path $cli)) { Fail "找不到 $cli（先跑 cargo build --release -p metis）" }

# **打包前必须冒烟**：一个打不开的 exe 打进包里，用户下载后才发现。
$cliVersion = (& $cli --version 2>&1 | Out-String).Trim()
if ($cliVersion -notmatch [regex]::Escape($Version)) {
    Fail "exe 报的版本（$cliVersion）与 Cargo.toml（$Version）不一致 —— 先确认构建产物是新的"
}
Write-Host "  ✓ 版本一致：$cliVersion"

# ── 目录结构 ────────────────────────────────────────────────
$name = "metis-$Version-windows-x64"
$dist = Join-Path $repo "dist"
$stage = Join-Path $dist $name
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force -Path $stage | Out-Null

Copy-Item $cli $stage
Copy-Item "LICENSE-MIT","LICENSE-APACHE" $stage -ErrorAction SilentlyContinue
Copy-Item "README.md" $stage -ErrorAction SilentlyContinue

# 包里的说明要**说清卸载**：撤接管不是删目录就完事。
$readme = @"
Metis $Version — 一份人设，统一管理多个 coding agent

## 怎么用

    metis.exe                     打开图形控制台（推荐；双击也行）
    metis.exe --help              查看全部命令

命令行与图形界面是**同一个 exe**：不带任何参数启动就是 GUI，
带子命令或旗标启动就是 CLI。

首次使用：

    metis.exe doctor              看看本机装了哪些 agent
    metis.exe persona 我的风格 --body "用简体中文，先给结论"
    metis.exe use 我的风格        选一份人设
    metis.exe arm codex           选择要接管的执行器
    metis.exe apply               真的写进去

也可以从官方人设库导入：

    metis.exe import dulingzhi/metis-personas

## 接管命令（裸敲即经 Metis 启动）

    metis.exe arm claude --channel shim
    metis.exe apply

然后把数据目录下的 `shim` 加到 PATH **最前面**，重开终端：

    %USERPROFILE%\.metis\shim

否则系统会先找到原来的 `claude.exe`，接管不会生效（`metis apply` 也会提示）。

## 卸载（重要）

**先撤销接管，再删目录。**

    metis.exe clean               撤掉 Metis 写进各处配置的内容
    metis.exe self-uninstall      撤接管 + 删除数据目录（不可逆）

直接删掉这个目录**不会**撤销已经写进各 agent 配置里的内容。

## 数据在哪

默认 ``%USERPROFILE%\.metis``，可用 ``--home`` 覆盖（写在子命令**之前**）。

## 许可

MIT OR Apache-2.0。详见包内的 LICENSE-MIT / LICENSE-APACHE。
"@
Set-Content (Join-Path $stage "使用说明.txt") -Value $readme -NoNewline -Encoding utf8

# ── 压缩 ────────────────────────────────────────────────────
$zip = Join-Path $dist "$name.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path $stage -DestinationPath $zip -CompressionLevel Optimal
Remove-Item $stage -Recurse -Force

# ── 校验和 ──────────────────────────────────────────────────
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
Set-Content "$zip.sha256" -Value "$hash  $(Split-Path $zip -Leaf)" -NoNewline -Encoding utf8

$sizeMb = [math]::Round((Get-Item $zip).Length / 1MB, 2)
Write-Host ""
Write-Host "✓ 打包完成" -ForegroundColor Green
Write-Host "  $(Split-Path $zip -Leaf)  ($sizeMb MB)"
Write-Host "  sha256: $hash"
Write-Host ""
Write-Host "  验证：Get-FileHash '$zip' -Algorithm SHA256"
