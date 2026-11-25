# Puts CMake and libclang (installed with `pip install --user cmake libclang`)
# on this shell's environment, needed to build whisper.cpp (whisper-rs).
# Usage:  . .\tools\dev-env.ps1 ; cargo tauri dev
$scripts = python -c "import sysconfig, os; print(sysconfig.get_path('scripts', os.name + '_user'))"
$env:PATH = "$scripts;$env:PATH"
$env:LIBCLANG_PATH = python -c "import clang, os; print(os.path.join(os.path.dirname(clang.__file__), 'native'))"
Write-Host "cmake:" (Get-Command cmake).Source
Write-Host "LIBCLANG_PATH:" $env:LIBCLANG_PATH
