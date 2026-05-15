param(
  [string]$BinDir = (Join-Path (Get-Location) "bin"),
  [string]$Tun2socksVersion = "v2.6.0",
  [string]$WintunVersion = "0.14.1",
  [string]$CacheReleaseUrl = "https://github.com/qiufeihai/px/releases/download/tun-helper-cache-v1"
)

$ErrorActionPreference = "Stop"

$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("px-tun-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null

try {
  New-Item -ItemType Directory -Path $BinDir -Force | Out-Null

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

  $tunZip = Join-Path $tmpDir "tun2socks-windows-amd64.zip"
  $tunUrl = "https://github.com/xjasonlyu/tun2socks/releases/download/$Tun2socksVersion/tun2socks-windows-amd64.zip"
  Invoke-DownloadWithFallback -AssetName "tun2socks-windows-amd64.zip" -UpstreamUrl $tunUrl -OutFile $tunZip
  Expand-Archive -Path $tunZip -DestinationPath (Join-Path $tmpDir "tun2socks") -Force
  $tunExe = Get-ChildItem -Path (Join-Path $tmpDir "tun2socks") -Filter "tun2socks*.exe" | Select-Object -First 1
  if (-not $tunExe) {
    throw "tun2socks.exe not found in archive"
  }
  Copy-Item $tunExe.FullName (Join-Path $BinDir "tun2socks.exe") -Force
  Write-Host "downloaded: $(Join-Path $BinDir 'tun2socks.exe')"

  $wintunZip = Join-Path $tmpDir "wintun-$WintunVersion.zip"
  $wintunUrl = "https://www.wintun.net/builds/wintun-$WintunVersion.zip"
  Invoke-DownloadWithFallback -AssetName "wintun-$WintunVersion.zip" -UpstreamUrl $wintunUrl -OutFile $wintunZip
  Expand-Archive -Path $wintunZip -DestinationPath (Join-Path $tmpDir "wintun") -Force
  Copy-Item (Join-Path $tmpDir "wintun\amd64\wintun.dll") (Join-Path $BinDir "wintun.dll") -Force
  Write-Host "downloaded: $(Join-Path $BinDir 'wintun.dll')"

  Write-Host "cache release: $CacheReleaseUrl"
  Write-Host "tun2socks version: $Tun2socksVersion"
  Write-Host "wintun version: $WintunVersion"
}
finally {
  if (Test-Path $tmpDir) {
    Remove-Item -Path $tmpDir -Recurse -Force
  }
}
