param(
  [string]$BinDir = (Join-Path (Get-Location) "bin"),
  [string]$WintunVersion = "0.14.1",
  [string]$CacheReleaseUrl = "https://github.com/qiufeihai/px/releases/download/tun-helper-cache-v1"
)

$ErrorActionPreference = "Stop"

$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("px-tun-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null

try {
  New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
  $helperPath = Join-Path $BinDir "px-tun-helper.exe"
  $helperSourceDir = Join-Path (Split-Path $PSScriptRoot -Parent) "helpers/px-tun-helper"

  function Invoke-DownloadWithFallback {
    param(
      [string]$AssetName,
      [string]$UpstreamUrl,
      [string]$OutFile
    )

    $cacheUrl = "$CacheReleaseUrl/$AssetName"
    Write-Host "trying cache: $cacheUrl"
    try {
      Invoke-WebRequest -Uri $cacheUrl -OutFile $OutFile -ConnectionTimeoutSeconds 8
      return
    } catch {
      Write-Host "cache miss, fallback to upstream: $UpstreamUrl"
    }

    Invoke-WebRequest -Uri $UpstreamUrl -OutFile $OutFile -ConnectionTimeoutSeconds 8
  }

  if (Test-Path (Join-Path $helperSourceDir "go.mod")) {
    Write-Host "building helper from source: $helperSourceDir"
    Push-Location $helperSourceDir
    try {
      go build -o $helperPath ./cmd/px-tun-helper
    }
    finally {
      Pop-Location
    }
    Write-Host "built: $helperPath"
  }
  elseif (-not (Test-Path $helperPath)) {
    throw "px-tun-helper source not found. Windows 正式发布包默认已自带 px-tun-helper.exe；若当前缺失，请重新解压发布包。开发环境请在仓库根目录执行 Go 构建后再重试。"
  }
  else {
    Write-Host "using existing helper: $helperPath"
  }

  $wintunZip = Join-Path $tmpDir "wintun-$WintunVersion.zip"
  $wintunUrl = "https://www.wintun.net/builds/wintun-$WintunVersion.zip"
  Invoke-DownloadWithFallback -AssetName "wintun-$WintunVersion.zip" -UpstreamUrl $wintunUrl -OutFile $wintunZip
  Expand-Archive -Path $wintunZip -DestinationPath (Join-Path $tmpDir "wintun") -Force
  $wintunDll = Get-ChildItem -Path (Join-Path $tmpDir "wintun") -Recurse -Filter "wintun.dll" |
    Where-Object { $_.FullName -match '[\\/]amd64[\\/]' } |
    Select-Object -First 1
  if (-not $wintunDll) {
    throw "wintun.dll not found under amd64 in archive"
  }
  Copy-Item $wintunDll.FullName (Join-Path $BinDir "wintun.dll") -Force
  Write-Host "downloaded: $(Join-Path $BinDir 'wintun.dll')"

  Write-Host "cache release: $CacheReleaseUrl"
  Write-Host "helper path: $helperPath"
  Write-Host "wintun version: $WintunVersion"
}
finally {
  if (Test-Path $tmpDir) {
    Remove-Item -Path $tmpDir -Recurse -Force
  }
}
