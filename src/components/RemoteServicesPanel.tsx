import { useEffect, useRef, useState } from "react";
import {
  Alert,
  Box,
  Button,
  Stack,
  TextField,
  Tooltip,
  Typography,
} from "@mui/material";
import { Dns, Radar, Refresh, TravelExplore } from "@mui/icons-material";
import { api } from "../api";
import { ServiceDiscoveryDialog } from "./ServiceDiscoveryDialog";

/** 可配置远程端点的第三方服务元数据（与后端检测口径一一对应）。
 *  四个服务均可选配访问密码：留空 = 无认证 / 保持已设密码不变。
 *  GreptimeDB 是边缘计算的附属时序库（随 Edge Conductor 启动），同样只配端点做可达性检测。 */
const SERVICES: {
  id: string;
  name: string;
  desc: string;
  defaultPort: number;
}[] = [
  { id: "redis", name: "Redis", desc: "缓存与键值存储", defaultPort: 6379 },
  { id: "vernemq", name: "VerneMQ", desc: "MQTT 消息代理（兼容标准 1883）", defaultPort: 18830 },
  { id: "minio", name: "MinIO", desc: "对象存储", defaultPort: 9000 },
  { id: "greptime", name: "GreptimeDB", desc: "时序数据库（随边缘计算启动）", defaultPort: 4000 },
];

/** 网格列布局：服务标识 | 主机 | 端口 | 访问密码 | 操作。
 *  固定模板保证各行的输入框纵向严格对齐，扫一眼就是一张配置表。 */
const ROW_GRID = {
  display: "grid",
  gridTemplateColumns: { xs: "1fr", sm: "minmax(180px, 220px) minmax(0, 1fr) 110px 190px 96px" },
  gap: 1.5,
  alignItems: "center",
} as const;

export interface RemoteServicesPanelProps {
  /** 任一服务端点保存成功后回调，父级用于立即刷新服务状态 */
  onSaved?: () => void;
}

type EndpointState = { host: string; port: string; password: string };

/**
 * 远程部署面板（网关地址配置）。
 *
 * Redis / VerneMQ / MinIO / GreptimeDB 由外部环境（虚拟机 / 远程主机）承载，平台没有
 * 也不需要它们的本地安装包——这里集中查看与编辑各服务的远程地址与访问密码，
 * 服务状态检测随之切换到该地址。网络环境变化（网段迁移、虚拟机换 IP）
 * 时，直接改地址或重新扫描即可，全程无需重新编译。
 *
 * 持久化：保存即写入应用配置目录（remote_endpoints.json / redis_credential.json），
 * 重启后仍生效——每个服务只配一次。
 */
