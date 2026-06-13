#Requires -Version 7
<#
.SYNOPSIS
  把 app 版本号一次性写入全部 4 个位置：
    apps/desktop/src-tauri/tauri.conf.json  (决定安装包/便携版名字 + app 内显示)
    apps/desktop/src-tauri/Cargo.toml       (exe 文件属性版本)
    package.json / apps/desktop/package.json (npm 记录)
.EXAMPLE
  pwsh scripts/set-version.ps1 minor    # 0.2.0 -> 0.3.0  (加了新功能)
  pwsh scripts/set-version.ps1 patch    # 0.2.0 -> 0.2.1  (只修 bug / 调样式)
  pwsh scripts/set-version.ps1 major    # 0.2.0 -> 1.0.0  (破坏性变更 / 里程碑)
  pwsh scripts/set-version.ps1 0.5.2    # 直接指定
  # 经 npm:  npm run version:set -- minor
#>
param([string]$Bump)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$tauriConf = Join-Path $root 'apps/desktop/src-tauri/tauri.conf.json'

$current = (Get-Content $tauriConf -Raw | ConvertFrom-Json).version

if (-not $Bump) {
  Write-Host "当前版本: $current"
  Write-Host "用法: pwsh scripts/set-version.ps1 <major|minor|patch|X.Y.Z>"
  return
}

function Get-NextVersion([string]$cur, [string]$bump) {
  if ($bump -match '^\d+\.\d+\.\d+$') { return $bump }   # 直接给了完整版本号
  $p = $cur -split '\.'
  $maj = [int]$p[0]; $min = [int]$p[1]; $pat = [int]$p[2]
  switch ($bump.ToLower()) {
    'major' { "$($maj + 1).0.0" }
    'minor' { "$maj.$($min + 1).0" }
    'patch' { "$maj.$min.$($pat + 1)" }
    default { throw "Bump 必须是 major|minor|patch 或 X.Y.Z，收到 '$bump'" }
  }
}

$new = Get-NextVersion $current $Bump

# 只替换每个文件里的第一处版本行，保留原有格式/缩进/换行，不重排整份文件。
function Update-VersionLine([string]$path, [string]$pattern, [string]$replacement) {
  if (-not (Test-Path $path)) { Write-Warning "跳过(不存在): $path"; return }
  $text = Get-Content $path -Raw
  $updated = ([regex]$pattern).Replace($text, $replacement, 1)
  if ($updated -ne $text) {
    Set-Content -Path $path -Value $updated -NoNewline -Encoding utf8
    Write-Host "  OK  $($path.Substring($root.Length + 1))"
  } else {
    Write-Warning "  未匹配到版本行: $path"
  }
}

Write-Host "版本: $current -> $new"
Update-VersionLine $tauriConf '"version"\s*:\s*"[^"]*"' ('"version": "' + $new + '"')
Update-VersionLine (Join-Path $root 'package.json') '"version"\s*:\s*"[^"]*"' ('"version": "' + $new + '"')
Update-VersionLine (Join-Path $root 'apps/desktop/package.json') '"version"\s*:\s*"[^"]*"' ('"version": "' + $new + '"')
Update-VersionLine (Join-Path $root 'apps/desktop/src-tauri/Cargo.toml') '(?m)^version\s*=\s*"[^"]*"' ('version = "' + $new + '"')
Write-Host ""
Write-Host "下一步: npm run release   (编译 + 生成 Quotio_${new}_x64 安装包/便携版)"
