@echo off
setlocal
chcp 65001 >nul
cd /d "%~dp0.."
where node >nul 2>nul
if errorlevel 1 (echo 请先安装 Node.js 20.11 或更新版本。 & pause & exit /b 1)
where cargo >nul 2>nul
if errorlevel 1 (echo 请先安装 Rust stable 的 MSVC 工具链，并将 cargo 加入 PATH。 & pause & exit /b 1)
if not exist node_modules (call npm ci)
if errorlevel 1 (pause & exit /b 1)
call npm run dev:desktop
if errorlevel 1 (pause & exit /b 1)
