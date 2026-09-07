export type ServiceKind =
  | "vertx"
  | "vue_spa"
  | "angular_spa"
  | "rust_bin"
  | "tauri_app"
  | "mqtt_broker"
  | "object_storage";

/** 运行态三态（P0-2）：managed=本平台受管运行 / detected=外部进程在跑 / down=未运行 */
export type RunState = "managed" | "detected" | "down";

export interface ServiceStatus {
  id: string;
  name: string;
  kind: ServiceKind;
  running: boolean;
  runState: RunState;
  healthy: boolean;
  ports: number[];
  dependsOn: string[];
  isDaemon: boolean;
  /** 第三方依赖（VerneMQ/MinIO 等系统级中间件）：平台只检测不托管，UI 隐藏启停按钮并分组展示 */
  thirdParty: boolean;
  description: string;
  installDir: string;
  buildCmd?: string | null;
  runCmd: string;
  healthUrl?: string | null;
  logPaths: string[];
}

export interface DiagnosisResponse {
  status: Record<string, [boolean, boolean]>;
  envs: ServiceEnvReport[];
  findings: Finding[];
  llmSummary?: string | null;
  llmNote?: string | null;
}

export interface ActionResult {
  id: string;
  ok: boolean;
  error?: string | null;
}

export interface Finding {
  service: string;
  /** 与 Rust `Severity` 枚举对齐（仅 Warning/Critical；Rust 端不产 Info） */
  severity: "warning" | "critical";
  message: string;
  fixHint?: string | null;
}

export interface LogsResponse {
  id: string;
  logs: string;
}

export type CheckStatus = "pass" | "warn" | "fail";

export interface EnvCheckItem {
  /** 稳定 id（修复建议/前端键控），不随显示名变化 */
  id: string;
  name: string;
  category: string;
  status: CheckStatus;
  expected: string;
  actual: string;
  isFatal: boolean;
  fixHint: string;
}

export interface ServiceEnvReport {
  id: string;
  name: string;
  overall: CheckStatus;
  items: EnvCheckItem[];
}

/** ===== 环境修复（java 检查项自动安装等） ===== */

/** 修复任务状态（GET /api/env/repair/status，前端轮询） */
export interface EnvRepairJob {
  running: boolean;
  /** download → install → verify → done */
  stage: string;
  logs: string[];
  ok: boolean | null;
  error: string | null;
}

/** POST /api/env/repair 的分级决策结果 */
export interface EnvRepairStart {
  ok: boolean;
  /** none=无需修复 / configure=本机已有达标 JDK 给配置指引 / install=启动自动安装 */
  action?: "none" | "configure" | "install";
  message?: string;
  home?: string;
  error?: string;
}

/** ===== AI 模型配置（三层：供应商→接入点→模型，web-craft ModelConfigPage 同构） ===== */

export interface AiProviderDto {
  id: string;
  name: string;
  apiKey: string;
  /** 后端脱敏后返回：true=已配置 Key（apiKey 字段恒为 ""，保存时留空表示保留原 Key） */
  keySet?: boolean;
  enabled: boolean;
}

export interface AiEndpointDto {
  id: string;
  providerId: string;
  name: string;
  /** openai-completions | anthropic-messages | ollama */
  apiType: string;
  baseUrl: string;
  enabled: boolean;
}

export interface AiModelDto {
  id: string;
  endpointId: string;
  name: string;
  /** 调用时传给 API 的模型标识 */
  refKey: string;
  enabled: boolean;
}

export interface AiModelConfigDto {
  providers: AiProviderDto[];
  endpoints: AiEndpointDto[];
  models: AiModelDto[];
  activeModelId: string | null;
}

export interface AiConfigResult {
  ok: boolean;
  config?: AiModelConfigDto;
  error?: string;
}

export interface AiTestResult {
  ok: boolean;
  message?: string;
  error?: string;
}

/** 应用自身设置（「设置」页；与 AI 助手配置分离）。 */
export interface AppSettings {
  statusRefreshSecs: number;
  envRefreshSecs: number;
}
