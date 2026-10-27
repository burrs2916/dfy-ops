import { useCallback, useEffect, useState } from "react";
import {
  Alert,
  Box,
  Button,
  Card,
  CardContent,
  CircularProgress,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import { api } from "../api";
import type { ExternalConfig } from "../types";

/**
 * 云端 Java 服务外部化配置面板。
 *
 * Spring Boot 2.4+ 自动加载 jar 同级 config/application-*.yaml 且优先级高于
 * jar 内配置——本面板直接读写 {install_dir}/config/application-local.yaml，
 * 保存后提示用户「重启服务生效」。字段覆盖端口 / MySQL / Redis / VerneMQ 连接。
 */

const EMPTY: ExternalConfig = {
  serverPort: "",
  mysqlMasterUrl: "",
  mysqlUsername: "",
  mysqlPassword: "",
  mysqlSlaveUrl: "",
  redisHost: "",
  redisPort: "",
  redisPassword: "",
  vernemqHostUrl: "",
  vernemqUsername: "",
  vernemqPassword: "",
};

/** 一个配置分组：标题 + 字段网格。 */
function FieldGroup({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <Box>
      <Typography variant="body2" sx={{ color: "text.secondary", mb: 1 }}>
        {title}
      </Typography>
      <Stack spacing={1.5}>{children}</Stack>
    </Box>
  );
}

export default function ExternalConfigPanel({ serviceId }: { serviceId: string }) {
  const [cfg, setCfg] = useState<ExternalConfig | null>(null);
  const [exists, setExists] = useState(true);
  const [configPath, setConfigPath] = useState("");
  const [hint, setHint] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [msg, setMsg] = useState<{ severity: "success" | "error"; text: string } | null>(null);

  const set = (k: keyof ExternalConfig) => (e: React.ChangeEvent<HTMLInputElement>) => {
    setCfg((c) => (c ? { ...c, [k]: e.target.value } : c));
  };

  const load = useCallback(async () => {
    setLoading(true);
    setMsg(null);
    try {
      const r = await api.getExternalConfig(serviceId);
      setConfigPath(r.configPath);
      setExists(r.exists);
      setHint(r.hint ?? "");
      setCfg(r.config ?? EMPTY);
    } catch (e) {
      setMsg({ severity: "error", text: "读取外部配置失败：" + (e as Error).message });
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
      const r = await api.updateExternalConfig(serviceId, cfg);
      if (r.ok) {
        setMsg({ severity: "success", text: "已写入外部配置。重启服务后生效。" });
      } else {
        setMsg({ severity: "error", text: r.error ?? "写入失败" });
      }
    } catch (e) {
      setMsg({ severity: "error", text: "写入外部配置失败：" + (e as Error).message });
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card variant="outlined">
      <CardContent>
        <Typography variant="subtitle2" sx={{ mb: 0.5 }}>
          外部化配置 · 可运维
        </Typography>
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 1.5 }}>
          {configPath
            ? `外部配置优先级高于 jar 内置配置（Spring Boot config/ 目录覆盖机制）。修改后需重启服务生效。`
            : ""}
        </Typography>

        {loading ? (
          <Stack direction="row" spacing={1} alignItems="center">
            <CircularProgress size={16} />
            <Typography variant="body2" color="text.secondary">
              读取外部配置…
            </Typography>
          </Stack>
        ) : msg ? (
          <Alert severity={msg.severity} onClose={() => setMsg(null)}>
            {msg.text}
          </Alert>
        ) : !exists ? (
          <Alert severity="info">
            {hint || "外部配置不存在，服务使用 jar 内置默认配置。"}
            <Box component="code" sx={{ display: "block", mt: 1, fontSize: 12 }}>
              {configPath}
            </Box>
          </Alert>
        ) : cfg ? (
          <Stack spacing={2.5}>
            <FieldGroup title="服务端口">
              <TextField
                size="small"
                label="server.port"
                value={cfg.serverPort}
                onChange={set("serverPort")}
                sx={{ maxWidth: 240 }}
              />
            </FieldGroup>

            <FieldGroup title="MySQL 数据库">
              <TextField
                size="small"
                label="主库连接 URL"
                value={cfg.mysqlMasterUrl}
                onChange={set("mysqlMasterUrl")}
              />
              <Stack direction="row" spacing={1.5}>
                <TextField
                  size="small"
                  label="用户名"
                  value={cfg.mysqlUsername}
                  onChange={set("mysqlUsername")}
                  sx={{ maxWidth: 240 }}
                />
                <TextField
                  size="small"
                  label="密码"
                  type="password"
                  value={cfg.mysqlPassword}
                  onChange={set("mysqlPassword")}
                  sx={{ maxWidth: 240 }}
                />
              </Stack>
              <TextField
                size="small"
                label="从库连接 URL（可留空不修改）"
                value={cfg.mysqlSlaveUrl}
                onChange={set("mysqlSlaveUrl")}
              />
            </FieldGroup>

            <FieldGroup title="Redis">
              <Stack direction="row" spacing={1.5}>
                <TextField
                  size="small"
                  label="地址"
                  value={cfg.redisHost}
                  onChange={set("redisHost")}
                  sx={{ maxWidth: 240 }}
                />
                <TextField
                  size="small"
                  label="端口"
                  value={cfg.redisPort}
                  onChange={set("redisPort")}
                  sx={{ maxWidth: 120 }}
                />
                <TextField
                  size="small"
                  label="密码"
                  type="password"
                  value={cfg.redisPassword}
                  onChange={set("redisPassword")}
                  sx={{ maxWidth: 240 }}
                />
              </Stack>
            </FieldGroup>

            <FieldGroup title="VerneMQ（IoT MQTT 连接）">
              <TextField
                size="small"
                label="Broker 地址"
                value={cfg.vernemqHostUrl}
                onChange={set("vernemqHostUrl")}
                sx={{ maxWidth: 360 }}
              />
              <Stack direction="row" spacing={1.5}>
                <TextField
                  size="small"
                  label="账号"
                  value={cfg.vernemqUsername}
                  onChange={set("vernemqUsername")}
                  sx={{ maxWidth: 240 }}
                />
                <TextField
                  size="small"
                  label="密码"
                  type="password"
                  value={cfg.vernemqPassword}
                  onChange={set("vernemqPassword")}
                  sx={{ maxWidth: 240 }}
                />
              </Stack>
            </FieldGroup>

            <Stack direction="row" spacing={1.5} alignItems="center">
              <Button variant="contained" onClick={save} disabled={saving}>
                {saving ? "保存中…" : "保存配置"}
              </Button>
              <Typography variant="caption" color="text.secondary">
                保存后请在上方点「重启」使配置生效
              </Typography>
            </Stack>
          </Stack>
        ) : null}
      </CardContent>
    </Card>
  );
}
