# dfy-ops 架构评审报告

> 评审时间：2026-09-05 ｜ 代码基线：`main@40abb88`
> 实测健康度：`tsc --noEmit` 0 error ｜ `cargo check` 0 error ｜ `cargo test` 5 passed / 0 failed ｜ `cargo clippy` 1 warning（可自动修复）

---

## 零、修复进展（2026-09-05，commit `b5ee2b5`）

| 项 | 状态 | 落点 |
|---|---|---|
| **P0-1** 阻塞调用进 async 运行时 | ✅ 已修 | `api/handlers.rs` 新增 `blocking()`（`spawn_blocking`）；端口/子进程探测、`wait_ready` 的 `thread::sleep`、日志与配置读写全部移交 blocking 池；`handlers_ai.rs` 的 `saved_config` 改 async 走 `spawn_blocking` |
| **P1-1** 「全部启动」拉起桌面客户端 | ✅ 已修 | `ops/orchestrator.rs:115` 改为「先对全量 defs 拓扑排序，再按 `is_daemon` 过滤执行顺序」，避免误判环 |
| **P1-2** 本地 API 无鉴权 + CORS permissive | ✅ 已修 | `api/server.rs` 换 origin 白名单（含 Windows 生产 `http://tauri.localhost`、macOS `tauri://localhost`、dev `:18720`）+ `require_client_header` 中间件；前端 `api.ts` 每个请求补 `X-Dfy-Ops-Client` 头 |
| P0-2 状态三态化 | ✅ 已修（第二阶段） | `api/handlers.rs` `ServiceStatus` 新增 `run_state: RunState(managed/detected/down)`；`to_status` 改 `managed`=受管进程真实存活、`detected`=端口探活兜底（识别外部/历史进程）、`down`=两者皆无；`running` 派生为「非 down」保持前端兼容且真实。前端 `types.ts`/`StatusBadge`/`ServiceCard`/`ServiceDetail`/`Topology` 透传并展示（运行中/外部运行/未运行） |
| P1-3 Unix 进程树 kill | ✅ 已修（第二阶段） | `ops/process.rs` `spawn_detached` 在 Unix 侧 `process_group(0)` 让 shell 自成进程组；`stop()` 用 `kill -KILL -<pgid>` 杀整组，java/nginx 等孙进程不再变孤儿。零新增依赖（复用 std `CommandExt`） |
| P1-4 子进程回收 + 存活真实性 | ✅ 已修（第二阶段） | `ops/process.rs` `is_running` 改 `try_wait` 真实探活：退出即回收僵尸并移除记录、仍在跑返回 true、出错保守移除。崩溃进程不再误报 running，也不留僵尸占用 PID 表 |
| P1-5 bind 不再静默 panic | ✅ 已修（第二阶段） | `lib.rs` 运维 API 的 `TcpListener::bind` / `Runtime::new` / `axum::serve` 失败均改为记日志并退出 API 线程，不再 `expect` panic 拖垮整个桌面应用；端口被占用即天然「单实例」互斥 |
| P1-6 api_key 明文回吐前端 | ✅ 已修（commit `b14976b`） | `ops/aimodels.rs` 新增 `public_config()`：对外视图将供应商 `apiKey` 恒脱敏为 `""` 并补 `keySet` 标记；`api/handlers_ai.rs` 的 `get_ai_config` / `saved_config` 改走脱敏视图，`save_provider` 加「编辑时 apiKey 留空 = 保留原值」防护；前端 `AiProviderDto` 加 `keySet?`、对话框按 `keySet` 提示。落盘仍明文（用户选「仅遮罩回传」，单机可接受）；若要彻底 at-rest 防护需进系统钥匙串（待批） |
| P2-a 端口笔误（registry 描述 8080↔48085） | ✅ 已修（commit `48b2d4e`） | `registry.rs:81` dfy-ui 描述「反代到后端 8080」实为 48085，改 48085（ports 字段本就正确，仅描述文本错误） |
| P2-c 契约 severity 漂移 | ✅ 已修（commit `48b2d4e`） | `types.ts` `Finding.severity` 去掉前端死分支 `info`（Rust `Severity` 只产 Warning/Critical），对齐契约；Dashboard 的 MUI Alert 兜底 `info` 保留 |
| P2-b MQTT 18830 双角色 | ✅ 已修（commit `5abe19d`） | 用户确认 18830 为**外部 VerneMQ**（edge-conductor 去连它）。`registry.rs` edge-conductor.ports 由 `[8084,8085,18830]` 改为 `[8084,8085]`，描述同步澄清；端口探活仍由 `environment.rs` 既有的「VerneMQ (MQTT) 127.0.0.1:18830 可达」外部依赖检查兜底。新增回归测试 `edge_conductor_ports_exclude_external_mqtt` |
| P2-d/e/f/g（AiAssistant 拆分 / 契约 codegen / is_daemon 保活 / logs 微优化） | ⏳ 待批 | 范围大、即时收益低，未动 |

