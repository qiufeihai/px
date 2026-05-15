param(
  [Parameter(Mandatory = $true)][string]$VpsHost,
  [string]$VpsUser = "root",
  [string]$RemoteCertPath = "/opt/px/config/server-cert.pem",
  [string]$OutPath = (Join-Path (Get-Location) "config/server-cert.pem")
)

$outDir = Split-Path -Parent $OutPath
if (-not (Test-Path $outDir)) {
  New-Item -ItemType Directory -Path $outDir -Force | Out-Null
}

scp "${VpsUser}@${VpsHost}:${RemoteCertPath}" "$OutPath"
Write-Host "downloaded: $OutPath"
