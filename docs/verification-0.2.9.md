# v0.2.9 验证记录

日期：2026-09-17。开发机：macOS Intel（x86_64）。修改前提交：`fac844d`。

## 修复内容

Claude Code 原有环境部署只生成说明文件和指令存档，没有配置 `settings.json`。现在桌面部署、启动代理和 CLI 共用环境部署实现，合并本机代理地址及客户端标识，保留原有认证、模型、权限、hooks 和其它字段。首次备份保留原始字节，重复部署不覆盖备份；按选择还原包含 Claude settings，未选客户端保持原状。

Anthropic Messages 与 count_tokens 转发使用代理当前供应商认证；切换供应商时地址和认证同时更新。识别原生 Claude 会话头，并兼容客户端在独立用户文本块前加入的环境提醒。内联引用、附件、工具结果和历史口令不触发启用。

## 基线与回归证据

- 基线测试复现：部署后 `ANTHROPIC_BASE_URL` 仍为原地址，预期本机代理地址的断言失败（退出码 101）。修复后同项通过。
- 常规 Rust 测试：42 项单元测试、13 项集成测试，共 55 项通过；依赖本机安装客户端的两项测试默认忽略。
- Claude settings 测试覆盖：已有配置合并、原始字节还原、重复部署、首次创建后删除、取消选择、无效 JSON/字段类型、外部修改冲突、符号链接拒绝以及包含中文和空格的路径。
- 认证回归：Messages 和 count_tokens 使用更新后的供应商 Key，Responses 请求保留既有认证语义；本地占位令牌缺少供应商 Key 时返回错误。
- 真实 Claude Code 2.1.274 验收：在临时用户目录加载生成的 settings，使用生产代理路由和随机本机端口。单独发送“矩龙”收到准确回执且不访问模拟上游；恢复同一会话时指令生效；新会话不继承启用状态。该测试及认证测试共 2 项通过。
- 当前 Intel Mac 的 `claude --version` 返回 2.1.274，原生程序为 Mach-O x86_64。截图中的原生组件缺失错误在检查时未复现，没有重装或修改可执行程序。

## 平台与静态检查

| 检查 | 结果 |
| --- | --- |
| macOS Intel `cargo test --manifest-path src-tauri/Cargo.toml` | 通过 |
| Apple Silicon `cargo check --manifest-path src-tauri/Cargo.toml --target aarch64-apple-darwin --tests` | 通过 |
| Windows x64 `cargo check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --tests` | 通过 |
| Rust 格式检查 | 通过 |
| `frontend/app.js`、`frontend/environments.js` 语法检查 | 通过 |
| Tauri JSON 格式检查 | 通过 |

macOS vendor 代码仍有既有的 unnecessary unsafe 警告，无编译错误。版本字段同步为 0.2.9；README 保留历史版本章节，并补充两种 Mac 架构及 Windows 的使用和 npm 原生依赖修复说明。构建脚本和安装资源清单未变更。

## 验证边界与回滚

真实 CLI 测试只使用临时目录、假 Key 和本地模拟上游，不证明远端供应商支持某一 Claude 模型。远端必须支持 Anthropic Messages。未进行 Windows 或 Apple Silicon 真机运行、Windows 安装包验收，也未重新打包或替换现有 App；更新 App 后需重新部署并重启代理及 Claude。

运行时还原：停止代理，勾选 Claude Code，点击“还原配置”；恢复首次部署前的 settings 字节，原本没有 settings 时删除生成文件。部署后的外部修改会阻止覆盖，需先保留并处理冲突。

源文件基线备份、失败基线、完整回归及平台日志位于本地 `artifacts/claude-integration-0.2.9/`，不进入 Git。验证复用上一任务已完成的测试记录；续接任务仅补充文档与发布检查。