**第二阶段验证（commit `6f4ec15`）**：`cargo check` 0 error ｜ `cargo clippy --all-targets` 仅 `health.rs:14`（redundant pattern matching）、`diagnosis.rs:125`（`vec!` 建议）两条**预存** warning，本次改动零新增告警 ｜ `cargo test` **14 passed / 0 failed** ｜ `npx tsc --noEmit` 0 error ｜ `vite build` 成功。第一阶段验证（14 passed 等）仍成立。`cargo fmt` 未全局执行——仓库 Rust 文件存在预存格式不一致（非本次改动区域），按「不碰范围外」原则保持工作树仅含本次 8 个目标文件。

---

## 一、总体结论

**这是一个完成度相当高的"小而准"的运维控制台**：分层干净、职责边界清晰、AI 配置的三层动态解析做得很到位（完全符合"禁止写死、禁止缓存"的铁律）。代码可读性和注释密度在同类项目里属于上游水平。

但它有一个**定位与实现之间的核心矛盾**：项目自我定位是"统一管理本地异构服务的**运维**平台"，而当前的进程模型是**"我只管我自己拉起来的进程"**。这导致控制台重启后所有已在运行的服务（nginx / java）全部显示"未运行"，运维平台最该有的"如实反映现状"能力缺失。

同时存在一组**阻塞调用进入 async 运行时**的问题，会在"全部启动"这类长操作时打满 tokio worker 池，让整个 API（含健康检查轮询）失去响应——这是最容易被真实用户踩中、且现象最像"卡死"的缺陷。

按严重度：**P0 两条、P1 六条、P2 七条**。

---

## 二、C4 Context：系统上下文

```
                    ┌──────────────────────────┐
                    │   运维人员（Windows 主机）  │
                    └────────────┬─────────────┘
                                 │ 桌面 GUI
                    ┌────────────▼─────────────┐
                    │      dfy-ops 控制台        │
                    │   Tauri 2 + React 18      │
                    └────────────┬─────────────┘
                                 │ REST over 127.0.0.1:18721
        ┌────────────────────────┼────────────────────────┐
        │            │           │            │           │
   ┌────▼───┐  ┌─────▼────┐ ┌────▼─────┐ ┌────▼────┐ ┌────▼─────┐
   │DFY后端 │  │DFY管理前端│ │Gateway   │ │Edge     │ │Biosphere │
   │Vert.x  │  │Vue3+nginx│ │Console   │ │Conductor│ │空间系统   │
   │:48085  │  │:80       │ │:8081     │ │:8084/85 │ │(Tauri)   │
   └────┬───┘  └──────────┘ └──────────┘ └────┬────┘ └──────────┘
        │                                       │
   ┌────▼────────┐                    ┌─────────▼──────────┐
   │MySQL / Redis│                    │GreptimeDB / VerneMQ│
   └─────────────┘                    └────────────────────┘
```

