param(
  [Parameter(Mandatory = $true)][string]$ServerAddr,
  [string]$ServerCertPath = "config/server-cert.pem",
  [string]$LocalSocksAddr = "127.0.0.1:1080",
  [int]$ConnectTimeoutMs = 5000,
  [string]$LogLevel = "info",
  [string]$OutPath = (Join-Path (Get-Location) "config/client.toml")
)

$outDir = Split-Path -Parent $OutPath
if (-not (Test-Path $outDir)) {
  New-Item -ItemType Directory -Path $outDir -Force | Out-Null
}

@"
server_addr = "$ServerAddr"
server_cert_path = "$ServerCertPath"
local_socks_addr = "$LocalSocksAddr"
connect_timeout_ms = $ConnectTimeoutMs
log_level = "$LogLevel"
"@ | Set-Content -Path $OutPath -Encoding UTF8

Write-Host "generated: $OutPath"
