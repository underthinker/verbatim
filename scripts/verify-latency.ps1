<#
.SYNOPSIS
  Portable raw-latency verification for the M1 acceptance criterion
  (docs/ROADMAP.md:27, issue #16) on the reference Windows laptop.

.DESCRIPTION
  Measures p50/p95 stop-to-text wall time for the recorded 10 s fixture through a
  resident whisper.cpp model and asserts p50 < 800 ms. Same bench CI runs, packaged
  as one command so the reference-hardware measurement is turnkey.

.PARAMETER ModelPath
  ggml model path. Omitted: uses $env:VERBATIM_WHISPER_MODEL, else downloads
  ggml-base.en into %USERPROFILE%\whisper-models (matching CI).

.EXAMPLE
  pwsh scripts/verify-latency.ps1
#>
[CmdletBinding()]
param([string]$ModelPath)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

$ModelDir = Join-Path $env:USERPROFILE 'whisper-models'
$ModelUrl = 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin'

$Model = if ($ModelPath) { $ModelPath } elseif ($env:VERBATIM_WHISPER_MODEL) { $env:VERBATIM_WHISPER_MODEL } else { $null }
if (-not $Model) {
    $Model = Join-Path $ModelDir 'ggml-base.en.bin'
    if (-not (Test-Path $Model)) {
        Write-Host "==> Downloading resident model to $Model"
        New-Item -ItemType Directory -Force -Path $ModelDir | Out-Null
        Invoke-WebRequest -Uri $ModelUrl -OutFile $Model
    }
}
if (-not (Test-Path $Model)) { throw "model not found at $Model" }

$Budget = if ($env:VERBATIM_BENCH_MAX_P50_MS) { $env:VERBATIM_BENCH_MAX_P50_MS } else { '800' }
Write-Host "==> Model:      $Model"
Write-Host "==> Budget:     p50 < $Budget ms"
Write-Host "==> Iterations: $(if ($env:VERBATIM_BENCH_ITERATIONS) { $env:VERBATIM_BENCH_ITERATIONS } else { '20' })"
Write-Host "==> Running resident-model latency bench (excludes load time by design)...`n"

$env:VERBATIM_WHISPER_MODEL = $Model
$env:VERBATIM_BENCH_REQUIRE = '1'
$env:VERBATIM_BENCH_MAX_P50_MS = $Budget
# CI sets this on Windows to avoid the i8mm ggml path on runners that lack it;
# harmless on hardware that supports it.
if (-not $env:GGML_NO_I8MM) { $env:GGML_NO_I8MM = '1' }

cargo bench --locked -p verbatim-engines --features whisper-cpp --bench latency
if ($LASTEXITCODE -ne 0) { throw "latency bench failed (exit $LASTEXITCODE)" }

Write-Host "`n==> Latency check passed. Record the printed p50/p95 in issue #16."
