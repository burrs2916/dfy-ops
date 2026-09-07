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

/**
 * 客户端标识头：后端 api/server.rs 的 require_client_header 中间件会校验它。
 *
 * 这是本机运维 API 的防跨站驱动措施——若不加，用户浏览的任意第三方页面都能
 * fetch 到 127.0.0.1:18721 去启停服务、触发构建。带上这个非安全头会强制浏览器
 * 发 OPTIONS 预检，预检由后端的来源白名单裁决，非法来源在预检阶段就被拦掉。
 *
 * 代价：每个 GET 也多一次预检往返（原先刻意省掉的）。本机回环下开销可忽略，
 * 换来的是运维接口不被任意网页驱动，这笔交换是值得的。
 */
const CLIENT_HEADER: Record<string, string> = {
  "X-Dfy-Ops-Client": "dfy-ops-web",
};

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  // 仅当有请求体时才设置 Content-Type：GET 带该头没必要，多一个非安全头徒增预检负担。
  const headers: Record<string, string> = { ...CLIENT_HEADER };
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
      desktopInReadiness?: boolean;
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