**边界说明**：项目明确排除全部远程运维（终端 / SSH / SFTP / 远程桌面），这是正确的克制——把范围锁在"本机进程 + 本机端口"，复杂度可控。

---

## 三、C4 Container：运行时容器

| 容器 | 技术 | 职责 | 通信 |
|---|---|---|---|
| React 控制台 | React 18 + TS + Vite + MUI + zustand | 6 个页面：总览 / 环境检查 / 拓扑 / AI 助手 / 设置 / 服务详情 | HTTP → 18721 |
| Axum 运维 API | Rust Axum 0.7（**内嵌于 Tauri 进程**） | 20+ 路由：服务 CRUD 动作、编排、诊断、环境检查、AI 配置 | 127.0.0.1:18721 |
| ops 领域层 | Rust 模块（8 个） | registry / process / orchestrator / health / logs / diagnosis / environment / ai | 同步调用 |
| 配置持久化 | JSON 文件 | `ai_models.json`、`settings.json` 落 config_dir | 文件读写 |

**关键架构决策：Axum 内嵌而非 sidecar。** 好处是零额外进程、零 IPC 开销、状态共享直接；代价是 Rust 侧的阻塞会直接影响 Tauri 主进程所在进程的资源，且 API 崩溃与 UI 崩溃耦合。对这个规模（5 个服务）是正确的取舍。

---

## 四、C4 Component：ops 领域层

```
ops/
 ├─ registry.rs    服务注册表（内置 5 个 + TOML 外部化覆盖）
 ├─ process.rs     ProcManager：shell 启停、构建
 ├─ orchestrator.rs 依赖拓扑排序（Kahn）、就绪门控（30s）、start-with-deps
 ├─ health.rs      TCP 探活 / 极简 HTTP 探活
 ├─ environment.rs 运行时依赖探测（runtime-only，不含构建工具链）
 ├─ diagnosis.rs   规则诊断引擎（两阶段：运行态 + 环境）
 ├─ logs.rs        大日志尾部读取（8KB 分块回读）
 ├─ ai.rs          LLM 调用（OpenAI 兼容 / Anthropic 双协议）
 └─ aimodels.rs    三层配置 Provider → Endpoint → Model，动态解析
```

**设计亮点（值得保留）**

1. **三层 AI 配置的动态解析**（`aimodels.rs:141` `resolve()`）——model → endpoint → provider 实时查、不缓存，换模型只改一层。完全符合"禁止写死"的约束。👍
2. **环境检查的边界克制**（`environment.rs:150-155`）——只探测"能否跑起来"的运行时依赖，不含任何构建工具链，且一律按原生程序探测（不依赖 Docker）。这个边界划得很清醒。
3. **构建日志的结束标记**（`process.rs:64-77`）——另起线程 wait 后追加 `==== 构建结束：exit code N ====`，让前端能可靠停止轮询。细节到位。
4. **诊断双阶段 + LLM 优雅降级**——未配置模型时自动退化为规则结果，不会因为 AI 不可用而让核心功能失效。
5. **日志分块回读**（`logs.rs:8`）——不再把大日志整体读进内存，且有 3 个单测覆盖边界。

---

## 五、问题清单（按严重度）

### P0-1 阻塞调用进入 async 运行时，可打满 tokio worker 池

**证据链**
- `api/handlers.rs:157` `start_all()` 是 `async fn`，函数体直接同步调用 `orchestrator::start_all()`
- `ops/orchestrator.rs:115` → `wait_ready()`（`orchestrator.rs:25-44`）内部是 `thread::sleep`，单服务最长 **30s**（`READY_TIMEOUT_SECS=30`）
- 5 个服务串行 ⇒ 最长 **150 秒**独占一个 worker 线程

tokio 默认 worker 线程数 = CPU 核数。一次"全部启动"就可能占掉全部 worker，期间 `/api/health`、`/api/services`（前端 5s 轮询）全部无响应——用户看到的现象就是**界面卡死**。

