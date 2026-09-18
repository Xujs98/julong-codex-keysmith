# v0.2.13 验证记录

## 变更范围

- 部署确认弹窗使用固定高度的变更列表滚动区，标题和操作区不随列表内容撑开。
- 列表有溢出时确认按钮保持禁用，滚动到底部后解锁；无溢出时直接解锁。
- 滚动区域可键盘聚焦，弹窗支持 `Esc` 关闭并恢复打开弹窗前的焦点。
- 同步更新 `VERSION`、前端展示、Node/Rust/Tauri 配置版本为 `0.2.13`。

## 自动化检查

```text
/Users/xujs/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/bin/node --check frontend/app.js
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml
python3 -m json.tool src-tauri/tauri.conf.json
npm run test:scroll-gate
```

预期：前端语法、Rust 格式、配置格式和滚动门控测试通过；Rust 测试中需要本机客户端可执行文件的集成测试允许保持忽略。

## 回滚

恢复 `artifacts/deploy-preview-scroll/original/` 中记录的原始文件，并将版本号恢复为 `0.2.12`；该目录保存改动前 SHA-256，便于逐文件校验。
