#Requires -Version 7
param(
  [switch]$SkipBuild
)

<#
.SYNOPSIS
  构建并组装免安装(便携)版：读取 tauri.conf.json 的版本号，把生产模式 exe + 资源
  打包成 dist-portable/Quotio_<版本>_x64_portable/ 及同名 zip。

.PARAMETER SkipBuild
  仅组装已有的 target/release/quotio-desktop.exe。只应在刚执行完 Tauri
  生产构建后使用；普通 `cargo build --release` 会生成访问 localhost 的开发模式程序。
#>
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$version = (Get-Content (Join-Path $root 'apps/desktop/src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json).version

if (-not $SkipBuild) {
  Write-Host "正在执行 Tauri 生产构建（跳过安装包与签名）..."
  Push-Location $root
  try {
    & npm --prefix apps/desktop run tauri -- build --no-bundle
    if ($LASTEXITCODE -ne 0) {
      throw "Tauri 生产构建失败（退出码 $LASTEXITCODE）"
    }
  } finally {
    Pop-Location
  }
}

$exe = Join-Path $root 'target/release/quotio-desktop.exe'
if (-not (Test-Path $exe)) { throw "找不到 $exe —— 请先运行 npm run desktop:build:portable" }

$name = "Quotio_${version}_x64_portable"
$outDir = Join-Path $root "dist-portable/$name"
$zip = Join-Path $root "dist-portable/$name.zip"

# 全新组装，避免残留旧文件
if (Test-Path $outDir) { Remove-Item $outDir -Recurse -Force }
$resDir = Join-Path $outDir 'resources'
New-Item -ItemType Directory -Path $resDir -Force | Out-Null

# 1) app 主程序 -> Quotio.exe
Copy-Item $exe (Join-Path $outDir 'Quotio.exe') -Force

# 2) 图标（由 tauri build 产出）
$icon = Join-Path $root 'target/release/resources/icon.ico'
if (Test-Path $icon) { Copy-Item $icon (Join-Path $resDir 'icon.ico') -Force }

# 3) proxy README 占位（来自仓库 resources/proxy）
foreach ($os in 'darwin', 'linux', 'windows') {
  $src = Join-Path $root "resources/proxy/$os/README.md"
  if (Test-Path $src) {
    $dst = Join-Path $resDir "proxy/$os"
    New-Item -ItemType Directory -Path $dst -Force | Out-Null
    Copy-Item $src (Join-Path $dst 'README.md') -Force
  }
}

# 4) 代理二进制（便携版离线可用）：取 %APPDATA% 里已下载的那份
$proxyExe = Join-Path $env:APPDATA 'Quotio/proxy/CLIProxyAPI.exe'
$winDir = Join-Path $resDir 'proxy/windows'
New-Item -ItemType Directory -Path $winDir -Force | Out-Null
if (Test-Path $proxyExe) {
  Copy-Item $proxyExe (Join-Path $winDir 'CLIProxyAPI.exe') -Force
  Write-Host "已内置 CLIProxyAPI.exe（便携版可离线启动代理）"
} else {
  Write-Warning "未找到 $proxyExe —— 便携版未内置代理，首次运行会自动下载（需联网一次）"
}

# 4b) kiro-rs sidecar（Kiro 代理池离线可用）：取 %APPDATA% 里那份
$kiroExe = Join-Path $env:APPDATA 'Quotio/kiro-rs/kiro-rs.exe'
if (Test-Path $kiroExe) {
  $kiroDir = Join-Path $resDir 'kiro/windows'
  New-Item -ItemType Directory -Path $kiroDir -Force | Out-Null
  Copy-Item $kiroExe (Join-Path $kiroDir 'kiro-rs.exe') -Force
  Write-Host "已内置 kiro-rs.exe（便携版 Kiro 池离线可用）"
} else {
  Write-Warning "未找到 $kiroExe —— 便携版未内置 kiro-rs（无 Kiro 账号时无所谓）"
}

# 4c) 按 key 路由插件（CLIProxyAPI scheduler 插件）：取 %APPDATA% 里那份。
#     放进 resources/proxy/windows/plugins/，app 启动时会自动装载到管理目录。
$plugin = Join-Path $env:APPDATA 'Quotio/proxy/plugins/quotio-key-router.dll'
if (Test-Path $plugin) {
  $pluginDir = Join-Path $winDir 'plugins'
  New-Item -ItemType Directory -Path $pluginDir -Force | Out-Null
  Copy-Item $plugin (Join-Path $pluginDir 'quotio-key-router.dll') -Force
  Write-Host "已内置 quotio-key-router.dll（便携版按 key 路由离线可用）"
} else {
  Write-Warning "未找到 $plugin —— 便携版未内置按 key 路由插件"
}

# 4d) Codex Dream Skin（启动方案可选；运行时仍要求 PATH 中有 Node.js 22+）。
$dreamSkinSource = Join-Path $root 'resources/dream-skin'
if (Test-Path $dreamSkinSource) {
  Copy-Item $dreamSkinSource (Join-Path $resDir 'dream-skin') -Recurse -Force
  Write-Host "已内置 Codex Dream Skin 运行资源"
}

# 5) 打 zip（zip 内含一层 $name/ 文件夹，解压即得便携目录）
Add-Type -AssemblyName System.IO.Compression.FileSystem
if (Test-Path $zip) { Remove-Item $zip -Force }
[IO.Compression.ZipFile]::CreateFromDirectory($outDir, $zip, [IO.Compression.CompressionLevel]::Optimal, $true)

$zipMB = [math]::Round((Get-Item $zip).Length / 1MB, 1)
Write-Host ""
Write-Host "便携版文件夹: dist-portable/$name/"
Write-Host "压缩包:       dist-portable/$name.zip ($zipMB MB)"