同类问题还有两处：
- `handlers.rs:93` `list_services` → `to_status` → `health::check_tcp`（每端口最长 800ms，5s 轮询高频触发）
- `handlers.rs:261` `env_check_all` → `environment::check_service_env` → `Command::output()` 阻塞式 spawn `java -version` / `nginx -v`

**修复方向**：所有同步阻塞段（探测、wait_ready、子进程探测）包进 `tokio::task::spawn_blocking`；或把编排/探测整体下沉到独立的 blocking 线程池，与 HTTP 请求处理线程池隔离。

---

### P0-2 `running` 状态失真：运维平台看不见"别人启动的服务"

**证据链**
- `api/handlers.rs:97` `to_status(d, state.procs.is_running(&d.id))`
- `ops/process.rs:20-22` `is_running` 只查内部 `HashMap<String, Child>`，**不做任何真实存活校验**

**后果（三重叠加）**
1. 控制台重启后，nginx / java 明明在跑，UI 全部显示"未运行"
2. `healthy` 走的是端口探测 → 出现**"健康但未运行"**这种自相矛盾的状态
3. `Topology.tsx:15` `statusTone()` 把 `!s.running` 直接判为 `down` ⇒ **拓扑图整片飘红**，即使服务全部正常

这是运维工具的致命伤：用户第一眼看的就是"现在到底跑着什么"，而当前答案只在"这个进程恰好是本平台拉起来的"时才正确。

**修复方向**：`running` 语义改为以**端口探测 / 进程表扫描**为准（内部 Child 句柄只用于 stop 和 PID 归属）；或引入 PID 文件 + 存活校验。建议把状态模型显式化为三态：`managed`（本平台拉起）/ `detected`（外部存在）/ `down`。

---

### P1-1 「全部启动」会把桌面客户端拉起来

`ops/orchestrator.rs:115` `start_all` 遍历**全部** defs，未按 `is_daemon` 过滤。而 `registry.rs:124` 的 `biosphere-spatial` 是 `is_daemon: false` 的 Tauri 桌面程序。函数注释却写着"启动全部**常驻**服务"。

实际效果：运维人员点"全部启动"，运维机上会弹出一个 GUI 桌面应用。

**修复**：`start_all` 增加 `.filter(|d| d.is_daemon)`。

---

### P1-2 本地 API 无鉴权 + CORS permissive ⇒ 可被任意网页驱动

`api/server.rs:124` `CorsLayer::permissive()`，全站无 token。

绑定 127.0.0.1 只防外部网络，**不防同机浏览器**：用户访问任意一个恶意页面，即可 `fetch("http://127.0.0.1:18721/api/services/{id}/build", {method:"POST"})` —— 而 `run_cmd` / `build_cmd` 由 `DFY_OPS_SERVICES` 指向的 TOML 提供，等价于**任意命令执行**；或者直接 `stop` 掉生产服务。

**修复**：
1. CORS 限定 `tauri://localhost` / `http://localhost:18720`，去掉 permissive
2. 启动时生成一次性 token 写入 `config_dir`，前端经 Tauri 命令获取后带在 header 上
3. `build` / `stop-all` 这类高危动作加二次确认（前端已有 stop-all 确认框，build 没有）

---

### P1-3 停止不杀进程树（非 Windows 平台）

`ops/process.rs:40-56`：`stop()` 只在 Windows 走 `taskkill /T /F`，其他平台仅 `child.kill()`。而进程是经 `sh -c cmd` 启动的（`process.rs:96`），杀掉的只是 shell，**java / nginx 会变成孤儿继续存活**。

macOS / Linux 开发环境直接踩中（当前开发机就是 macOS）。

**修复**：Unix 侧用 `setsid` 建进程组后 `kill(-pgid)`，或遍历杀子进程树。

---

### P1-4 僵尸进程与句柄不回收，崩溃退出无感知

