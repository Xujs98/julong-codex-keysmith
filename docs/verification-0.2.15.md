# v0.2.15 文档与版本同步验证

## 本次变更

- 核对日期：2026-10-18（Asia/Shanghai）。
- README 新增推荐中转站地址 `https://api.julongkj.top`。
- README 新增 `gpt-5.6-sol-大白话` 与中转站侧激活口令 `矩龙大白话` 的使用说明。
- 版本号从 `0.2.14` 同步递增至 `0.2.15`，覆盖 `VERSION`、`inkos.json`、npm、Cargo、Tauri 配置和前端界面。

## 已执行检查

| 检查 | 实际结果 |
|---|---|
| `node --check frontend/app.js` / `frontend/environments.js` | 通过 |
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | 通过 |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 77 通过、0 失败、3 忽略 |
| `python3 -m json.tool`（Tauri、sidecar、inkos、package） | 通过 |
| `git diff --check` | 通过 |
| 版本同步检查 | 通过，项目版本为 `0.2.15` |

本次仅更新公开文档和版本标识，没有启动代理、修改用户 Codex 配置或重新打包 App。
