import { useEffect, useRef, useState } from "react";
import {
  Alert,
  Box,
  Button,
  Paper,
  Stack,
  TextField,
  Tooltip,
  Typography,
} from "@mui/material";
import { Dns, Radar, Refresh, TravelExplore } from "@mui/icons-material";
import { api } from "../api";
import { ServiceDiscoveryDialog } from "./ServiceDiscoveryDialog";

/** 可配置远程端点的第三方服务元数据（与后端检测口径一一对应） */
const SERVICES: { id: string; name: string; desc: string; defaultPort: number }[] = [
  { id: "redis", name: "Redis", desc: "缓存与键值存储", defaultPort: 6379 },
  { id: "vernemq", name: "VerneMQ", desc: "MQTT 消息代理（兼容标准 1883）", defaultPort: 18830 },
  { id: "minio", name: "MinIO", desc: "对象存储", defaultPort: 9000 },
];

export interface RemoteServicesPanelProps {
  /** 任一服务端点保存成功后回调，父级用于立即刷新服务状态 */
  onSaved?: () => void;
}

type EndpointState = { host: string; port: string };

/**
 * 远程部署面板（网关地址配置）。
 *
 * Redis / VerneMQ / MinIO 由外部环境（虚拟机 / 远程主机）承载，平台没有
 * 也不需要它们的本地安装包——这里集中查看与编辑各服务的远程地址，
 * 服务状态检测随之切换到该地址。网络环境变化（网段迁移、虚拟机换 IP）
 * 时，直接改地址或重新扫描即可，全程无需重新编译。
 */
export function RemoteServicesPanel({ onSaved }: RemoteServicesPanelProps) {
  const [values, setValues] = useState<Record<string, EndpointState>>({});
  const [configured, setConfigured] = useState<Record<string, boolean>>({});
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState<string | null>(null);
  const [msg, setMsg] = useState<{ severity: "success" | "error" | "info"; text: string } | null>(null);
  const [discovering, setDiscovering] = useState(false);
  // 读取完成时的值快照：给「重新读取」按钮展示是否有未保存修改
  const snapshotRef = useRef<string>("");

  const reload = async () => {
    setLoading(true);
    try {
      const results = await Promise.all(
        SERVICES.map(async (s) => {
          try {
            const r = await api.serviceRemote(s.id);
            return [s.id, r] as const;
          } catch {
            return [s.id, { host: null, port: null }] as const;
          }
        }),
      );
      const nextValues: Record<string, EndpointState> = {};
      const nextConfigured: Record<string, boolean> = {};
      for (const [id, r] of results) {
        nextValues[id] = { host: r.host ?? "", port: String(r.port ?? SERVICES.find((s) => s.id === id)?.defaultPort ?? "") };
        nextConfigured[id] = r.host !== null;
      }
      setValues(nextValues);
      setConfigured(nextConfigured);
      snapshotRef.current = JSON.stringify(nextValues);
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
      const r = await api.saveServiceRemote(id, host, port);
      if (!r.ok) {
        setMsg({ severity: "error", text: r.error ?? "保存失败" });
      } else {
        setConfigured((c) => ({ ...c, [id]: true }));
        setMsg({ severity: "success", text: `${id} 远程地址已更新为 ${host}:${port}，服务状态即将按新地址检测` });
        onSaved?.();
        // 回读落库值并刷新快照
        void reload();
      }
    } catch (e) {
      setMsg({ severity: "error", text: e instanceof Error ? e.message : String(e) });
    } finally {
      setSaving(null);
    }
  };

  return (
    <Box sx={{ maxWidth: 760, mx: "auto" }}>
      <Stack direction="row" spacing={1} alignItems="center" sx={{ mb: 0.5 }}>
        <Dns color="primary" />
        <Typography variant="h6" sx={{ fontWeight: 700 }}>
          远程部署配置
        </Typography>
      </Stack>
      <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>
        Redis / VerneMQ / MinIO 部署在虚拟机或远程主机上（平台不提供本地安装），
        按此处地址监测服务是否可达。网段变化后改地址或重新扫描即可，无需重新编译。
      </Typography>

      {msg && (
        <Alert severity={msg.severity} onClose={() => setMsg(null)} sx={{ mb: 1.5 }}>
          {msg.text}
        </Alert>
      )}

      {loading ? (
        <Typography variant="body2" color="text.secondary" sx={{ py: 4, textAlign: "center" }}>
          正在读取当前配置…
        </Typography>
      ) : (
        <Stack spacing={1.25}>
          {SERVICES.map((s) => {
            const v = values[s.id] ?? { host: "", port: String(s.defaultPort) };
            const isConfigured = configured[s.id];
            return (
              <Paper
                key={s.id}
                variant="outlined"
                sx={{
                  p: 1.5,
                  display: "flex",
                  flexDirection: { xs: "column", sm: "row" },
                  gap: 1.25,
                  alignItems: { xs: "stretch", sm: "center" },
                  borderColor: isConfigured ? "divider" : "warning.main",
                  bgcolor: isConfigured ? "background.paper" : "rgba(186,117,23,0.04)",
                }}
              >
                <Stack sx={{ width: 110, flexShrink: 0 }}>
                  <Typography variant="subtitle2" sx={{ fontWeight: 700 }}>
                    {s.name}
                  </Typography>
                  <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.35 }}>
                    {s.desc}
                  </Typography>
                </Stack>
                <TextField
                  size="small"
                  sx={{ flex: 1, minWidth: 0 }}
                  label="主机地址"
                  placeholder="虚拟机 / 主机的 IP"
                  value={v.host}
                  onChange={(e) => setValues((m) => ({ ...m, [s.id]: { ...v, host: e.target.value } }))}
                />
                <TextField
                  size="small"
                  label="端口"
                  sx={{ width: 110, flexShrink: 0 }}
                  value={v.port}
                  onChange={(e) => setValues((m) => ({ ...m, [s.id]: { ...v, port: e.target.value.replace(/[^0-9]/g, "") } }))}
                  inputProps={{ inputMode: "numeric" }}
                />
                <Tooltip title={isConfigured ? "保存修改" : "保存并启用远程监测"}>
                  <Button
                    variant="contained"
                    disabled={saving === s.id}
                    onClick={() => void save(s.id)}
                    sx={{ flexShrink: 0, alignSelf: { xs: "flex-end", sm: "center" } }}
                  >
                    {saving === s.id ? "保存中…" : "保存"}
                  </Button>
                </Tooltip>
              </Paper>
            );
          })}
        </Stack>
      )}

      <Alert severity="info" icon={<Radar fontSize="inherit" />} sx={{ mt: 2 }}>
        不知道 IP？用「扫描网段自动发现」：选择或手动输入虚拟机所在网段，
        平台逐台探测 {SERVICES.map((s) => s.name).join(" / ")} 的默认端口并确认服务身份，一键采用。
      </Alert>

      <Stack direction="row" spacing={1.5} sx={{ mt: 2 }}>
        <Button variant="outlined" startIcon={<TravelExplore />} onClick={() => setDiscovering(true)}>
          扫描网段自动发现
        </Button>
        <Button variant="outlined" color="inherit" startIcon={<Refresh />} onClick={() => void reload()}>
          重新读取
        </Button>
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
