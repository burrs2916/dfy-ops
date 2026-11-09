import { useCallback, useEffect, useState } from "react";
import {
  Alert,
  Box,
  Button,
  CircularProgress,
  Grid,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import SaveOutlinedIcon from "@mui/icons-material/SaveOutlined";
import UndoIcon from "@mui/icons-material/Undo";
import { api } from "../api";
import type { EdgeConfig, ExternalConfigResponse, ExternalConfigWriteResult } from "../types";

/**
 * 边缘计算（edge-conductor）config.toml 可视化编辑面板。
 *
 * 后端按 service id 分流到 ops::edge_config：TOML 段定位行级替换，
 * 保留注释与未托管段落；保存前语法校验 + 备份 + 原子写回。
 * TOML 无热加载——保存后需重启边缘计算（服务总览 → 启动）才生效。
 */

const EMPTY: EdgeConfig = {
  httpBind: "",
  httpPort: "",
  wsBind: "",
  wsPort: "",
  upstreamServerUrl: "",
  greptimeAddr: "",
  mqttBrokerUrl: "",
  mqttBrokerPort: "",
};

/** 端口合法性：空 = 保留原值（后端只替换命中的行），非空必须 1–65535 */
const portInvalid = (v: string) => {
  const t = v.trim();
  if (!t) return false;
  const n = Number(t);
  return !Number.isInteger(n) || n < 1 || n > 65535;
};

/** 上游地址：非空时必须 http(s):// 开头 */
const urlInvalid = (v: string) => {
  const t = v.trim();
  if (!t) return false;
  return !/^https?:\/\//.test(t);
};

/** host:port 形态地址（GreptimeDB addr / MQTT broker） */
const addrInvalid = (v: string) => {
  const t = v.trim();
  if (!t) return false;
  const parts = t.split(":");
  return parts.length !== 2 || portInvalid(parts[1]);
};

export default function EdgeConfigPanel({ serviceId }: { serviceId: string }) {
  const [cfg, setCfg] = useState<EdgeConfig | null>(null);
  const [exists, setExists] = useState(true);
  const [configPath, setConfigPath] = useState("");
  const [hint, setHint] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [msg, setMsg] = useState<{ severity: "success" | "error"; text: string } | null>(null);
  /** 已保存基线：dirty 判定 + 「重置」恢复到上次写盘的内容 */
  const [saved, setSaved] = useState<EdgeConfig | null>(null);

  const set = (k: keyof EdgeConfig) => (e: React.ChangeEvent<HTMLInputElement>) => {
    setCfg((c) => (c ? { ...c, [k]: e.target.value } : c));
  };

  const load = useCallback(async () => {
    setLoading(true);
    setMsg(null);
    try {
      const r: ExternalConfigResponse = await api.getExternalConfig(serviceId);
      setConfigPath(r.configPath);
      setExists(r.exists);
      setHint(r.hint ?? "");
      const loaded = (r.config as unknown as EdgeConfig) ?? EMPTY;
      setCfg(loaded);
      setSaved(loaded);
    } catch (e) {
      setMsg({ severity: "error", text: "读取 config.toml 失败：" + (e as Error).message });
    } finally {
      setLoading(false);
    }
  }, [serviceId]);

  useEffect(() => {
    void load();
  }, [load]);

  const save = async () => {
    if (!cfg) return;
    setSaving(true);
    setMsg(null);
    try {
      const r: ExternalConfigWriteResult = await api.updateExternalConfig(serviceId, cfg);
      if (r.ok) {
        setSaved({ ...cfg });
        setMsg({
          severity: "success",
          text: r.hint ?? "已写入 config.toml，重启边缘计算后生效。",
        });
      } else {
        setMsg({ severity: "error", text: r.error ?? "写入失败" });
      }
    } catch (e) {
      setMsg({ severity: "error", text: "写入 config.toml 失败：" + (e as Error).message });
    } finally {
      setSaving(false);
    }
  };

  // ===== 校验 =====
  const httpPortT = cfg?.httpPort.trim() ?? "";
  const wsPortT = cfg?.wsPort.trim() ?? "";
  const httpPortErr = portInvalid(httpPortT) ? "端口需为 1–65535" : undefined;
  const wsPortErr = portInvalid(wsPortT)
    ? "端口需为 1–65535"
    : httpPortT !== "" && httpPortT === wsPortT
      ? "与 HTTP 端口相同，会端口占用"
      : undefined;
  const httpBindErr = cfg && !cfg.httpBind.trim() ? "监听地址不能为空" : undefined;
  const wsBindErr = cfg && !cfg.wsBind.trim() ? "监听地址不能为空" : undefined;
  const upstreamErr = cfg && urlInvalid(cfg.upstreamServerUrl) ? "需以 http:// 或 https:// 开头" : undefined;
  const greptimeErr = cfg && addrInvalid(cfg.greptimeAddr) ? "格式需为 host:port" : undefined;
  const mqttUrlErr = cfg && !cfg.mqttBrokerUrl.trim() ? "Broker 地址不能为空" : undefined;
  const mqttPortErr = portInvalid(cfg?.mqttBrokerPort ?? "") ? "端口需为 1–65535" : undefined;
  const hasErrors = !!(
    httpPortErr ||
    wsPortErr ||
    httpBindErr ||
    wsBindErr ||
    upstreamErr ||
    greptimeErr ||
    mqttUrlErr ||
    mqttPortErr
  );

  // dirty：与已保存基线逐字段对比
  const dirty = !!cfg && !!saved && JSON.stringify(cfg) !== JSON.stringify(saved);

  if (loading) {
    return (
      <Stack alignItems="center" sx={{ py: 3 }}>
        <CircularProgress size={22} />
      </Stack>
    );
  }

  if (!exists) {
    return (
      <Stack spacing={1}>
        <Alert severity="info">{hint || "config.toml 不存在：请先安装边缘计算。"}</Alert>
        <Button size="small" onClick={() => void load()}>
          重新检测
        </Button>
      </Stack>
    );
  }

  return (
    <Stack spacing={1.5}>
      <Box sx={{ display: "flex", alignItems: "center", gap: 1, flexWrap: "wrap" }}>
        <Typography variant="caption" sx={{ fontFamily: "monospace", wordBreak: "break-all" }}>
          {configPath}
        </Typography>
        {dirty && (
          <Typography variant="caption" color="warning.main" sx={{ whiteSpace: "nowrap" }}>
            未保存
          </Typography>
        )}
      </Box>

      <Grid container spacing={1.5}>
        {/* --- HTTP API --- */}
        <Grid item xs={12} sm={6}>
          <TextField
            size="small"
            fullWidth
            label="API 监听地址"
            value={cfg?.httpBind ?? ""}
            onChange={set("httpBind")}
            error={!!httpBindErr}
            helperText={httpBindErr ?? "127.0.0.1 仅本机 / 0.0.0.0 全网卡"}
          />
        </Grid>
        <Grid item xs={12} sm={6}>
          <TextField
            size="small"
            fullWidth
            label="API 端口"
            value={cfg?.httpPort ?? ""}
            onChange={set("httpPort")}
            error={!!httpPortErr}
            helperText={httpPortErr ?? "默认 8084"}
          />
        </Grid>
        {/* --- WebSocket --- */}
        <Grid item xs={12} sm={6}>
          <TextField
            size="small"
            fullWidth
            label="WebSocket 监听地址"
            value={cfg?.wsBind ?? ""}
            onChange={set("wsBind")}
            error={!!wsBindErr}
            helperText={wsBindErr ?? "127.0.0.1 仅本机 / 0.0.0.0 全网卡"}
          />
        </Grid>
        <Grid item xs={12} sm={6}>
          <TextField
            size="small"
            fullWidth
            label="WebSocket 端口"
            value={cfg?.wsPort ?? ""}
            onChange={set("wsPort")}
            error={!!wsPortErr}
            helperText={wsPortErr ?? "默认 8085"}
          />
        </Grid>
        {/* --- 上游服务 --- */}
        <Grid item xs={12}>
          <TextField
            size="small"
            fullWidth
            label="上游 DFY 后端地址"
            value={cfg?.upstreamServerUrl ?? ""}
            onChange={set("upstreamServerUrl")}
            error={!!upstreamErr}
            helperText={upstreamErr ?? "如 http://127.0.0.1:48085——前后端同机即用本机回环地址"}
          />
        </Grid>
        {/* --- GreptimeDB --- */}
        <Grid item xs={12} sm={6}>
          <TextField
            size="small"
            fullWidth
            label="GreptimeDB 时序库地址"
            value={cfg?.greptimeAddr ?? ""}
            onChange={set("greptimeAddr")}
            error={!!greptimeErr}
            helperText={greptimeErr ?? "host:port，默认 127.0.0.1:4003"}
          />
        </Grid>
        {/* --- MQTT (VerneMQ) --- */}
        <Grid item xs={12} sm={6}>
          <TextField
            size="small"
            fullWidth
            label="MQTT Broker 地址"
            value={cfg?.mqttBrokerUrl ?? ""}
            onChange={set("mqttBrokerUrl")}
            error={!!mqttUrlErr}
            helperText={mqttUrlErr ?? "VerneMQ 所在主机"}
          />
        </Grid>
        <Grid item xs={12} sm={6}>
          <TextField
            size="small"
            fullWidth
            label="MQTT Broker 端口"
            value={cfg?.mqttBrokerPort ?? ""}
            onChange={set("mqttBrokerPort")}
            error={!!mqttPortErr}
            helperText={mqttPortErr ?? "默认 18830"}
          />
        </Grid>
      </Grid>

      {msg && (
        <Alert severity={msg.severity} sx={{ py: 0.5 }}>
          {msg.text}
        </Alert>
      )}

      <Stack direction="row" spacing={1}>
        <Button
          size="small"
          variant="contained"
          startIcon={saving ? <CircularProgress size={14} color="inherit" /> : <SaveOutlinedIcon />}
          disabled={saving || !dirty || hasErrors}
          onClick={() => void save()}
          sx={{ textTransform: "none" }}
        >
          保存（自动备份）
        </Button>
        <Button
          size="small"
          startIcon={<UndoIcon />}
          disabled={!dirty}
          onClick={() => setCfg(saved ? { ...saved } : null)}
          sx={{ textTransform: "none" }}
        >
          重置
        </Button>
      </Stack>
      <Typography variant="caption" color="text.secondary">
        仅托管上述 8 个关键字段，config.toml 其余段落与注释原样保留；保存前自动校验 TOML
        语法并备份原文件（config.toml.bak-*）。重启边缘计算后生效。
      </Typography>
    </Stack>
  );
}
