param()

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RootDir = Split-Path -Parent $ScriptDir

$candidates = @(
  (Join-Path $RootDir "gui/PX 个人代理.exe"),
  (Join-Path $RootDir "gui/tauri-ui.exe")
)

$app = $null
foreach ($candidate in $candidates) {
  if (Test-Path $candidate) {
    $app = $candidate
    break
  }
}

if (-not $app) {
  $app = Get-ChildItem -Path (Join-Path $RootDir "gui") -Filter *.exe -Recurse -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -notmatch 'uninstall|setup' } |
    Select-Object -First 1 -ExpandProperty FullName
}

if (-not $app) {
  Write-Error "未找到可启动的 GUI 程序，请确认发布目录下的 gui/ 内包含 GUI 可执行文件。"
  exit 1
}

Set-Location $RootDir
Start-Process -FilePath $app -WorkingDirectory $RootDir
