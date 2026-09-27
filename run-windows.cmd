@echo off
rem One-shot build and launch for Windows. Double-click it, or run it from a
rem terminal with the same options as run-windows.ps1, e.g.  run-windows.cmd -Demo
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0run-windows.ps1" %*
