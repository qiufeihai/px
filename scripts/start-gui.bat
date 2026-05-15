@echo off
setlocal

set "SCRIPT_DIR=%~dp0"
for %%I in ("%SCRIPT_DIR%..") do set "ROOT_DIR=%%~fI"

set "APP1=%ROOT_DIR%\gui\PX 个人代理.exe"
set "APP2=%ROOT_DIR%\gui\tauri-ui.exe"

if exist "%APP1%" (
  cd /d "%ROOT_DIR%"
  start "" "%APP1%"
  exit /b 0
)

if exist "%APP2%" (
  cd /d "%ROOT_DIR%"
  start "" "%APP2%"
  exit /b 0
)

echo 未找到可启动的 GUI 程序，请确认发布目录下的 gui\ 内包含 GUI 可执行文件。
exit /b 1
