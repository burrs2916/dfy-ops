import type {
  ActionResult,
  AiConfigResult,
  AiEndpointDto,
  AiModelConfigDto,
  AiModelDto,
  AiProviderDto,
  AiTestResult,
  AppSettings,
  DiagnosisResponse,
  LogsResponse,
  ServiceEnvReport,
  ServiceStatus,
} from "./types";

// 运维后端（内嵌 Axum）监听本地 18721。
const BASE = "http://127.0.0.1:18721";

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  // 仅当有请求体时才设置 Content-Type：GET 带该头会触发 CORS 预检（非安全头），
  // 造成每个请求都多一次 OPTIONS 往返（后端虽已支持预检，能省则省）。
  const headers: Record<string, string> = {};
  if (init?.body) {
    headers["Content-Type"] = "application/json";
  }
  const res = await fetch(`${BASE}${path}`, {
    ...init,
    headers: { ...headers, ...(init?.headers as Record<string, string>) },
  });
  if (!res.ok) {
    throw new Error(`请求失败 ${res.status}: ${path}`);
  }
  return res.json() as Promise<T>;
}

export const api = {
  health: () => request<{ status: string; services: number }>("/api/health"),
  listServices: () => request<ServiceStatus[]>("/api/services"),
  getService: (id: string) =>
    request<ServiceStatus>(`/api/services/${id}`),
  start: (id: string) =>
    request<{ ok: boolean; error?: string }>(`/api/services/${id}/start`, {
      method: "POST",
    }),
  stop: (id: string) =>
    request<{ ok: boolean; error?: string }>(`/api/services/${id}/stop`, {
      method: "POST",
    }),
  restart: (id: string) =>
    request<{ ok: boolean; error?: string }>(`/api/services/${id}/restart`, {
      method: "POST",
    }),
  build: (id: string) =>
    request<{ ok: boolean; error?: string }>(`/api/services/${id}/build`, {
      method: "POST",
    }),
  buildLog: (id: string) =>
    request<LogsResponse>(`/api/services/${id}/build-log`),
  logs: (id: string) =>
    request<LogsResponse>(`/api/services/${id}/logs`),
  diagnosis: () => request<DiagnosisResponse>("/api/diagnosis"),
  envCheck: (id: string) =>
    request<ServiceEnvReport>(`/api/services/${id}/env`),
  envCheckAll: () => request<ServiceEnvReport[]>("/api/env"),
  startAll: () =>
    request<ActionResult[]>("/api/services/start-all", { method: "POST" }),
  stopAll: () =>
    request<ActionResult[]>("/api/services/stop-all", { method: "POST" }),
  appSettings: () => request<AppSettings>("/api/settings/app"),
  saveAppSettings: (body: AppSettings) =>
    request<{
      ok: boolean;
      statusRefreshSecs?: number;
      envRefreshSecs?: number;
      error?: string;
    }>("/api/settings/app", {
      method: "POST",
      body: JSON.stringify(body),
    }),
  // AI 助手域（三层：供应商→接入点→模型；与应用「设置」分离）
  // 后端保存/删除/切换均返回最新完整配置（ok + config），失败时 ok=false + error。
  aiConfig: () => request<AiModelConfigDto>("/api/ai/config"),
  saveAiProvider: (body: AiProviderDto) =>
    expectConfig(
      request<AiConfigResult>("/api/ai/providers", {
        method: "POST",
        body: JSON.stringify(body),
      }),
    ),
  deleteAiProvider: (id: string) =>
    expectConfig(
      request<AiConfigResult>(`/api/ai/providers/${id}`, {
        method: "DELETE",
      }),
    ),
  saveAiEndpoint: (body: AiEndpointDto) =>
    expectConfig(
      request<AiConfigResult>("/api/ai/endpoints", {
        method: "POST",
        body: JSON.stringify(body),
      }),
    ),
  deleteAiEndpoint: (id: string) =>
    expectConfig(
      request<AiConfigResult>(`/api/ai/endpoints/${id}`, {
        method: "DELETE",
      }),
    ),
  testAiEndpoint: (id: string) =>
    request<AiTestResult>(`/api/ai/endpoints/${id}/test`, { method: "POST" }),
  saveAiModel: (body: AiModelDto) =>
    expectConfig(
      request<AiConfigResult>("/api/ai/models", {
        method: "POST",
        body: JSON.stringify(body),
      }),
    ),
  deleteAiModel: (id: string) =>
    expectConfig(
      request<AiConfigResult>(`/api/ai/models/${id}`, {
        method: "DELETE",
      }),
    ),
  testAiModel: (id: string) =>
    request<AiTestResult>(`/api/ai/models/${id}/test`, { method: "POST" }),
  setActiveAiModel: (modelId: string) =>
    expectConfig(
      request<AiConfigResult>("/api/ai/active-model", {
        method: "POST",
        body: JSON.stringify({ modelId }),
      }),
    ),
};

/** 保存类接口的统一解包：失败直接抛错，成功返回最新配置。 */
async function expectConfig(pending: Promise<AiConfigResult>): Promise<AiModelConfigDto> {
  const res = await pending;
  if (!res.ok || !res.config) {
    throw new Error(res.error || "保存失败");
  }
  return res.config;
}
