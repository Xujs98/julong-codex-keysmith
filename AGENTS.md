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
