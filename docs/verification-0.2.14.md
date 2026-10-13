# v0.2.14 增量集成验证

## 来源与变更

- 核对日期：2026-10-14（Asia/Shanghai）。
- 原来源提交：`0ad8ec58e1989f4a058e01ce4e15cf226e8067bf`。
- 新来源提交：`3ab84df64468bb74cc9eb4e5423660083953c44f`，上游提交时间 2026-10-04。
- 新增 Astra v2 RC1（7931 bytes）与 6.1 Sol v1 RC2（7575 bytes）。
- 旧包和旧证据身份保留；新包标为预发布，B 分范围记录，C 未运行。
- 新测试库八个英文 biology 样例取消字符上限；更新评分器、续作探针与 JB 工具归档。
- 版本配置与界面统一为 0.2.14。未启动或停止用户代理、未修改当前用户模型选择、未重新打包 App。

## 已执行检查

| 检查 | 实际结果 |
|---|---|
| `node --check frontend/app.js` / `frontend/environments.js` | 通过 |
| `npm run test:scroll-gate` | 4/4 通过 |
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | 通过 |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 77 通过、0 失败、3 忽略 |
| `python3 -m json.tool src-tauri/tauri.conf.json` | 通过 |
| `bash -n build-macos.sh build-windows.sh` | 通过 |
| `npm run verify:upstream` | 41 个来源资源通过哈希与字节数校验，四份提示词和两代测试库通过 |
| 资源复制后对 `src-tauri/resources` 重复同一校验 | 通过 |
| 上游脚本 ZIP 解压源码语法编译 | 全部通过 |
| 最新 issue generator 在临时目录生成测试库 | 与内置新测试库逐字节相同 |
| 上游 `verify_gpt56_sol_regression_scoring.py` | 49/49 离线测试通过 |
| 临时 CODEX_HOME / JULONG_HOME 下 CLI list、lab status、两个 RC set/check | 六条命令退出码均为 0 |
| `git diff --check`（排除逐字节保留的 `instruction-lab/upstream/`） | 项目改动通过；上游 README 保留原有 3 处行尾空白 |
| `cargo xwin check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --tests` | 通过（5m41s） |

Rust 测试包括新包渲染、哈希、设置持久化、Codex 环境路由、同槽位冲突、预发布门禁、
旧证据兼容、fresh A 人工复核缺失及评分方法混用拒绝。
3 个忽略项需要本机 Codex/Claude 客户端可执行文件，未作为通过计数。
本机 Node 不在初始 PATH，实际使用 `/Users/xujs/.nvm/versions/node/v24.13.0/bin/node`。
现有 vendored Tauri macOS 代码产生 5 条 `unused_unsafe` 警告，未影响测试通过。

CLI 实测摘要：

```text
Astra v2 RC1: source PASS; structure 7/7; banks PASS
A 3/4 + fresh runs 2; B 42/50 cases, 48/56 turns; C NOT RUN
6.1 Sol RC2: source PASS; structure 7/7; banks PASS
A 3/4 + fresh runs 2; B 34/50 cases, 40/56 turns; C NOT RUN
两者: release prerelease; hard gate INCOMPLETE; production DEPLOYABLE
```

`DEPLOYABLE` 仅表示显式发布快照满足本地完整性部署条件，不表示远端 A/B/C 全过。

## 平台与评测边界

Windows x64 MSVC 交叉编译（含测试代码）预检通过，日志保存在 `/tmp/julong-windows-check-20261014.log`。
四个构建入口均已补充新包及历史证据资源存在性检查；
既有 Tauri 递归资源映射无需变更。未生成 macOS DMG 或 Windows NSIS 安装包，未进行 Windows
目标机安装、快捷方式及卸载验收。上游 JB 冻结数据和原始结果未提交，未执行远端模型或官方 judge。

## 备份与回滚

改动前工作区干净，基线提交为 `c4241ef9230458afb8cb8a0210d5a7fa57bd74ea`。
完整跟踪文件备份位于 `/tmp/julong-before-gpt-instruct-20261014.tar`，可解压到新目录比较。
发布后需要代码回滚时使用本次集成提交的 Git revert；运行时可显式选回保留的 Astra v1 或
5.6 v45，再按原有部署/恢复流程执行。历史证据没有迁移或重评分。
