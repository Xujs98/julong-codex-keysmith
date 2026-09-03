# Project Maintenance Guide

本项目是基于上游项目的二次开发版本，由 Xujs98 的个人仓库持续维护。

## GitHub 发布约定

- 个人维护仓库：`https://github.com/Xujs98/julong-codex-keysmith.git`
- 每次完成代码或文档改动后，先运行必要的检查，再创建本地 Git 提交。
- 本地提交完成后，将当前分支推送到个人仓库的 `main` 分支。
- 推送遇到网络错误时只自动重试一次；第二次仍失败则保留本地提交，并在交接或下一次任务中继续推送。
- 不要覆盖或删除用户已有的未提交改动；提交前检查 `git status` 和 `git diff`。

## 验证基线

- 前端：`node --check frontend/app.js`
- Rust：`cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
- Rust 测试：`cargo test --manifest-path src-tauri/Cargo.toml`
- 配置：`python3 -m json.tool src-tauri/tauri.conf.json`

## 维护原则

- 继续使用现有 Rust + Tauri + 原生 HTML/CSS/JS 技术栈。
- 保持 macOS、Windows 构建脚本与 README 教程同步。
- 新增功能时同时更新 README 的功能模块、构建说明和验证记录。

## CLI 控制台入口（待实现）

- 新增独立命令 `julong-codex start`，完成代理部署并监听 `127.0.0.1:8080`。
- 新增独立命令 `julong-codex stop`，停止代理并恢复 Codex 原始配置。
- 新增独立命令 `julong-codex status`，显示代理进程、8080 端口、部署状态和中转站地址。
- CLI 与 Tauri 桌面端共享同一套部署、恢复、端口和状态逻辑，避免双重实现造成状态不一致。
- 同步更新 macOS、Windows 的 CLI 安装与使用说明，并为 `start`、`stop`、`status` 增加可重复执行的验证记录。
- CLI 功能完成后由用户自行运行 macOS/Windows 构建；代理执行任务期间不主动重新打包 App。

## macOS / Windows 双平台适配

- 所有功能、修复和重构都必须同时考虑 macOS 与 Windows，保持同一套 Rust/Tauri 核心逻辑可编译、可运行。
- 涉及路径、环境变量、进程、端口、文件权限、系统托盘、窗口行为和外部命令时，优先使用跨平台 API，并为平台差异提供明确分支。
- macOS 使用 `.app` / `.dmg` 构建链路，Windows 使用 `.exe` / NSIS / MSI 构建链路；不得用一端脚本或路径覆盖另一端配置。
- 修改构建、安装、部署或恢复流程时，同步检查 `build-macos.sh`、`build-windows.sh`、Windows PowerShell/CMD 脚本及 README 文档。
- 每次跨平台改动至少验证 Rust 格式、Rust 测试、前端语法和配置格式；具备对应工具链时分别执行 macOS 与 Windows 构建前检查。
- 提交前检查平台专属条件编译、资源路径和打包资源清单，避免出现“一端可用、另一端启动失败”的回归。

## Windows 完整安装程序

- Windows 版本的正式交付物必须是完整的 `.exe` 安装程序，裸 `矩龙破甲.exe` 仅作为调试或辅助文件，不作为最终发布方式。
- 安装程序必须携带并正确安装所有运行时资源，包括 Tauri 资源目录、`bridge.md`、`codex-skills/`、前端静态资源、图标及应用所需的配置模板。
- Windows 构建优先生成 NSIS `.exe` 安装包；需要 MSI 时作为附加格式，EXE 安装程序仍是必需交付物。
- 安装后必须从全新目录验证：开始菜单/桌面快捷方式、卸载入口、资源完整性、应用启动、代理启动及 `julong-codex status` 均可用。
- 修改资源清单、安装路径或 Windows 构建脚本时，同步更新 `build-windows.ps1`、`build-windows.cmd`、`build-windows.sh` 和 README，并记录安装包验证结果。

## 当前开发机与命令输出

- 当前开发机是 macOS；面向用户给出的可直接执行命令默认使用 macOS 的 zsh/bash 语法和项目 macOS 脚本。
- Windows PowerShell/CMD 命令仅在明确标注“Windows 目标机”时展示，不作为当前 macOS 的操作指令。
- 在 macOS 上涉及 Windows 时，说明 `build-windows.sh` 的交叉编译边界；完整 NSIS `.exe` 安装包的构建步骤单独标注为 Windows 目标环境流程。
