# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# GStreamer for building HakuCX with audio and video (see docs/MEDIA.md).
#
#   . packaging\windows\gstreamer.ps1             set PATH and PKG_CONFIG_PATH in this shell
#   packaging\windows\gstreamer.ps1 -Install      download the official installer (checksum
#                                                 checked), install it for this user, then set up
#
# On GitHub Actions the variables are also written to GITHUB_ENV / GITHUB_PATH for later steps.

param(
  [switch]$Install,
  [string]$Version = "1.28.8",
  [string]$Sha256 = "e4c41ee6ee9c6c32a45d198463ec08d79bac3bd5fa58adc93d13fe4a08abc9d9"
)
$ErrorActionPreference = "Stop"

function Find-GStreamer {
  $candidates = @()
  if ($env:GSTREAMER_1_0_ROOT_MSVC_X86_64) { $candidates += $env:GSTREAMER_1_0_ROOT_MSVC_X86_64 }
  $candidates += "$env:LOCALAPPDATA\Programs\gstreamer\1.0\msvc_x86_64"
  $candidates += "$env:ProgramFiles\gstreamer\1.0\msvc_x86_64"
  $candidates += "C:\gstreamer\1.0\msvc_x86_64"
  foreach ($c in $candidates) {
    if ($c -and (Test-Path "$c\lib\pkgconfig\gstreamer-1.0.pc")) { return $c.TrimEnd("\") }
  }
  return $null
}

if ($Install -and -not (Find-GStreamer)) {
  $url = "https://gstreamer.freedesktop.org/data/pkg/windows/$Version/msvc/gstreamer-1.0-msvc-x86_64-$Version.exe"
  $temp = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }
  $file = Join-Path $temp "gstreamer-1.0-msvc-x86_64-$Version.exe"
  Write-Host "Downloading $url"
  Invoke-WebRequest -Uri $url -OutFile $file -UseBasicParsing
  $hash = (Get-FileHash $file -Algorithm SHA256).Hash
  if ($hash -ne $Sha256.ToUpperInvariant()) { throw "GStreamer installer checksum mismatch: $hash" }
  # Per-user install, with the development files (headers, import libraries, pkg-config).
  $p = Start-Process $file -ArgumentList "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CURRENTUSER", "/TYPE=full" -Wait -PassThru
  if ($p.ExitCode -ne 0) { throw "GStreamer installer failed ($($p.ExitCode))" }
}

$root = Find-GStreamer
if (-not $root) { throw "GStreamer (MSVC x86_64 with development files) not found; run this script with -Install" }
$env:GSTREAMER_1_0_ROOT_MSVC_X86_64 = $root
$env:PKG_CONFIG_PATH = "$root\lib\pkgconfig"
if (-not ($env:PATH -split ";" | Where-Object { $_ -eq "$root\bin" })) { $env:PATH = "$root\bin;$env:PATH" }
if ($env:GITHUB_ENV) {
  "GSTREAMER_1_0_ROOT_MSVC_X86_64=$root" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
  "PKG_CONFIG_PATH=$root\lib\pkgconfig" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
  "$root\bin" | Out-File -FilePath $env:GITHUB_PATH -Append -Encoding utf8
}
Write-Host "GStreamer: $root"
