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
  EnvCheckItem,
  EnvRepairJob,
  EnvRepairStart,
  GenericRepairStart,
  LogsResponse,
  RepairExecResult,
  RepairPlan,
  ResourceScan,
  ServiceEnvReport,
  ServiceStatus,
  GenericUninstallStart,
  UninstallTargetStatus,
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
  // force：跳过后端 60s 缓存强制重探——手动刷新/轮询都要新数据，
  // 否则刚修完环境点「重新检查」会看到缓存里的旧结论
  envCheckAll: (force = false) =>
    request<ServiceEnvReport[]>(`/api/env${force ? "?force=true" : ""}`),
  // 单服务组合刷新：后端一次进程快照同时返回状态与环境（启停后局部刷新用，
  // 原先 status+env 两个请求各采一份快照，Windows 上等待时间翻倍）
  refreshService: (id: string) =>
    request<{ status: ServiceStatus; env: ServiceEnvReport }>(
      `/api/services/${id}/refresh`,
    ),
  // 环境修复（java 检查项分级修复：none/configure/install）
  startEnvRepair: (serviceId: string, checkId: string) =>
    request<EnvRepairStart>("/api/env/repair", {
      method: "POST",
      body: JSON.stringify({ serviceId, checkId }),
    }),
  envRepairStatus: () => request<EnvRepairJob>("/api/env/repair/status"),
  // ===== 环境修复中心（本地资源目录 → 在线 → AI 方案 → 白名单代执行） =====
  // dir：设置页对未保存草稿目录的即时预览；缺省扫已配置的资源目录
  resources: (dir?: string) =>
    request<ResourceScan>(`/api/env/resources${dir ? `?dir=${encodeURIComponent(dir)}` : ""}`),
  startGenericRepair: (target: string) =>
    request<GenericRepairStart>("/api/env/repair/generic", {
      method: "POST",
      body: JSON.stringify({ target }),
    }),
  // previousPlan + followUp：多轮追问时携带上一轮方案与用户追问
  repairPlan: (checkItems: EnvCheckItem[], previousPlan?: string, followUp?: string) =>
    request<RepairPlan>("/api/env/repair/plan", {
      method: "POST",
      body: JSON.stringify({ checkItems, previousPlan, followUp }),
    }),
  repairExec: (command: string) =>
    request<RepairExecResult>("/api/env/repair/exec", {
      method: "POST",
      body: JSON.stringify({ command }),
    }),
  // 卸载 / 维护（当前覆盖 MySQL/Redis）
  uninstallStatus: () =>
    request<{ targets: UninstallTargetStatus[] }>("/api/env/repair/uninstall/status"),
  startUninstall: (target: string, removeData: boolean) =>
    request<GenericUninstallStart>("/api/env/repair/uninstall", {
      method: "POST",
      body: JSON.stringify({ target, removeData }),
    }),
  // 数据库运维：服务启停（net start/stop，UAC 确认）与 SQL 导入（stdin 直灌）
  serviceAction: (target: string, action: "start" | "stop") =>
    request<{ ok: boolean; running?: boolean; message?: string; error?: string }>(
      "/api/env/repair/service",
      {
        method: "POST",
        body: JSON.stringify({ target, action }),
      },
    ),
  mysqlImport: (file: string) =>
    request<{ ok: boolean; message?: string; error?: string }>("/api/env/repair/mysql/import", {
      method: "POST",
      body: JSON.stringify({ file }),
    }),
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
      resourceDir?: string;
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
