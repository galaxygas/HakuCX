# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# Build the Windows release packages into dist\:
#   HakuCX-Setup-<version>-x64.exe     installer (needs NSIS 3: makensis on PATH or installed)
#   HakuCX-<version>-x64-portable.zip  portable copy (keeps its data in a Data folder beside it)
#
#   powershell -ExecutionPolicy Bypass -File packaging\windows\package.ps1 [-SkipBuild] [-SkipInstaller]
#   (Windows PowerShell 5.1 or PowerShell 7)
#
# The Visual C++ runtime DLLs HakuCX needs are copied next to hakucx.exe (Microsoft allows
# this "app-local" deployment), so HakuCX runs on a fresh Windows without installing them.
# So are the GStreamer libraries and plugins for audio and video (LGPL; their licenses go in
# licenses\gstreamer), which the build lists in gstreamer-files.txt.

param(
  [switch]$SkipBuild,
  [switch]$SkipInstaller
)
$ErrorActionPreference = "Stop"
$root = (Resolve-Path "$PSScriptRoot\..\..").Path
Set-Location $root

$version = (Select-String -Path "$root\Cargo.toml" -Pattern '^version = "([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
Write-Host "HakuCX $version"
# GStreamer: needed to build (pkg-config) and shipped with the program.
# (Called, not dot-sourced: it only sets environment variables, which the whole process shares.)
& "$PSScriptRoot\gstreamer.ps1"

if (-not $SkipBuild) {
  cargo build --release -p hakucx -p haku-release
  if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { "$root\target" }
$release = "$target\release"
# ANGLE: mozangle (a Servo dependency) builds libEGL/libGLESv2 into its own OUT_DIR. hakucx's
# build script copies them next to the exe, but Cargo may run it before mozangle is built (it
# always does on a clean build), so take them from mozangle's newest output here.
foreach ($dll in "libEGL.dll", "libGLESv2.dll") {
  if (-not (Test-Path "$release\$dll")) {
    $angle = Get-ChildItem "$release\build" -Directory -Filter "mozangle-*" -ErrorAction SilentlyContinue |
      ForEach-Object { Join-Path $_.FullName "out" } |
      Where-Object { (Test-Path "$_\libEGL.dll") -and (Test-Path "$_\libGLESv2.dll") } |
      Sort-Object { (Get-Item "$_\libEGL.dll").LastWriteTime } -Descending |
      Select-Object -First 1
    if ($angle) { Copy-Item "$angle\$dll" "$release\$dll" }
  }
}
foreach ($f in "hakucx.exe", "libEGL.dll", "libGLESv2.dll", "haku-release.exe", "gstreamer-files.txt") {
  if (-not (Test-Path "$release\$f")) { throw "missing $release\$f (build first)" }
}

# Visual C++ runtime from the newest Visual Studio installation.
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
$crt = Get-ChildItem "$vs\VC\Redist\MSVC\*\x64\Microsoft.VC14*.CRT" -Directory | Sort-Object FullName | Select-Object -Last 1
if (-not $crt) { throw "Visual C++ redistributable files not found under $vs" }
Write-Host "C++ runtime: $($crt.FullName)"

$dist = "$root\dist"
$stage = "$dist\stage\HakuCX"
if (Test-Path "$dist\stage") { Remove-Item -Recurse -Force "$dist\stage" }
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item "$release\hakucx.exe", "$release\libEGL.dll", "$release\libGLESv2.dll" $stage
Copy-Item "$($crt.FullName)\*.dll" $stage
Copy-Item "$root\LICENSE" "$stage\LICENSE.txt"
$gstFiles = Get-Content "$release\gstreamer-files.txt" | Where-Object { $_ }
foreach ($f in $gstFiles) { Copy-Item "$release\$f" $stage }
Write-Host "GStreamer: $($gstFiles.Count) files"
New-Item -ItemType Directory -Force "$stage\licenses" | Out-Null
Copy-Item -Recurse "$env:GSTREAMER_1_0_ROOT_MSVC_X86_64\share\licenses" "$stage\licenses\gstreamer"
Copy-Item "$root\docs\THIRD-PARTY.md" "$stage\licenses\THIRD-PARTY.md"
& "$release\haku-release.exe" icon "$dist\stage\hakucx.ico"
if ($LASTEXITCODE -ne 0) { throw "icon failed" }

# Portable zip: the same files plus the marker that keeps data next to the program.
$portable = "$dist\stage\portable\HakuCX"
New-Item -ItemType Directory -Force $portable | Out-Null
Copy-Item "$stage\*" $portable -Recurse
Set-Content -Path "$portable\portable" -Value "HakuCX keeps its data in the Data folder next to hakucx.exe while this file exists." -Encoding utf8
$zip = "$dist\HakuCX-$version-x64-portable.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path $portable -DestinationPath $zip
Write-Host "Portable: $zip"

if (-not $SkipInstaller) {
  $makensis = (Get-Command makensis -ErrorAction SilentlyContinue).Source
  if (-not $makensis) { $makensis = "${env:ProgramFiles(x86)}\NSIS\makensis.exe" }
  if (-not (Test-Path $makensis)) { throw "NSIS (makensis) not found: install NSIS 3 or pass -SkipInstaller" }
  $setup = "$dist\HakuCX-Setup-$version-x64.exe"
  & $makensis /V2 "/DVERSION=$version" "/DSTAGE=$stage" "/DOUTFILE=$setup" "/DICON=$dist\stage\hakucx.ico" "$PSScriptRoot\installer.nsi"
  if ($LASTEXITCODE -ne 0) { throw "makensis failed" }
  Write-Host "Installer: $setup"
}
