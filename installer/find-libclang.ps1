# Points LIBCLANG_PATH at an LLVM install (bindgen needs it for the ASIO SDK headers),
# installing LLVM with Chocolatey when the runner image has none.
$ErrorActionPreference = 'Stop'
$candidates = @("$env:ProgramFiles\LLVM\bin", "C:\Program Files\LLVM\bin")
$found = $candidates | Where-Object { Test-Path (Join-Path $_ 'libclang.dll') } | Select-Object -First 1
if (-not $found) {
    choco install llvm -y --no-progress
    $found = $candidates | Where-Object { Test-Path (Join-Path $_ 'libclang.dll') } | Select-Object -First 1
}
if (-not $found) { throw 'libclang.dll not found after installing LLVM' }
"LIBCLANG_PATH=$found" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
Write-Host "LIBCLANG_PATH=$found"