`ops/process.rs:26-34`：`start()` 后从不调用 `wait()` / `try_wait()`。子进程退出后仍留在 `children` map 中 ——
- Unix：僵尸进程累积
- Windows：句柄泄漏
- 服务崩溃退出不会被感知，只能靠端口探测间接推断

**修复**：spawn 一个 reaper 线程定期 `try_wait` 清理，并同步更新存活状态（与 P0-2 一并解决）。

---

### P1-5 端口占用即静默死亡，且无单实例保护

`lib.rs:68-70` `.expect("failed to bind ops API on 127.0.0.1:18721")`。

端口被占（二次启动 / 残留进程）时，该 spawn 出的线程 panic，**Tauri 主进程照常启动**。前端所有请求失败，界面只在 `Dashboard.tsx:254` 显示一句"（请确认运维后端已在 127.0.0.1:18721 运行）"——用户拿不到真实根因。

**修复**：bind 失败时优雅降级 + 弹出明确提示；加 `tauri-plugin-single-instance` 做单实例保护。

---

### P1-6 API Key 明文落盘，且原样回吐给前端

`ops/aimodels.rs:76-82`：`ai_models.json` 明文存 `api_key`，config_dir 权限未收紧。
`api/handlers_ai.rs:13`：`get_ai_config` 直接把完整配置（含 key）返回给 UI。

**修复**：改用 OS keychain（`keyring` crate）；至少收紧文件权限为 600；列表接口返回时对 key 脱敏（只在编辑时按需取原文）。

---

### P2 清单

| # | 问题 | 位置 |
|---|---|---|
| P2-1 | 前后端契约手工双写，无 codegen / 契约测试，改字段易漏 | `src/types.ts` (121行) ↔ Rust structs |
| P2-2 | `AiAssistant.tsx` 1020 行 / 26 个 useState；颜色常量硬编码（`PRIMARY_COLOR` 等）未走 MUI theme（`theme.ts` 仅 15 行） | `src/pages/AiAssistant.tsx:104-112` |
| P2-3 | 契约不一致：TS 侧 `Finding.severity` 含 `"info"`，Rust `Severity` 只有 `warning/critical` ⇒ `Dashboard.tsx:348` 的 info 分支永不执行 | `types.ts:44` / `diagnosis.rs:10` |
| P2-4 | 文档自相矛盾：`registry.rs:81` 写"反代 /api 到后端 **8080**"，实际后端端口 **48085** | `ops/registry.rs:81` |
| P2-5 | 端口语义冲突：README 说 Edge Conductor 监听 MQTT **18830**，而 `environment.rs:233` 把 18830 当**外部 VerneMQ 依赖**探测 ⇒ 同一端口既自己监听又是依赖，二者必有一错 | `README.md:12` / `environment.rs:233` |
| P2-6 | `is_daemon` 字段注释称"决定保活策略"，实际无任何保活逻辑，仅在 diagnosis 中跳过检查 | `registry.rs:4-5` |
| P2-7 | 日志 tail 每个 8KB chunk 都全量 `filter(count('\n'))`，大日志下退化为 O(n²/8192)；可改为增量计数 | `ops/logs.rs:43` |

---

## 六、技术选型决策矩阵

| 维度 | 当前选型 | 备选 | 评分(1-5) | 结论 |
|---|---|---|---|---|
| 桌面外壳 | **Tauri 2** | Electron | 体积 5 / 内存 5 / 生态 3 | ✅ 正确。运维工具无 Node 生态依赖，包体从 ~100MB 降到 ~10MB 级 |
| 后端形态 | **Axum 内嵌进程** | 独立 sidecar 二进制 | 简洁 5 / 隔离性 2 | ✅ 规模适配。但需配合 P0-1 的线程池隔离修复隔离性短板 |
| 服务注册 | **内置 + TOML 外部化** | 纯代码硬编码 / 数据库 | 灵活 4 / 安全 3 | ✅ 合理。但 TOML 可提供任意命令（配合 P1-2 需加来源校验） |
| 状态管理 | **zustand** | Redux Toolkit | 简洁 5 / 规范性 3 | ✅ 规模适配。若页面继续膨胀需补 selector 规范 |
| UI 库 | **MUI v5** | Ant Design | 组件 5 / 体积 3 | ✅ 合理。建议把硬编码色值收敛进 theme |
| 配置持久化 | **JSON 文件** | SQLite / keychain | 简单 5 / 安全 2 | ⚠️ 结构配置 OK，**密钥必须移出**（P1-6） |