export function RemoteServicesPanel({ onSaved }: RemoteServicesPanelProps) {
  const [values, setValues] = useState<Record<string, EndpointState>>({});
  const [configured, setConfigured] = useState<Record<string, boolean>>({});
  const [passwordSet, setPasswordSet] = useState<Record<string, boolean>>({});
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState<string | null>(null);
  const [msg, setMsg] = useState<{ severity: "success" | "error"; text: string } | null>(null);
  const [discovering, setDiscovering] = useState(false);
  const msgAnchor = useRef<HTMLDivElement | null>(null);

  const reload = async () => {
    setLoading(true);
    try {
      const results = await Promise.all(
        SERVICES.map(async (s) => {
          try {
            const r = await api.serviceRemote(s.id);
            return [s.id, r] as const;
          } catch {
            return [s.id, { host: null, port: null, password_set: false }] as const;
          }
        }),
      );
      const nextValues: Record<string, EndpointState> = {};
      const nextConfigured: Record<string, boolean> = {};
      const nextPwdSet: Record<string, boolean> = {};
      for (const [id, r] of results) {
        nextValues[id] = {
          host: r.host ?? "",
          port: String(r.port ?? SERVICES.find((s) => s.id === id)?.defaultPort ?? ""),
          password: "",
        };
        nextConfigured[id] = r.host !== null;
        nextPwdSet[id] = Boolean(r.password_set);
      }
      setValues(nextValues);
      setConfigured(nextConfigured);
      setPasswordSet(nextPwdSet);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const save = async (id: string) => {
    const v = values[id];
    const host = v.host.trim();
    if (!host) {
      setMsg({ severity: "error", text: `${id} 的主机地址不能为空——请填写虚拟机/主机的 IP 地址` });
      return;
    }
    const port = Number(v.port);
    if (!Number.isInteger(port) || port <= 0 || port > 65535) {
      setMsg({ severity: "error", text: `端口「${v.port}」无效：请填写 1~65535 的整数` });
      return;
    }
    setSaving(id);
    setMsg(null);
    try {
      const pwd = v.password.trim();
      const r = await api.saveServiceRemote(id, host, port, pwd);
      if (!r.ok) {
        setMsg({ severity: "error", text: r.error ?? "保存失败" });
        return;
      }
      setConfigured((c) => ({ ...c, [id]: true }));
      setPasswordSet((m) => ({ ...m, [id]: pwd ? true : m[id] }));
      const svcName = SERVICES.find((s) => s.id === id)?.name ?? id;
      const pwdNote = pwd ? "、访问密码已更新" : "";
      setMsg({
        severity: "success",
        text: `${svcName} 远程地址已保存为 ${host}:${port}${pwdNote}——已写入配置文件持久化，重启应用后仍生效；服务状态即将按新地址检测。`,
      });
      onSaved?.();
      void reload();
    } catch (e) {
      setMsg({ severity: "error", text: e instanceof Error ? e.message : String(e) });
    } finally {
      setSaving(null);
    }
  };

  return (
    <Box sx={{ flex: 1, minHeight: 0, overflow: "auto" }}>
      {/* 页头：标题说明居左，扫描入口居右——主操作（改地址保存）在表内，扫描是辅助 */}
      <Stack
        direction={{ xs: "column", sm: "row" }}
        justifyContent="space-between"
        alignItems={{ xs: "flex-start", sm: "center" }}
        spacing={1}
        sx={{ mb: 2 }}
      >
        <Box>
          <Stack direction="row" spacing={1} alignItems="center">
            <Dns color="primary" />
            <Typography variant="h6" sx={{ fontWeight: 700 }}>
              远程部署配置
            </Typography>
          </Stack>
          <Typography variant="body2" color="text.secondary" sx={{ mt: 0.5 }}>
            Redis / VerneMQ / MinIO / GreptimeDB 部署在虚拟机或远程主机上（平台不提供本地安装），
            按此处地址监测服务可达性；保存即持久化，每个服务只配置一次。
          </Typography>
        </Box>
        <Button variant="outlined" startIcon={<TravelExplore />} onClick={() => setDiscovering(true)}>
          扫描网段自动发现
        </Button>
      </Stack>

      {msg && (
        <Box ref={msgAnchor}>
          <Alert severity={msg.severity} onClose={() => setMsg(null)} sx={{ mb: 1.5 }}>
            {msg.text}
          </Alert>
        </Box>
      )}

      {loading ? (
        <Typography variant="body2" color="text.secondary" sx={{ py: 6, textAlign: "center" }}>
          正在读取当前配置…
        </Typography>
      ) : (
        <Box>
          {/* 表头 */}
          <Box sx={{ ...ROW_GRID, px: 1.5, pb: 0.75 }}>
            {["服务", "主机地址（虚拟机 / 主机 IP）", "端口", "访问密码", ""].map((h, i) => (
              <Typography
                key={i}
                variant="caption"
                sx={{ fontWeight: 700, color: "text.secondary", letterSpacing: 0.3 }}
              >
                {h}
              </Typography>
            ))}
          </Box>
          <Stack spacing={1}>
            {SERVICES.map((s) => {
              const v = values[s.id] ?? { host: "", port: String(s.defaultPort), password: "" };
              const isConfigured = configured[s.id];
              return (
                <Box
                  key={s.id}
                  sx={{
                    ...ROW_GRID,
                    px: 1.5,
                    py: 1.5,
                    borderRadius: 1,
                    border: "1px solid",
                    borderColor: isConfigured ? "divider" : "warning.main",
                    bgcolor: isConfigured ? "background.paper" : "rgba(186,117,23,0.05)",
                  }}
                >
                  <Box sx={{ display: "flex", alignItems: "center", gap: 1, minWidth: 0 }}>
                    <Dns fontSize="small" sx={{ color: "primary.main", flexShrink: 0 }} />
                    <Box sx={{ minWidth: 0 }}>
                      <Stack direction="row" spacing={0.75} alignItems="center">
                        <Typography variant="subtitle2" sx={{ fontWeight: 700 }}>
                          {s.name}
                        </Typography>
                        <Typography
                          variant="caption"
                          sx={{
                            px: 0.6,
                            borderRadius: 0.75,
                            fontSize: 11,
                            fontWeight: 700,
                            bgcolor: isConfigured ? "rgba(30,63,102,0.10)" : "rgba(186,117,23,0.12)",
                            color: isConfigured ? "primary.main" : "warning.main",
                          }}
                        >
                          {isConfigured ? "已配置" : "待配置"}
                        </Typography>
                      </Stack>
                      <Typography
                        variant="caption"
                        color="text.secondary"
                        sx={{ display: "block", lineHeight: 1.35 }}
                        noWrap
                      >
                        {s.desc}
                      </Typography>
                    </Box>
                  </Box>
                  <TextField
                    size="small"
                    placeholder="如 192.168.56.101"
                    value={v.host}
                    onChange={(e) => setValues((m) => ({ ...m, [s.id]: { ...v, host: e.target.value } }))}
                  />
                  <TextField
                    size="small"
                    value={v.port}
                    onChange={(e) =>
                      setValues((m) => ({ ...m, [s.id]: { ...v, port: e.target.value.replace(/[^0-9]/g, "") } }))
                    }
                    inputProps={{ inputMode: "numeric" }}
                  />
                  <TextField
                    size="small"
                    type="password"
                    placeholder={passwordSet[s.id] ? "已设置 · 留空不改" : "无密码可留空"}
                    value={v.password}
                    onChange={(e) => setValues((m) => ({ ...m, [s.id]: { ...v, password: e.target.value } }))}
                  />
                  <Tooltip title="保存并立即持久化，服务状态按新地址检测">
                    <Button variant="contained" disabled={saving === s.id} onClick={() => void save(s.id)}>
                      {saving === s.id ? "保存中…" : "保存"}
                    </Button>
                  </Tooltip>
                </Box>
              );
            })}
          </Stack>
        </Box>
      )}

      <Stack spacing={1} sx={{ mt: 2 }}>
        <Alert severity="info" icon={<Radar fontSize="inherit" />}>
          不知道 IP？点右上角「扫描网段自动发现」：选择或手动输入虚拟机所在网段，
          平台逐台探测 {SERVICES.map((s) => s.name).join(" / ")} 的端口并确认服务身份，一键采用。
        </Alert>
        <Alert severity="success" icon={<Refresh fontSize="inherit" />} variant="outlined">
          所有配置保存即写入应用配置目录，重启后自动加载，无需重复配置；服务状态检测共用同一份数据，
          改一处全局生效。
        </Alert>
      </Stack>

      {/* 全服务发现：采用后立即重读配置，让用户马上看到落库结果 */}
      <ServiceDiscoveryDialog
        open={discovering}
        onClose={() => setDiscovering(false)}
        targets={SERVICES.map((s) => s.id)}
        onAdopted={() => void reload()}
      />
    </Box>
  );
}

export default RemoteServicesPanel;
