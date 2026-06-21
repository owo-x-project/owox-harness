# owox-harness 導入スクリプト (Windows)。
# GitHub Releases から owox を取得し、SHA256SUMS で checksum 照合してから配置する
# (control/docs/decisions/20260621-Phase10-配布とrelease正本.md)。
#
# 使い方:
#   irm https://raw.githubusercontent.com/owoDra/workspace/main/control/scripts/install.ps1 | iex
#
# 環境変数:
#   OWOX_VERSION  取得する版 (例 owox-v0.1.0 または 0.1.0)。既定は最新の owox-v*
#   OWOX_BIN_DIR  配置先ディレクトリ。既定 $env:LOCALAPPDATA\owox\bin
#   OWOX_REPO     リポジトリ。既定 owoDra/workspace
$ErrorActionPreference = "Stop"

$repo = if ($env:OWOX_REPO) { $env:OWOX_REPO } else { "owoDra/workspace" }
$binDir = if ($env:OWOX_BIN_DIR) { $env:OWOX_BIN_DIR } else { Join-Path $env:LOCALAPPDATA "owox\bin" }

# CPU を target triple へ写像する。配布は x86_64 のみ。
$arch = $env:PROCESSOR_ARCHITECTURE
if ($arch -ne "AMD64") {
    throw "owox install: 未対応の CPU: $arch (Windows は x86_64 のみ配布)"
}
$target = "x86_64-pc-windows-msvc"
$asset = "owox-$target.zip"

# 版を解決する。未指定なら最新の owox-v* tag を Releases から拾う。
$tag = $env:OWOX_VERSION
if (-not $tag) {
    $releases = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases"
    $tag = ($releases | Where-Object { $_.tag_name -like "owox-v*" } | Select-Object -First 1).tag_name
    if (-not $tag) { throw "owox install: 最新の owox-v* リリースを見つけられない。OWOX_VERSION で指定する" }
}
elseif (-not $tag.StartsWith("owox-v")) {
    $tag = "owox-v" + $tag.TrimStart("v")
}

$base = "https://github.com/$repo/releases/download/$tag"
Write-Host "owox install: $tag の $asset を取得する"

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
try {
    $zip = Join-Path $tmp $asset
    Invoke-WebRequest -Uri "$base/$asset" -OutFile $zip
    $sumsPath = Join-Path $tmp "SHA256SUMS"
    Invoke-WebRequest -Uri "$base/SHA256SUMS" -OutFile $sumsPath

    # 自分の成果物の行だけ照合する (SHA256SUMS は全成果物を含む)。
    $expected = (Get-Content $sumsPath | Where-Object { $_ -match "\s$([regex]::Escape($asset))$" } |
        Select-Object -First 1) -split '\s+' | Select-Object -First 1
    if (-not $expected) { throw "owox install: SHA256SUMS に $asset の行が無い" }
    $actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
    if ($actual -ne $expected.ToLower()) {
        throw "owox install: checksum が一致しない。配置を中止する"
    }

    Expand-Archive -Path $zip -DestinationPath $tmp -Force
    $exe = Join-Path $tmp "owox.exe"
    if (-not (Test-Path $exe)) { throw "owox install: 成果物に owox.exe が無い" }

    New-Item -ItemType Directory -Force -Path $binDir | Out-Null
    Copy-Item $exe (Join-Path $binDir "owox.exe") -Force
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

Write-Host "owox install: $binDir\owox.exe へ配置した"
& (Join-Path $binDir "owox.exe") --version

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$binDir*") {
    Write-Host "owox install: PATH に $binDir を加える (例: setx PATH `"$binDir;%PATH%`")"
}
