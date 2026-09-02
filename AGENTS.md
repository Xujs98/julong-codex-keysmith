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
