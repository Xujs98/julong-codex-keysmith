# v0.2.11：Claude 供应商模型映射

日期：2026-09-18。基线：`1b5529c`。开发机：macOS Intel。

## 上游研究

使用 cc-switch 固定提交 [`06082e189d65e6d6dbadc35dacdac1ce6c79d89a`](https://github.com/farion1231/cc-switch/tree/06082e189d65e6d6dbadc35dacdac1ce6c79d89a)，对照用户截图：

- `src/components/providers/forms/ClaudeFormFields.tsx`：Sonnet、Opus、Fable、Haiku、Subagent，菜单名称、实际模型、1M 声明、默认兜底。
- `src/components/providers/forms/hooks/useModelState.ts`：角色环境变量与 `[1M]` 能力标记。
- `src-tauri/src/proxy/model_mapper.rs`：角色映射、Fable → Opus、Subagent 与默认兜底、转发前剥离 1M 后缀。
- `src-tauri/src/claude_desktop_config.rs`：`proxy_model_routes`、`inference_model_json`、`model_list_response`、`map_proxy_request_model`。Desktop 菜单使用兼容的 Claude 角色 ID，代理转发为上游模型名；显示名称和 `supports1m` 使用 profile 对象字段。

本项目独立实现共享映射模块，未引入上游框架或整段复制源码。沿用现有 Messages 传输，没有实现 OpenAI Responses / Chat Completions → Anthropic 协议转换。

## 实现与兼容性

- 供应商新增可选 `claude_models`；旧记录缺少该字段时从默认模型和旧模型列表推导角色。显式空映射不会被已下载模型列表重新填满。
- Code 写入各角色实际模型、菜单名称、Subagent、默认模型；清理前供应商遗留映射，保留其它设置，仍由首次备份支持还原。
- Desktop 使用 Sonnet / Opus / Fable / Haiku 路由 ID 及显示名称，解除实际模型必须以 `claude-*` 开头的限制。profile 与 `/claude-desktop/v1/models` 目录保持一致。
- Messages 与 count_tokens 使用同一映射。Code 已替换的实际角色/Subagent 模型优先保留；Desktop 的显式路由优先，避免与其它角色实际模型重名时绕过映射。Fable 留空回落到 Opus，然后兜底；兜底空则保留原名。
- 1M 声明用于客户端，出站一律剥离 `[1M]` / `[1m]`；不提高真实上下文能力。
- 原生 Code 使用非 Claude 模型时可能在 messages 末尾增加 system-role 环境信息。Messages 转发将其合并到顶层 system，保留文本块与 cache_control；启用判定忽略尾部 system，但不跳过 assistant/tool 消息，也不接受附件和历史示例触发。
- 映射编辑、供应商切换沿用事务写入。macOS / Windows 共用 Rust 实现，没有新增资源或平台命令；现有构建脚本和安装资源清单不需要调整。

## 验证结果

- Rust 常规测试：71 项通过，3 项可选原生客户端测试默认忽略。
- 单独原生 Claude 测试：3 项通过。Code 直接使用 `gpt-6-astra[1M]`，Desktop profile 使用 Claude 角色路由；本地模拟上游收到 `gpt-6-astra`，无本地能力后缀、无 messages 内 system 角色；启用、续聊及新会话隔离通过。
- 集成覆盖：角色和兜底、Fable 回落、Subagent 不被覆盖、日期后缀、count_tokens、Desktop 目录、流式响应、工具结果/工具定义/thinking 字段保持、运行时切换模型与 Key、空供应商错误、任意品牌模型配置、旧数据反序列化、重复部署及首次备份原字节还原。
- 前端隔离模拟 IPC 验证：一键设置、下载模型、角色下拉键盘选择、自定义名称与 1M 保存/重新打开、正常及 420px 窗口布局。界面使用自定义模型下拉与复选框样式；保存按钮保持固定，内容可滚动。
- Apple Silicon `aarch64-apple-darwin` 与 Windows x64 `x86_64-pc-windows-msvc` 的 `cargo check --tests` 通过。既有 macOS Tauri 依赖的 unnecessary unsafe 警告未新增。
- 两个前端 JS 语法、Rust 格式、Tauri JSON、构建 shell 语法、版本同步与 Git diff 检查通过。

日志和初始源码备份位于忽略目录 `artifacts/claude-models-0.2.11/`。本轮测试没有使用真实供应商 Key、未发送付费推理请求、未更改生产客户端配置或中断当前代理。原生引擎/profile 测试不等同于完整 Desktop GUI 真机验收，也不代替 Windows 安装程序验收。未主动重新打包 App。

## 使用

重新构建应用后，编辑 Claude 类供应商，将兜底设为供应商实际模型（如 `gpt-6-astra`），一键设置四档并按需调整各档、Subagent 与 1M，保存后部署和重启客户端。供应商必须支持 Anthropic Messages。短请求验收仅验证当前兜底或首个映射模型；不同档位模型的权限、工具能力和流式兼容性仍以真实供应商验收为准。
