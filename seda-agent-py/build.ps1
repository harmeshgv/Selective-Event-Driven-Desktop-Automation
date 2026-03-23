$ErrorActionPreference = "Stop"

Set-Location $PSScriptRoot

Get-Process seda-agent -ErrorAction SilentlyContinue | Stop-Process -Force
Get-Process SEDA -ErrorAction SilentlyContinue | Stop-Process -Force

if (-not (Test-Path ".\\.venv\\Scripts\\python.exe")) {
  python -m venv .venv
}

.\\.venv\\Scripts\\python -m pip install -U pip
.\\.venv\\Scripts\\pip install -e . --upgrade
.\\.venv\\Scripts\\pip install pyinstaller

Remove-Item -Recurse -Force .\\dist -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force .\\build -ErrorAction SilentlyContinue

.\\.venv\\Scripts\\pyinstaller `
  --noconsole `
  --onefile `
  --name "SEDA" `
  .\\seda_qt_app.py

Write-Host "Built: $PSScriptRoot\\dist\\SEDA.exe"

