# v0.2.16 听话分组文档验证

## 本次变更

- 核对日期：2026-10-18（Asia/Shanghai）。
- README 改为说明：中转站自带“听话分组”时，无需安装、部署或启动矩龙破甲。
- 保留推荐中转地址 `https://api.julongkj.top`、模型 `gpt-5.6-sol-大白话` 和分组名称 `矩龙大白话`。
- 版本号从 `0.2.15` 同步递增至 `0.2.16`。

## 已执行检查

| 检查 | 实际结果 |
|---|---|
| `node --check frontend/app.js` / `frontend/environments.js` | 通过 |
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | 通过 |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 77 通过、0 失败、3 忽略 |
| `python3 -m json.tool`（Tauri、sidecar、inkos、package） | 通过 |
| `git diff --check` | 通过 |

本次只调整公开文档和版本标识，没有改变代理激活逻辑、启动用户代理或重新打包 App。
