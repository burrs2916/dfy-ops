export type ServiceKind =
  | "vertx"
  | "vue_spa"
  | "angular_spa"
  | "rust_bin"
  | "tauri_app";

export interface ServiceStatus {
  id: string;
  name: string;
  kind: ServiceKind;
  running: boolean;
  healthy: boolean;
  ports: number[];
  dependsOn: string[];
  isDaemon: boolean;
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
  severity: "info" | "warning" | "critical";
  message: string;
  fixHint?: string | null;
}

export interface LogsResponse {
  id: string;
  logs: string;
}

export type CheckStatus = "pass" | "warn" | "fail";

export interface EnvCheckItem {
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

/** ===== AI 模型配置（三层：供应商→接入点→模型，web-craft ModelConfigPage 同构） ===== */

export interface AiProviderDto {
  id: string;
  name: string;
  apiKey: string;
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
