# v0.2.7 验证记录

验证日期：2026-09-17。主机：macOS x86_64。修改前提交：`9dbfc50`。

## 会话识别问题与修复

v0.2.6 的启用开关只查找 `x-julong-session`、`session_id` 等字段，没有识别 Codex 原生的 `thread-id` / `session-id`。单独发送口令时，代理会返回“缺少会话标识”。旧验收手动添加了矩龙会话头，未覆盖原生客户端请求。

修复前运行新增 HTTP 回归测试，复现 `x-julong-activation` 实际为 `inactive`、预期为 `active` 的失败。修复新增两种原生头，显式矩龙头仍优先，Codex 原生头优先使用 `thread-id`。同一对话的 `session-id` 改变不会丢失启用状态；不同对话即使共享 `session-id` 也不会互相启用。无会话标识的请求仍不启用。

## 已完成检查

- `cargo test --manifest-path src-tauri/Cargo.toml`：40 项单元测试、6 项集成测试通过；附加原生客户端测试默认忽略，另行执行结果如下。
- 本机已安装的 `codex-cli 0.154.0-alpha.6.2`：通过 `JULONG_CODEX_TEST_BIN` 指定原生可执行文件，运行 `cargo test --manifest-path src-tauri/Cargo.toml --test codex_client -- --ignored --nocapture`，1 项通过。
- 原生客户端测试使用隔离 Codex 配置目录、临时工程、本机空闲端口、测试凭据和固定上游响应。直接连接生产 Rust 代理路由，实际观察到 `session-id`、`thread-id`，没有手动添加矩龙请求头。日志仅输出头名称，不输出认证值、会话值或请求正文。
- 原生客户端单独发送口令，输出文件准确收到程序启用回执，且没有上游调用；恢复同一对话的后续请求包含测试模型包；新建对话的请求不包含该包。两个后续请求均被原生客户端正常读取并结束。
- `node --check frontend/app.js`、`node --check frontend/environments.js`、Rust 格式、Tauri JSON 格式、macOS/Windows shell 构建脚本语法和 `git diff --check` 通过。
- `cargo check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --tests` 通过；使用本机已有 LLVM 资源编译器，包含新增测试的 Windows 编译检查。
- VERSION、Node 包及锁文件、inkos.json、Cargo 包及锁文件、Tauri 配置和前端显示版本均为 0.2.7；历史验证记录保留原版本。

## 供应商测试隔离修复

首轮完整测试另发现旧测试仅设置 `CODEX_HOME`，但保存的环境选择优先级更高，导致测试写入真实 Codex 目录。首个测试因临时目录认证文件仍为空而失败，第二个测试因互斥锁中毒失败。

已将供应商激活和模型回退的文件操作提取为接收显式目录的函数。测试直接传入临时目录，不再修改全局环境变量；生产入口只解析一次目录，并在同一目录更新部署完整性。

受影响的模型、供应商名称和中转地址已按保存的供应商记录恢复，认证从与该记录一致的已有备份恢复，配置完整性哈希同步更新，其余配置字段保留。恢复前快照保存在本机 Codex 目录的 `julong-test-recovery-20260917-1801/`，不进入 Git。重新运行完整测试后，真实 `config.toml`、`auth.json`、`relay_url.txt` 和部署清单的 SHA-256 均与恢复后测试前一致。

## 验证范围与本机记录

此记录验证本机 Codex 核心、生产代理开关及隔离上游链路，没有调用真实远端模型，也没有测试其它五种客户端的原生会话格式。没有重新打包或替换已安装 App；旧代理进程需更新二进制并重启才能生效。Windows 安装程序运行验收仍需在 Windows 目标机执行。

修改前源文件副本和执行日志保存在 `artifacts/codex-session-0.2.7/`：`baseline/`、`rust-tests.log`（首轮失败）、`rust-tests-final.log`、`native-client.log`、`windows-check.log`。这些本机记录不包含生产 API Key。
