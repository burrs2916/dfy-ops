/**
 * 服务访问方式：启动完成后告诉用户「端口是哪个、怎么访问」。
 *
 * 内置服务按 id 给准确地址（与 src-tauri/src/ops/registry.rs 的 builtin 定义对应）；
 * services.toml 自定义服务按 kind 兜底推导；两边都没有则返回 null（不硬编造地址）。
 */

interface Known {
  /** 浏览器地址（http 服务才有） */
  url?: string;
  /** 访问方式描述 */
  how: string;
  /** 无 ServiceStatus 上下文时的默认端口（修复中心等只拿得到 id 的调用方） */
  ports?: number[];
  /** url 之后补充的附加访问信息（如随服务捆绑启动的其他控制台） */
  extra?: string;
}

const KNOWN: Record<string, Known> = {
  "gateway-console": { url: "http://localhost:4200/", how: "边缘网关控制台", ports: [4200] },
  "dfy-ui": { url: "http://localhost/", how: "云端管理控制台", ports: [80] },
  nginx: { url: "http://localhost/", how: "前端站点入口", ports: [80] },
  "edge-conductor": {
    // 后端 API（8084/8085）：不进 BROWSER_PAGES，不对用户暴露
    url: "http://localhost:8084/",
    how: "边缘计算 API",
    ports: [8084, 8085],
  },
  // GreptimeDB 控制台必须带完整路径 /dashboard：4000 根路径是 API 根，打开是空白页
  greptime: { url: "http://localhost:4000/dashboard", how: "GreptimeDB 控制台", ports: [4000] },
  "dfy-backend": { url: "http://localhost:48085/", how: "后端接口服务", ports: [48085] },
  mysql: { how: "MySQL 客户端连接 127.0.0.1:3306", ports: [3306] },
  "dfy-spatial": { how: "桌面应用：从桌面 / 开始菜单图标打开" },
};

/** 服务的浏览器地址（有才有）；批量启动时用来汇总「可访问：…」。 */
export function accessUrl(id: string): string | null {
  return KNOWN[id]?.url ?? null;
}

/** 卡片常驻展示用的地址条目：url=浏览器页面（点击打开）；text=非网页地址（点击复制）。 */
export interface AccessLink {
  url?: string;
  text?: string;
  label: string;
}

/** 只暴露用户要打开的控制台/页面；纯后端 API 端口（边缘 8084、后端 48085、
 *  S3 9000 等）不对外展示——它们是程序间调用地址，不是给人看的页面。
 *  GreptimeDB 控制台必须带完整路径 /dashboard（4000 根路径是 API 根，打开空白）；
 *  边缘计算卡片不带 GreptimeDB 链接（2026-11-16 用户定案，链接挂 GreptimeDB 自身卡片）。 */
const BROWSER_PAGES = new Set(["gateway-console", "dfy-ui", "nginx", "greptime"]);

/** 服务的地址列表（无地址的服务返回空，如 dfy-spatial 桌面应用、dfy-backend 后端 API）。 */
export function accessLinks(
  id: string,
  opts?: { kind?: string; ports?: number[]; remote?: [string, number] | null },
): AccessLink[] {
  const remoteHost = opts?.remote?.[0] ?? "192.168.56.101";
  if (id === "minio") {
    return [
      { label: "MinIO 控制台", url: `http://${remoteHost}:9001/` },
      { label: "登录账号", text: "dfy / dfy123456" },
    ];
  }
  if (id === "mysql") return [{ label: "MySQL 数据库", text: "127.0.0.1:3306" }];
  const known = KNOWN[id];
  if (known?.url) {
    const out: AccessLink[] = BROWSER_PAGES.has(id) ? [{ label: known.how, url: known.url }] : [];
    return out;
  }
  const kind = opts?.kind ?? "";
  const ports = opts?.ports ?? [];
  if (["vertx", "vue_spa", "angular_spa", "rust_bin"].includes(kind) && ports.length) {
    return [{ label: "服务入口", url: `http://localhost:${ports[0]}/` }];
  }
  return [];
}

/** 组装访问入口文案：与卡片展示同源（accessLinks 白名单）——
 *  后端 API/连接类端口（8084、48085、18830、6379 等）不对外展示。 */
export function accessLine(
  id: string,
  opts?: { kind?: string; ports?: number[]; remote?: [string, number] | null },
): string | null {
  const links = accessLinks(id, opts);
  if (!links.length) return null;
  return links.map((l) => `${l.label} ${l.url ?? l.text ?? ""}`).join("；");
}
