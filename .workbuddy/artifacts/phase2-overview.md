# dfy-ops 第二阶段修复概览（2026-09-05，commit `6f4ec15`）

## 范围（四条绑定推进）
P1-3 Unix 进程树 kill · P1-4 子进程回收 + 存活真实性 · P0-2 running 三态化 · P1-5 bind 不再静默 panic

> 这四条是绑定的：没有 P1-3/P1-4，P0-2 的「真实存活」就没有可靠的回收支撑。

## 改动清单（8 文件，+124 / -18）

### 后端 Rust
- `src-tauri/src/ops/process.rs`
  - **P1-3**：`spawn_detached` 在 Unix 侧 `command.process_group(0)`（std `CommandExt`，零新依赖），让 shell 自成进程组；`stop()` 用 `kill -KILL -<pgid>` 杀整组，java/nginx 等孙进程不再变孤儿。
  - **P1-4**：`is_running` 改 `try_wait` 真实探活——退出即回收僵尸并移除记录，崩溃进程不再误报 running，也不留僵尸占用 PID 表。
  - kill/taskkill 的 stderr 重定向 null，消除「已退出进程」产生的 EPERM/ESRCH 噪声。
- `src-tauri/src/api/handlers.rs`
  - **P0-2**：新增 `RunState(managed/detected/down)` 枚举 + `ServiceStatus.run_state` 字段；`to_status` 以受管进程真实存活为 managed、端口探活兜底为 detected、皆无为 down；`running` 派生为「非 down」保持前端兼容且真实。
- `src-tauri/src/lib.rs`
  - **P1-5**：运维 API 的 `Runtime::new` / `TcpListener::bind` / `axum::serve` 失败均改为记日志并退出 API 线程，不再 `expect` panic 拖垮整个桌面应用；端口被占用即天然单实例互斥。

### 前端 TS/TSX
- `src/types.ts`：新增具名 `RunState` 类型。
- `src/components/StatusBadge.tsx`：区分「运行中 / 健康」与「外部运行 / 健康」。
- `src/components/ServiceCard.tsx` · `src/pages/ServiceDetail.tsx`：透传 `runState`。
- `src/pages/Topology.tsx`：`!running → down` 在新三态下正确显示「外部运行」。

## 验证结果
| 项 | 结果 |
|---|---|
| `cargo check` | 0 error |
| `cargo clippy --all-targets` | 仅 `health.rs:14` / `diagnosis.rs:125` 两条**预存** warning，本次改动零新增 |
| `cargo test` | **14 passed / 0 failed** |
| `npx tsc --noEmit` | 0 error |
| `vite build` | 成功 |

注意：未跑全局 `cargo fmt`——仓库 Rust 文件存在预存格式不一致（非本次改动区域），按你的「不碰范围外」原则，工作树仅含本次 8 个目标文件。

## 遗留（未批未动）
P1-6（api_key 落盘入 keychain）、P2 各项（契约 codegen、AiAssistant 拆分、文档端口冲突 8080↔48085、MQTT 18830 双角色、is_daemon 保活等）、CI 补 cargo test/clippy。
