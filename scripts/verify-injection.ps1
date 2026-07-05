<#
.SYNOPSIS
  Portable injection verification for the M1 acceptance criterion
  (docs/ROADMAP.md:25, issue #18) on Windows.

.DESCRIPTION
  Runs the real-state Windows seam E2E (gated behind VERBATIM_WIN_E2E because it
  mutates the real clipboard), then prints the manual real-keypress checklist
  including the elevated-window UIPI case that only a live session can complete.

.EXAMPLE
  pwsh scripts/verify-injection.ps1
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

Write-Host "==> Platform: Windows"
Write-Host "==> Running seam E2E (VERBATIM_WIN_E2E=1, real clipboard)...`n"

$env:VERBATIM_WIN_E2E = '1'
cargo test --locked -p verbatim-platform --features win-inject --test windows_seams
if ($LASTEXITCODE -ne 0) { throw "windows seam E2E failed (exit $LASTEXITCODE)" }

Write-Host ""
Write-Host "======================================================================"
Write-Host " Automated seam E2E passed. Manual real-keypress checklist for Windows"
Write-Host " (docs/M1_INJECTION_VERIFICATION.md) - record results in issue #18:"
Write-Host "======================================================================"
Write-Host ""
Write-Host " For every check: open a plain text editor, keep the caret focused there,"
Write-Host " trigger dictation, speak a known phrase, confirm the EXACT text lands"
Write-Host " IN THE EDITOR (not merely on the clipboard)."
Write-Host ""
Write-Host "  [ ] Dictate into Notepad; text lands via SendInputUnicode."
Write-Host "  [ ] Dictate into an ELEVATED window (e.g. an admin console); UIPI blocks"
Write-Host "      SendInput, the failure is detected (short insert), and it falls"
Write-Host "      through to clipboard (E4) rather than silently dropping text."
Write-Host "  [ ] The user's prior clipboard is restored after a paste-backed injection."
Write-Host ""