---

## 七、DevOps 与交付

**现状**：GitHub Actions `windows-build.yml`，`windows-latest`，push main / PR / tag `v*` / 手动触发。

**做得好的**：Rust 依赖缓存（Swatinem/rust-cache）、tag 自动发 Release、Artifact 上传。

**缺口**
1. **CI 未跑质量门禁**：`cargo test`、`cargo clippy` 都没进流水线。当前 5 个单测和 1 个 clippy warning 全靠手工跑。
2. **`pnpm install --frozen-lockfile=false`**（`windows-build.yml:44`）：绕过了 lockfile 校验，削弱可复现性。建议补 lockfile 后改回 `--frozen-lockfile`。
3. **单一平台验证**：只有 Windows 构建。开发机是 macOS，意味着 macOS 侧的编译问题（如 P1-3 的 Unix 进程树处理）在 CI 上完全无覆盖。
4. **版本号三处手工同步**：`Cargo.toml` / `package.json` / `tauri.conf.json` 均为 0.1.0，无自动化校验，发版易漏改。
5. **无 Release Note / CHANGELOG**。

**建议的流水线补强**

```
lint ──► tsc --noEmit ─┐
     └─► cargo clippy ─┤
test ──► cargo test ───┼──► build (matrix: windows + macos) ──► artifact ──► release(on tag)
     └─► vitest?  ─────┘
```

---

## 八、技术债务治理路线图

### 第一阶段：止血（1-2 天，建议立刻做）
| 项 | 动作 | 风险 |
|---|---|---|
| P0-1 | 全部同步阻塞段包 `spawn_blocking`；编排与探测下沉独立 blocking 池 | 低，改动局部 |
| P1-1 | `start_all` 加 `is_daemon` 过滤 | 极低，一行 |
| P1-2 | CORS 收紧到具体 origin + 启动 token | 中，需前后端配套 |

### 第二阶段：补核心能力（3-5 天）
| 项 | 动作 | 风险 |
|---|---|---|
| P0-2 | 状态模型改三态 `managed/detected/down`，running 以端口+进程探测为准 | **高**，触及状态语义，需前后端同步改 + 回归 |
| P1-3 | Unix 进程组 kill | 中，需真机验证 |
| P1-4 | reaper 线程 + 存活同步 | 中，与 P0-2 合并做 |
| P1-5 | bind 失败优雅降级 + 单实例锁 | 低 |

### 第三阶段：加固与减债（1-2 周）
- P1-6 密钥迁移 keychain
- P2-1 引入契约层：`ts-rs` 从 Rust 类型生成 `types.ts`，消灭双写
- P2-2 拆分 `AiAssistant.tsx`（Provider / Endpoint / Model 三个对话框各自成文件），色值收敛进 theme
- P2-4 / P2-5 修正文档与端口语义冲突（**先确认 18830 到底谁监听**，这是产品语义问题不是技术问题）
- CI 补 `cargo test` + `clippy`，加 macOS 构建矩阵
- 补契约测试：Rust 序列化快照 ↔ TS 类型校验

---

## 九、一句话建议

> 先把 **P0-1（阻塞线程池隔离）** 和 **P0-2（状态真实性）** 解决掉。前者是"用户会以为它死了"的体验问题，后者是"运维平台看不见真实世界"的定位问题——这两条修完，这个项目就从"能用的工具"变成"敢用的工具"。P1 里的 CORS 鉴权（P1-2）虽然排在第三，但从安全角度建议和第一阶段一起做。
