# Puts CMake and libclang (installed with `pip install --user cmake libclang`)
# on this shell's environment, needed to build whisper.cpp (whisper-rs).
# Usage:  . .\tools\dev-env.ps1 ; cargo tauri dev
$scripts = python -c "import sysconfig, os; print(sysconfig.get_path('scripts', os.name + '_user'))"
$env:PATH = "$scripts;$env:PATH"
$env:LIBCLANG_PATH = python -c "import clang, os; print(os.path.join(os.path.dirname(clang.__file__), 'native'))"
Write-Host "cmake:" (Get-Command cmake).Source
Write-Host "LIBCLANG_PATH:" $env:LIBCLANG_PATH

# Signing key of the updater (created once with `cargo tauri signer generate`
# into %USERPROFILE%\.tauri, with its password in nihongo-michi.senha), needed
# by `cargo tauri build`. Both stay outside the repository.
$keyDir = Join-Path $env:USERPROFILE ".tauri"
$key = Join-Path $keyDir "nihongo-michi.key"
$pass = Join-Path $keyDir "nihongo-michi.senha"
if ((Test-Path $key) -and (Test-Path $pass)) {
    $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content $key -Raw).Trim()
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = (Get-Content $pass -Raw).Trim()
    Write-Host "updater key: ok"
}
