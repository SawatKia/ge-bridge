$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$stages = @(
  @{ Name = 'cargo test';          Cmd = { cargo test } },
  @{ Name = 'cargo build release'; Cmd = { cargo build --release } },
  @{ Name = 'extension unit';      Cmd = { node --test "tests/unit/*.test.mjs" } },
  @{ Name = 'integration';         Cmd = { node --test "tests/integration/*.test.mjs" } }
)

$summary = @()
foreach ($s in $stages) {
  Write-Host "=== $($s.Name) ==="
  $sw = [Diagnostics.Stopwatch]::StartNew()
  & $s.Cmd
  $code = $LASTEXITCODE
  $sw.Stop()
  $status = if ($code -eq 0) { 'PASS' } else { 'FAIL' }
  $summary += ('{0,-4} {1,-20} {2:N1}s' -f $status, $s.Name, $sw.Elapsed.TotalSeconds)
  if ($code -ne 0) {
    Write-Host ''
    $summary | ForEach-Object { Write-Host $_ }
    exit 1
  }
}

Write-Host ''
$summary | ForEach-Object { Write-Host $_ }
Write-Host 'ALL STAGES PASSED'
exit 0
