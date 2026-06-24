# owox-harness installer script (Windows).
# Downloads owox from GitHub Releases, verifies it with SHA256SUMS, then installs it
# See release distribution policy in control/docs/decisions/.
#
# Usage:
#   irm https://raw.githubusercontent.com/owoDra/workspace/main/control/scripts/install.ps1 | iex
#
# Environment variables:
#   OWOX_VERSION  Version to download (for example, owox-v0.1.0 or 0.1.0). Defaults to the latest owox-v*
#   OWOX_BIN_DIR  Install directory. Defaults to $env:LOCALAPPDATA\owox\bin
#   OWOX_REPO     Repository. Defaults to owoDra/workspace
$ErrorActionPreference = "Stop"

$repo = if ($env:OWOX_REPO) { $env:OWOX_REPO } else { "owo-x-project/owox-harness" }
$binDir = if ($env:OWOX_BIN_DIR) { $env:OWOX_BIN_DIR } else { Join-Path $env:LOCALAPPDATA "owox\bin" }

# Map the CPU to a target triple. Only x86_64 is distributed.
$arch = $env:PROCESSOR_ARCHITECTURE
if ($arch -ne "AMD64") {
    throw "owox install: unsupported CPU: $arch (Windows is distributed for x86_64 only)"
}
$target = "x86_64-pc-windows-msvc"
$asset = "owox-$target.zip"

# Resolve the version. If unset, use the latest owox-v* tag from Releases.
$tag = $env:OWOX_VERSION
if (-not $tag) {
    $releases = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases"
    $tag = ($releases | Where-Object { $_.tag_name -like "owox-v*" } | Select-Object -First 1).tag_name
    if (-not $tag) { throw "owox install: could not find the latest owox-v* release. Set OWOX_VERSION" }
}
elseif (-not $tag.StartsWith("owox-v")) {
    $tag = "owox-v" + $tag.TrimStart("v")
}

$base = "https://github.com/$repo/releases/download/$tag"
Write-Host "owox install: downloading $asset from $tag"

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
try {
    $zip = Join-Path $tmp $asset
    Invoke-WebRequest -Uri "$base/$asset" -OutFile $zip
    $sumsPath = Join-Path $tmp "SHA256SUMS"
    Invoke-WebRequest -Uri "$base/SHA256SUMS" -OutFile $sumsPath

    # Verify only this artifact line (SHA256SUMS contains all artifacts).
    $expected = (Get-Content $sumsPath | Where-Object { $_ -match "\s$([regex]::Escape($asset))$" } |
        Select-Object -First 1) -split '\s+' | Select-Object -First 1
    if (-not $expected) { throw "owox install: SHA256SUMS has no line for $asset" }
    $actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
    if ($actual -ne $expected.ToLower()) {
        throw "owox install: checksum mismatch. Aborting installation"
    }

    Expand-Archive -Path $zip -DestinationPath $tmp -Force
    $exe = Join-Path $tmp "owox.exe"
    if (-not (Test-Path $exe)) { throw "owox install: artifact does not contain owox.exe" }

    New-Item -ItemType Directory -Force -Path $binDir | Out-Null
    Copy-Item $exe (Join-Path $binDir "owox.exe") -Force
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

Write-Host "owox install: installed to $binDir\owox.exe"
& (Join-Path $binDir "owox.exe") --version

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$binDir*") {
    Write-Host "owox install: add $binDir to PATH (example: setx PATH `"$binDir;%PATH%`")"
}
