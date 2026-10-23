# dfy-ops 会话成果总览（2026-09-05）

## 本轮完成

### 1. 环境检查重设计落地（commit `91337b6`）
- 三段式检查模型：kind 运行时检查 → `requires` 声明检查（path/port）→ `dependsOn` 依赖实时状态检查（未运行=Fail、运行不健康=Warn）
- `CheckItem` 新增稳定 `id`；fixHint 按平台生成（win=winget / mac=brew）；前端 ack 键控改用 item.id
- 内置 mysql(3306)/redis(6379) 注册为第三方检测服务；`env_check_all` 加 60s TTL 缓存
- 实测：dep-mysql/dep-redis/greptime 检查项全 pass，TTL 缓存命中 ~7ms
- 坑：本机覆盖 `~/Library/Application Support/com.dfy.ops/services.toml` 会整体替换内置定义，已同步更新

### 2. P2-d AiAssistant.tsx 拆分（commit `5a1a55d`）
- 1020 行 → 408 行主文件 + `pages/ai/` 5 个子模块（shared 65 / dialogs 325 / ModelRow 99 / EndpointCard 180 / ProviderCard 182）
- 纯机械抽取，行为 1:1；tsc + vite build 通过

## 状态
- 评审清单 P2 全部清零；`cargo test` 39 全过、clippy 0 warning
- 待办：用户网络通时 `git push origin main`（17+ 提交）；CI 首跑结果确认

## 2026-09-07：桌面客户端计入就绪度（方案 B+C）

背景：实测 dfy-ui 与 biosphere-spatial 都是 down，但只有后者不被统计 —— 分流点在 ReadinessCard.tsx:23 与 diagnosis.rs:47。

改动：
- Rust：`appsettings.rs` 加 `desktop_in_readiness`（默认 false）；`handlers.rs` 接收并回吐；`contract_sync.rs` 契约快照补键并钉住默认值
- 前端：types/store/api 同步；`Settings.tsx` 通用页加 Switch；`Dashboard.tsx` 传 `desktopInReadiness`
- `ReadinessCard.tsx` 重写：开关决定分母口径；桌面客户端抽出常驻独立区块（修复原先挂在 notReady 分支内、常驻全就绪时整块隐形的缺陷），带彩色圆点显示在线状态；移除右上角重复 Chip

未改（刻意）：`diagnosis.rs:47` 仍不为桌面客户端产诊断发现项 —— 开关只改展示口径，不动诊断语义。

验证：`cargo test --lib` 36 passed / 0 failed；`tsc --noEmit` 干净；`vite build` 成功。Rust 侧需重启 Tauri dev 进程后才生效。
