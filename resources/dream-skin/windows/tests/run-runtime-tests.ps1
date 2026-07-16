[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
. (Join-Path $Root 'scripts\common-windows.ps1')

$temporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) "quotio-dream-skin-tests-$PID-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $temporaryRoot | Out-Null

try {
  # State/log writes replace existing files, so exercise the exact atomic path
  # used by repeated launches and verification.
  $atomicPath = Join-Path $temporaryRoot 'atomic.txt'
  Write-DreamSkinUtf8FileAtomically -Path $atomicPath -Content 'first'
  Write-DreamSkinUtf8FileAtomically -Path $atomicPath -Content 'second'
  if ((Read-DreamSkinUtf8File -Path $atomicPath) -cne 'second') {
    throw 'Atomic UTF-8 replacement did not preserve the latest content.'
  }

  foreach ($file in Get-ChildItem (Join-Path $Root 'scripts') -Filter '*.ps1') {
    $tokens = $null
    $parseErrors = $null
    [void][System.Management.Automation.Language.Parser]::ParseFile(
      $file.FullName,
      [ref]$tokens,
      [ref]$parseErrors
    )
    if ($parseErrors.Count -gt 0) {
      throw "PowerShell parse failed for $($file.Name): $($parseErrors[0].Message)"
    }
  }

  $startScript = Get-Content (Join-Path $Root 'scripts\start-dream-skin.ps1') -Raw
  if (-not $startScript.Contains('NoFallbackRelaunch')) {
    throw 'Quotio launch ownership guard is missing from the Dream Skin start script.'
  }
  if ($startScript.Contains("[ValidateSet('dream', 'aurora', 'midnight')]") -or
    -not $startScript.Contains('[string]$ThemeDir')) {
    throw 'Dream Skin start script still restricts launch to the three bundled themes.'
  }
  if ($startScript.Contains('Start-Process -FilePath $codex.Executable') -or
    -not $startScript.Contains('Start-DreamSkinCodex -Codex $codex -Arguments $arguments')) {
    throw 'Dream Skin still launches the Store Codex executable directly instead of using package activation.'
  }
  if ($startScript.Contains('$arguments += ConvertTo-DreamSkinProcessArgument')) {
    throw 'Dream Skin launch arguments are encoded twice before package activation.'
  }
  $quotedProfileArgument = ConvertTo-DreamSkinProcessArgument -Value '--user-data-dir=C:\Test User\'
  if ($quotedProfileArgument -cne '"--user-data-dir=C:\Test User\\"') {
    throw 'Dream Skin package launch argument quoting is invalid for a spaced path ending in a slash.'
  }

  $commonScriptPath = Join-Path $Root 'scripts\common-windows.ps1'
  $commonScript = Get-Content $commonScriptPath -Raw
  if (-not $commonScript.Contains('ApplicationActivationManager') -or
    -not $commonScript.Contains('ActivateApplication') -or
    -not $commonScript.Contains('function Start-DreamSkinCodex')) {
    throw 'Dream Skin packaged-app activation support is missing.'
  }
  Initialize-DreamSkinPackagedAppActivator
  if ($null -eq ('CodexDreamSkin.Interop.PackagedAppLauncher' -as [type])) {
    throw 'Dream Skin packaged-app activation interop failed to compile.'
  }
  $fakeAumid = Get-DreamSkinCodexAppUserModelId -Codex ([pscustomobject]@{
    PackageFamilyName = 'OpenAI.Codex_TestFamily'
  })
  if ($fakeAumid -cne 'OpenAI.Codex_TestFamily!App') {
    throw 'Dream Skin packaged-app AUMID fallback is invalid.'
  }

  $escapedCommonPath = $commonScriptPath.Replace("'", "''")
  $smokeContent = @(
    ". '$escapedCommonPath'"
    'Initialize-DreamSkinPackagedAppActivator'
    "if (`$null -eq ('CodexDreamSkin.Interop.PackagedAppLauncher' -as [type])) { exit 7 }"
  ) -join [Environment]::NewLine
  $encodedSmoke = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($smokeContent))
  & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand $encodedSmoke *> $null
  if ($LASTEXITCODE -ne 0) {
    throw 'Dream Skin packaged-app activation interop failed under Windows PowerShell 5.1.'
  }

  $renderer = Read-DreamSkinUtf8File -Path (Join-Path $Root 'assets\renderer-inject.js')
  $personSpecificChineseName = -join @([char]0x859B, [char]0x51F1, [char]0x742A)
  $personSpecificChineseGivenName = -join @([char]0x51F1, [char]0x742A)
  if ($renderer.Contains('Fiona Sit') -or $renderer.Contains($personSpecificChineseName)) {
    throw 'Unlicensed person-specific Dream Skin branding is still present.'
  }
  if ($renderer.Contains('const artUrl = previous?.artUrl ||')) {
    throw 'Dream Skin still reuses the previous theme image when switching themes.'
  }
  if (-not $renderer.Contains('brandTitle.textContent') -or
    -not $renderer.Contains('signature.textContent')) {
    throw 'Dream Skin chrome branding is not refreshed when switching themes.'
  }
  $baseCss = Read-DreamSkinUtf8File -Path (Join-Path $Root 'assets\dream-skin.css')
  if ($baseCss.Contains($personSpecificChineseGivenName) -or $baseCss.Contains('Fiona')) {
    throw 'Unlicensed person-specific Dream Skin copy is still present in CSS.'
  }

  $node = Get-DreamSkinNodeRuntime
  & $node.Path (Join-Path $Root 'scripts\injector.mjs') --self-test *> $null
  if ($LASTEXITCODE -ne 0) { throw 'Injector CDP self-test failed.' }
  foreach ($themeDir in Get-ChildItem (Join-Path $Root 'themes') -Directory) {
    $theme = $themeDir.Name
    $themeConfig = (Read-DreamSkinUtf8File -Path (Join-Path $themeDir.FullName 'theme.json')) | ConvertFrom-Json
    $themeCss = Read-DreamSkinUtf8File -Path (Join-Path $themeDir.FullName 'theme.css')
    if ($themeConfig.galleryPreset -eq $true) {
      if ($themeCss.Length -lt 2000 -or
        -not $themeCss.Contains("[data-dream-theme=`"$theme`"]")) {
        throw "Gallery theme is still only a generic color preset: $theme"
      }
      $switchModeLabel = -join @([char]0x5207, [char]0x6362, [char]0x6A21, [char]0x5F0F)
      $searchLabel = -join @([char]0x641C, [char]0x7D22)
      if ($themeCss.Contains("button[aria-label^=`"$switchModeLabel`"]") -or
        $themeCss.Contains("button[aria-label=`"$searchLabel`"]")) {
        throw "Gallery theme still relies on Chinese-only control labels: $theme"
      }
    }
    & $node.Path (Join-Path $Root 'scripts\injector.mjs') --check-payload --theme-dir $themeDir.FullName *> $null
    if ($LASTEXITCODE -ne 0) { throw "Injector payload test failed for theme: $theme" }
  }

  $customThemeDir = Join-Path $temporaryRoot 'user-test-theme'
  New-Item -ItemType Directory -Path $customThemeDir | Out-Null
  Copy-Item (Join-Path $Root 'assets\dream-reference.png') (Join-Path $customThemeDir 'background.png')
  Write-DreamSkinUtf8FileAtomically -Path (Join-Path $customThemeDir 'theme.json') -Content @'
{
  "schemaVersion": 1,
  "id": "user-test-theme",
  "name": "\u7528\u6237\u4e3b\u9898\u6d4b\u8bd5",
  "image": "background.png"
}
'@
  Write-DreamSkinUtf8FileAtomically -Path (Join-Path $customThemeDir 'theme.css') -Content ':root.codex-dream-skin { --dream-theme-name: "user-test"; }'
  & $node.Path (Join-Path $Root 'scripts\injector.mjs') --check-payload --theme-dir $customThemeDir *> $null
  if ($LASTEXITCODE -ne 0) { throw 'Injector payload test failed for a generated user theme.' }

  Write-Host 'PASS: Quotio Dream Skin runtime, atomic state writes, payload, and launch ownership.'
} finally {
  Remove-Item -LiteralPath $temporaryRoot -Recurse -Force -ErrorAction SilentlyContinue
}
