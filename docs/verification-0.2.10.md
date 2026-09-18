# v0.2.10：Claude 分类与 Desktop 验证记录

日期：2026-09-18。开发机 macOS Intel。基线提交 `4b75985`。

## 上游依据

研究 cc-switch 固定提交 [`06082e189d65e6d6dbadc35dacdac1ce6c79d89a`](https://github.com/farion1231/cc-switch/tree/06082e189d65e6d6dbadc35dacdac1ce6c79d89a)：

- [`services/provider/live.rs`](https://github.com/farion1231/cc-switch/blob/06082e189d65e6d6dbadc35dacdac1ce6c79d89a/src-tauri/src/services/provider/live.rs)：Claude Code 供应商写入 settings.json。
- [`claude_desktop_config.rs`](https://github.com/farion1231/cc-switch/blob/06082e189d65e6d6dbadc35dacdac1ce6c79d89a/src-tauri/src/claude_desktop_config.rs)：`apply_provider_to_paths_inner`、`build_gateway_profile`、`write_meta` 及平台目录解析，说明 Desktop 通过 deploymentMode、configLibrary profile 和 appliedId 使用第三方网关。

本项目独立实现上述配置格式，并复用自身事务、备份、环境勾选和代理。未复制 cc-switch 的完整实现，未加入其模型映射、协议转换和 OAuth 服务。没有写入其 `coworkEgressAllowedHosts: ["*"]` 设置。

本机 `/Applications/Claude.app` 版本 2.2553.0 的只读资源检查确认存在对应 deploymentMode、configLibrary、inferenceGatewayBaseUrl、inferenceGatewayApiKey、inferenceModels 字段及网关 sessionEnvVars 逻辑。未修改 App 文件。Desktop 的自定义用户目录测试入口需要应用自身签发的测试授权；未绕过该限制，也未用生产配置冒充隔离 GUI 验收。

## 实现

新增第七个客户端 `claude-desktop`，共用 `astra-claude` 指令但独立会话命名空间。迁移旧环境设置和旧部署清单，记录文件归属，Desktop 两个目录共同部署和还原。供应商新增默认兼容的 `category` 字段，OpenAI / Codex 和 Claude 独立选择、转发；Claude 的错误不触发 Codex 供应商或模型回退。

Claude Code 和 Desktop 写入所选 Claude 类 Key 和模型，当前供应商编辑、切换与排序同步已勾选且已部署客户端。Desktop `/models` GET 与 Messages/count_tokens 共用生产核心，移除独立路径前缀并替换为当前 Claude 认证；不再误返回健康检查文本。

## 验证

- 隔离文件测试覆盖：Code / Desktop 同时部署、四份 Desktop 配置、原偏好/MCP/profile 保留、初始字节恢复、缺失文件删除、重复部署、当前供应商同步、未勾选客户端不变、JSON 错误、模型和 Key 缺失、路径重叠、符号链接、外部修改保护、旧六客户端设置/清单迁移、平台候选目录。
- HTTP 集成测试覆盖：模型 GET 保留查询参数且不发送 JSON body；Desktop 路径转发；启用本地回执、续聊、跨客户端和新会话隔离；两分类独立切换与 Key 隔离；缺失分类明确报错；Messages 验收拒绝 HTTP 200 的 OpenAI 响应。
- 原生 Claude 引擎测试使用已安装 CLI、临时 HOME、假 Key、本机随机端口。验证 Code 路径，以及从 Desktop profile 提取认证并按 Desktop sessionEnvVars 形式运行原生引擎的路径；两者验证启用、续聊和新会话隔离。**这不是完整 Desktop GUI 端到端测试。**
- 隔离界面使用真实 HTML/CSS/JS 和模拟 IPC：确认 Claude 分类筛选、两类当前供应商、从 Claude 筛选新增的默认分类、左右方向键切换分类与提示、保存后 Claude 卡片归类，以及第七项 Claude Desktop 勾选保存。文件写入由 Rust 测试验证，界面模拟回显不作为推理证据。

最终验证结果：

- `cargo test --manifest-path src-tauri/Cargo.toml`：64 项通过，3 项可选原生客户端测试默认忽略。
- 单独执行 Claude 原生引擎测试：3 项通过（Code、Desktop profile 网关路径及凭据回归）。
- Apple Silicon `aarch64-apple-darwin` 与 Windows x64 `x86_64-pc-windows-msvc` 的 `cargo check --tests` 均通过；macOS 依赖存在既有的 unnecessary unsafe 警告。
- 前端两个 JS 语法检查、Rust 格式、Tauri JSON、macOS/Windows shell 脚本语法及 `git diff --check` 均通过。
- 修复新增分类控件挤占基础信息网格的问题：分类移入右侧表单并横跨两列，名称/备注正常并排，官网占满一行。浏览器在正常宽度、600px、420px 三种宽度下验证通过，420px 自动改为单列。

日志及源码备份位于 `artifacts/claude-desktop-0.2.10/`，不进入 Git。未重新打包 App 或生成安装程序。

## 使用边界

需要供应商支持 Anthropic Messages 与所选模型。验收按钮由用户主动触发短请求；本次自动测试使用本地模拟上游，不声称已验证任意真实供应商。未自动更改当前客户端选择和真实认证；未停止现有代理，未替换正在运行的 App。Windows / Apple Silicon 的编译检查不能代替真机运行与 NSIS 安装验收。

更新构建后，将 Claude 供应商设为 Claude 分类，勾选客户端和 Claude 指令，部署后启动代理，再完整退出重启 Claude。停止代理并仅勾选 Desktop 后点击还原可恢复两个目录的首次备份；未选客户端不受影响。
