# DFY运维平台 (dfy-ops)

统一管理 DFY 系列**本地异构服务**的智能运维控制台。**纯本地运维，不含任何远程运维（终端 / SSH / SFTP / 远程桌面）功能。**

## 运维目标

| 服务 | 技术栈 | 端口 | 说明 |
|---|---|---|---|
| DFY 后端 | Java Vert.x | 48085 | 胖 jar，`java -jar` |
| DFY 管理前端 | Vue3 (nginx) | 80 | 反代 `/api` → 后端 |
| Gateway Console | Angular 19 | 8081 | 静态托管 |
| Edge Conductor | Rust Axum | 8084/8085/MQTT 18830 | upstream → 后端 48085 |
| 空间数据驱动程序 | Tauri 桌面 | — | 桌面客户端，非常驻；镜像名判「跑没跑」、卸载注册表判「装没装」 |

## 技术栈

- 前端：React 18 + TypeScript + Vite + MUI + zustand
- 后端：Rust **Axum**，内嵌于 Tauri 进程，监听 `127.0.0.1:18721` 提供运维 REST API
- 外壳：Tauri 2（Windows 原生 `.msi` / `.exe`）
- 工程骨架参考 `web-craft`，但已移除全部远程运维模块

## 架构

```
Tauri 桌面应用
 ├─ React 控制台 (前端)
 └─ Rust 核心
     ├─ ops/registry      服务注册（5 个目标服务定义）
     ├─ ops/process       生命周期管理（启停/构建）
     ├─ ops/orchestrator  依赖拓扑排序，按序启停（start-all / stop-all）
     ├─ ops/health        端口 / TCP / HTTP 健康探针
     ├─ ops/logs          日志聚合（尾部读取）
     ├─ ops/diagnosis     规则诊断引擎（兜底）
     ├─ ops/ai            LLM 增强层（根因分析，配置后启用）
     └─ api (Axum)        REST：/api/services, start/stop/restart/build/logs/diagnosis, start-all/stop-all
```

## 本地开发

```bash
pnpm install
pnpm tauri dev      # 同时启动前端 + Rust 后端（含 Axum API）
```

前端默认 `http://localhost:18720`，运维 API `http://127.0.0.1:18721`。

## Windows 打包

GitHub Actions（`.github/workflows/windows-build.yml`）在 `windows-latest` 上：

1. 安装 Rust（目标 `x86_64-pc-windows-msvc`）+ Node + pnpm
2. `pnpm install` → `pnpm build`（产出 `dist`）
3. `pnpm tauri build` → 生成 `msi` 与 `nsis` 安装包
4. 上传为 Artifact；打 `v*` tag 时自动发布 Release

> 服务路径默认按 Windows 约定（`C:/services/...`），实际部署请在 `src-tauri/src/ops/registry.rs` 中按需调整，或改用外部配置（见下）。

## 服务注册表配置（外部化）

默认内置 5 个服务的注册定义。如需不改代码即调整路径/端口/依赖，可提供一个 TOML 并设环境变量：

```bash
DFY_OPS_SERVICES=C:/services/services.toml pnpm tauri dev
```

参考 `services.example.toml`：每个服务含 `id / name / kind / installDir / buildCmd / runCmd / ports / logPaths / dependsOn / isDaemon / description`。解析失败自动回退内置默认。

## 智能运维

- **依赖编排**：启动/停止按依赖拓扑排序（如 Edge Conductor 必在其依赖的 DFY 后端之后启动）；提供「全部启动 / 全部停止」。
- **就绪门控**：启动某服务后，运维端会持续探测其端口直至全部可连（超时 30s）再启动依赖方，避免「依赖未真正就绪就拉起上游」导致的 upstream 连不上问题。
- **规则兜底**：端口占用、进程退出、依赖未启动、启动顺序校验。
- **LLM 增强（双引擎）**：配置以下环境变量后，`/api/diagnosis` 会调用大模型输出根因分析与处置建议；未配置时自动退化为规则结果。
  - `DFY_OPS_LLM_ENDPOINT`：OpenAI 兼容的 `chat/completions` 地址
  - `DFY_OPS_LLM_KEY`：鉴权 token
  - `DFY_OPS_LLM_MODEL`：模型名（默认 `gpt-4o-mini`）
