# v0.2.6 验证记录

验证主机：macOS x86_64。上游固定提交：`2f75eeacd02bb67008e5a0c28dd52ceb5b8344a2`。原项目修改前为 `7aa7269`，原件保存在独立工作树与 BASELINE.tar；所有实现先写在副本。

## 已观察的结果

- 原版 Rust 基线：38 个单元测试通过。
- 修改版：40 个单元测试与 5 个集成测试通过；无失败。
- 前端 `app.js`、`environments.js` 的 `node --check` 通过；Rust 格式及 Tauri JSON 格式通过。
- CLI 二进制编译通过；隔离状态目录的 `environment list`、两次 `environment deploy` 均退出 0；未选中 Grok 的目录没有写入说明文件。
- Windows `cargo check --target x86_64-pc-windows-msvc` 通过。首轮缺少 PATH 中的 `llvm-rc`，加入本机已有 LLVM 路径后通过。
- macOS / Windows shell 构建脚本语法检查通过。既有资源递归复制覆盖六包，源文件与资源副本 SHA-256 一致；未改变各平台的包格式或资源映射。
- 浏览器使用隔离 IPC fixture 验证六环境卡片、中文/空格手动路径、自动识别返回路径、多模型勾选、保存后焦点、Enter 发送和 Escape 关闭。此项只验证 UI；程序开关由下面的真实 HTTP 测试验证。

## 程序开关与部署闭环

生产 Rust HTTP 路由配合本机临时上游，覆盖 Codex Responses、Claude Messages、Grok / DeepSeek / GLM Chat Completions、Gemini 消息格式，以及 JSON / SSE 回执。

- **BASELINE**：会话未启用，转发到临时上游的请求没有模型包。
- **MODIFIED**：单独发送 `矩龙`，程序返回 `把每一次交互，变成可控能力`，上游请求计数不增加；同会话的下一条请求带对应包。其它会话、凭据或环境不继承该状态。
- **ROLLBACK**：六个原生说明文件逐字节还原；取消选择、迁移目录和重复部署验证通过。程序会话重置和过期后不再注入。
- 历史口令、开发者消息、工具输出、混合图片消息和“文档示例：矩龙”均不会触发新会话启用。
- 缺少模型包、相对路径、重复目录、符号链接目标、部署后用户改动均中止写入或还原；保留用户字节。
- 人工构造中断事务后，日志恢复原始文件并删除事务新增文件，随后可正常还原。

## 验证范围

本地 `127.0.0.1:8080` 已被既有代理占用，`scripts/verify-cli.sh` 返回 77（SKIP），没有停止该代理或改写正在使用的客户端配置。HTTP 集成测试使用操作系统分配的空闲端口。

没有重新打包 App，也没有在 Windows 上运行 NSIS 安装程序。没有调用真实供应商测试六种远端模型；本地成功回执只证明会话开关。外部客户端必须接入代理且提供会话标识。未知客户端的专有配置不自动覆盖。

完整命令、标准输出、标准错误、退出码、文件哈希与源码回滚验证保存于工作区 `artifacts/multi-environment-0.2.6/VERIFICATION.txt` 及其引用的 JSON 记录。
