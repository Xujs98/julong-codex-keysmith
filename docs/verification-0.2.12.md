# v0.2.12 验证记录

## 目标

修复部署文件被外部修改后，普通部署和普通还原都被完整性保护拒绝，导致用户没有可操作的恢复路径。默认保护仍保留，覆盖外部修改必须由用户在界面中明确确认。

## 实现检查

- 预览阶段不再因外部 SHA-256 漂移直接失败；配置页读取 `deployment_conflict` 并把确认按钮改为“覆盖修改并部署”。
- 冲突状态下显示“覆盖修改并还原”；未检测到冲突时该按钮保持隐藏。
- 强制部署和强制还原共享现有文件事务与恢复日志；普通 `deploy` / `restore`、CLI 和启动代理仍走完整性保护。
- 强制操作仍执行路径、符号链接、配置、供应商和模型指令校验，不绕过输入校验。

## 命令结果

- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`：通过。
- `cargo test --manifest-path src-tauri/Cargo.toml`：通过，新增显式强制部署/还原测试与现有测试全部通过。
- `python3 -m json.tool src-tauri/tauri.conf.json`：通过。
- `node --check frontend/app.js`：当前 macOS 开发机未安装 `node`/`nodejs`，未执行；需在带 Node.js 的环境补跑。

## 范围

未重新打包 macOS App 或 Windows NSIS 安装程序。Windows 完整安装包仍需在 Windows 目标机运行现有 PowerShell/CMD 构建流程后验收。
