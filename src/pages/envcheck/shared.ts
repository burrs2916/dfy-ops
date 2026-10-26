import type { CheckStatus } from "../../types";

/** 状态→颜色（MUI severity 语义） */
export const STATUS_COLOR: Record<CheckStatus, "success" | "warning" | "error"> = {
  pass: "success",
  warn: "warning",
  fail: "error",
};

/** 状态→中文标签 */
export const STATUS_LABEL: Record<CheckStatus, string> = {
  pass: "满足",
  warn: "警告",
  fail: "缺失",
};

/** 状态→十六进制色（用于非 MUI 组件，如 Dot 原始 CSS） */
export const STATUS_HEX: Record<CheckStatus, string> = {
  pass: "#1e3f66",
  warn: "#BA7517",
  fail: "#A32D2D",
};

/** ack 状态持久化（localStorage key） */
export const ACK_KEY = "dfy-ops:ack";
export function loadAck(): Set<string> {
  try {
    const raw = localStorage.getItem(ACK_KEY);
    return new Set(raw ? (JSON.parse(raw) as string[]) : []);
  } catch {
    return new Set();
  }
}
export function saveAck(set: Set<string>) {
  try {
    localStorage.setItem(ACK_KEY, JSON.stringify([...set]));
  } catch {
    /* ignore */
  }
}
export function ackId(reportId: string, itemId: string) {
  return `${reportId}::${itemId}`;
}

/** 与后端 ops/resource.rs 的 repair_target 口径一致：dep- 与 service- 前缀折叠后匹配。
 *  仅本地可装的软件：Redis/VerneMQ/MinIO/GreptimeDB 均为远程部署，问题项走「去远程部署」配置。 */
export const REPAIR_TARGETS = new Set(["java", "mysql"]);
/** 纯远程部署的软件：不进待安装/已安装列表（本机不安装、无安装包可识别），
 *  集中在「远程部署」区配置地址与访问密码。与后端 ServiceDef.remoteOnly 同口径。 */
export const FIXED_REMOTE = new Set(["redis", "vernemq", "minio", "greptime"]);
/** 有平台托管端点的远程服务（「远程部署」面板可配地址/密码）——与面板 SERVICES 同口径。 */
export const MANAGED_ENDPOINT_TARGETS = new Set(["redis", "vernemq", "minio", "greptime"]);
export function repairTarget(checkId: string): string | null {
  const c = checkId.replace(/^dep-/, "").replace(/^service-/, "");
  return REPAIR_TARGETS.has(c) ? c : null;
}
/** 折叠后命中的检查项 → 引导去「远程部署」配置页（地址/密码都在那里改）。 */
export function configTarget(checkId: string): string | null {
  const c = checkId.replace(/^dep-/, "").replace(/^service-/, "");
  return FIXED_REMOTE.has(c) ? c : null;
}

/** 软件中文名 */
export const TARGET_LABEL: Record<string, string> = {
  java: "Java 运行时",
  mysql: "MySQL",
  redis: "Redis",
  vernemq: "VerneMQ",
  minio: "MinIO",
  greptime: "GreptimeDB",
};

/** 安装与维护卡片的展示顺序：用户重点的数据库/运行时在前 */
export const SOFTWARE_ORDER = ["mysql", "java", "redis", "vernemq", "minio", "greptime"];
/** 注册了 Windows 服务、可启停的软件（与后端 service_spec 口径一致；java 无服务） */
export const SERVICE_TARGETS = new Set(["mysql"]);
/** 一句话说清每个软件是干嘛的（卡片副标题，消灭抽象概念） */
export const TARGET_DESC: Record<string, string> = {
  mysql: "数据库",
  java: "Java 运行时",
  redis: "缓存数据库",
  vernemq: "MQTT 消息队列",
  minio: "对象存储",
  greptime: "时序数据库",
};
/** 约定子目录名（与后端 convention_target 对齐）：无包时引导用户放到哪 */
export const CONVENTION_DIR: Record<string, string> = {
  java: "JDK",
  mysql: "MySQL",
  redis: "Redis",
  vernemq: "VerneMQ",
  minio: "MinIO",
  greptime: "GreptimeDB",
};

/** 包体积 → 人读文案（KB/MB/GB，1 位小数）。 */
export function fmtSize(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

/** 修复任务阶段 → 用户可读文案。 */
export const REPAIR_STAGE_LABEL: Record<string, string> = {
  download: "下载安装包…",
  install: "静默安装中（请在 UAC 弹窗确认管理员权限）…",
  extract: "解压安装包…",
  configure: "写配置并初始化…",
  backup: "备份 MySQL 业务数据（非系统库导出为 SQL 文件，删数据前自动执行）…",
  uninstall: "卸载中（停服 → 清理残留进程 → 注销服务）…",
  verify: "验证结果…",
  done: "已完成",
};
