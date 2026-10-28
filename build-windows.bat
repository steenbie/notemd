@echo off
REM Builds nimblenote.exe. Needs Rust from https://rustup.rs (installs the MSVC build tools too).
where cargo >nul 2>nul || (echo Rust is not installed. Get it from https://rustup.rs & exit /b 1)
cargo build --release || exit /b 1
echo.
echo Done: %~dp0target\release\nimblenote.exe
