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
    url: "http://localhost:8084/",
    how: "边缘计算 API",
    ports: [8084, 8085],
    extra: "GreptimeDB 控制台 http://localhost:4000/",
  },
  greptime: { url: "http://localhost:4000/", how: "GreptimeDB 控制台", ports: [4000] },
  "dfy-backend": { url: "http://localhost:48085/", how: "后端接口服务", ports: [48085] },
  mysql: { how: "MySQL 客户端连接 127.0.0.1:3306", ports: [3306] },
  "dfy-spatial": { how: "桌面应用：从桌面 / 开始菜单图标打开" },
};

/** 服务的浏览器地址（有才有）；批量启动时用来汇总「可访问：…」。 */
export function accessUrl(id: string): string | null {
  return KNOWN[id]?.url ?? null;
}

/** 组装「访问地址 / 访问方式」片段，如「访问地址 http://localhost:4200/（边缘网关控制台，端口 4200，浏览器打开）」。 */
export function accessLine(
  id: string,
  opts?: { kind?: string; ports?: number[]; remote?: [string, number] | null },
): string | null {
  // 远程部署型服务：地址按托管端点生成（未配置时按平台默认 192.168.56.101）；
  // MinIO 控制台在 :9001、S3 API 在 :9000，链接可直接点击打开
  const remoteHost = opts?.remote?.[0] ?? "192.168.56.101";
  if (id === "minio") {
    return `访问地址 http://${remoteHost}:9001/（MinIO 控制台，浏览器打开）；API http://${remoteHost}:9000/`;
  }
  if (id === "vernemq") return `访问方式 VerneMQ MQTT ${remoteHost}:18830（用户名 cloud）`;
  if (id === "redis") return `访问方式 Redis 连接 ${remoteHost}:6379`;

  const known = KNOWN[id];
  if (known) {
    if (known.url) {
      const ports = opts?.ports?.length ? opts.ports : known.ports ?? [];
      const portPart = ports.length ? `，端口 ${ports.join("、")}` : "";
      const extra = known.extra ? `；${known.extra}` : "";
      return `访问地址 ${known.url}（${known.how}${portPart}，浏览器打开）${extra}`;
    }
    return `访问方式 ${known.how}`;
  }

  // services.toml 自定义服务：按类型兜底
  const ports = opts?.ports ?? [];
  if (!ports.length) return null;
  const kind = opts?.kind ?? "";
  if (kind === "database" || kind === "mqtt_broker" || kind === "object_storage") {
    return `连接地址 127.0.0.1:${ports.join("、")}（端口 ${ports.join("、")}）`;
  }
  if (kind === "tauri_app") return "桌面应用：从桌面 / 开始菜单图标打开";
  if (["vertx", "vue_spa", "angular_spa", "rust_bin"].includes(kind)) {
    return `访问地址 http://localhost:${ports[0]}/（端口 ${ports.join("、")}，浏览器打开）`;
  }
  return null;
}
