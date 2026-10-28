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
import type { NginxConfig, ExternalConfigResponse, ExternalConfigWriteResult } from "../types";

/**
 * Nginx 反向代理配置可视化运维面板。
 *
 * 直接读写 {install_dir}/conf/nginx.conf，字段覆盖 listen/server_name/root/proxy_pass/
 * 日志路径。保存后提示「重载 nginx」——nginx 支持 `nginx -s reload` 热加载，
 * 不需要中断服务（后续可暴露重载按钮）。
 */

const EMPTY: NginxConfig = {
  listenPort: "",
  serverName: "",
  frontendRoot: "",
  backendHost: "",
  backendPort: "",
  accessLog: "",
  errorLog: "",
};

function FieldGroup({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <Box>
      <Typography variant="body2" sx={{ color: "text.secondary", mb: 1 }}>
        {title}
      </Typography>
      <Stack spacing={1.5}>{children}</Stack>
    </Box>
  );
}

export default function NginxConfigPanel({ serviceId }: { serviceId: string }) {
  const [cfg, setCfg] = useState<NginxConfig | null>(null);
  const [exists, setExists] = useState(true);
  const [configPath, setConfigPath] = useState("");
  const [hint, setHint] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [msg, setMsg] = useState<{ severity: "success" | "error"; text: string } | null>(null);

  const set = (k: keyof NginxConfig) => (e: React.ChangeEvent<HTMLInputElement>) => {
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
      // config 字段对 nginx 来说是 NginxConfig，但 JSON camelCase key 一致，Object.assign 可用
      setCfg((r.config as unknown as NginxConfig) ?? EMPTY);
    } catch (e) {
      setMsg({ severity: "error", text: "读取 nginx.conf 失败：" + (e as Error).message });
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
        setMsg({
          severity: "success",
          text: "已写入 nginx.conf。请在上方服务面板点「重载」或「重启」使配置生效。",
        });
      } else {
        setMsg({ severity: "error", text: r.error ?? "写入失败" });
      }
    } catch (e) {
      setMsg({ severity: "error", text: "写入 nginx.conf 失败：" + (e as Error).message });
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card variant="outlined">
      <CardContent>
        <Typography variant="subtitle2" sx={{ mb: 0.5 }}>
          Nginx 反向代理 · 可视化配置
        </Typography>
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 1.5 }}>
          {configPath
            ? `直接读写 nginx.conf 指令行，保留文件原始格式和未托管的自定义配置块。修改后 nginx -s reload 热加载即可生效。`
            : ""}
        </Typography>

        {loading ? (
          <Stack direction="row" spacing={1} alignItems="center">
            <CircularProgress size={16} />
            <Typography variant="body2" color="text.secondary">
              读取 nginx.conf…
            </Typography>
          </Stack>
        ) : msg ? (
          <Alert severity={msg.severity} onClose={() => setMsg(null)}>
            {msg.text}
          </Alert>
        ) : !exists ? (
          <Alert severity="info">
            {hint || "nginx.conf 不存在，请先安装 Nginx（资源目录 nginx zip）。"}
            <Box component="code" sx={{ display: "block", mt: 1, fontSize: 12 }}>
              {configPath}
            </Box>
          </Alert>
        ) : cfg ? (
          <Stack spacing={2.5}>
            <FieldGroup title="基础">
              <Stack direction="row" spacing={1.5}>
                <TextField
                  size="small"
                  label="监听端口（listen）"
                  value={cfg.listenPort}
                  onChange={set("listenPort")}
                  sx={{ maxWidth: 160 }}
                  helperText="HTTP 默认 80；改完记得 nginx -s reload"
                />
                <TextField
                  size="small"
                  label="主机名（server_name）"
                  value={cfg.serverName}
                  onChange={set("serverName")}
                  sx={{ maxWidth: 280 }}
                  helperText="localhost / 域名 / _ 通配"
                />
              </Stack>
            </FieldGroup>

            <FieldGroup title="前端静态资源">
              <TextField
                size="small"
                label="前端根目录（root）"
                value={cfg.frontendRoot}
                onChange={set("frontendRoot")}
                fullWidth
                helperText="前端 dist/ 解压目标，如 C:/services/dfy-ui-admin-vue3-master"
              />
            </FieldGroup>

            <FieldGroup title="后端 API 反代（/api/ → proxy_pass）">
              <Stack direction="row" spacing={1.5}>
                <TextField
                  size="small"
                  label="后端主机"
                  value={cfg.backendHost}
                  onChange={set("backendHost")}
                  sx={{ maxWidth: 200 }}
                  helperText="127.0.0.1 或内网 IP"
                />
                <TextField
                  size="small"
                  label="后端端口"
                  value={cfg.backendPort}
                  onChange={set("backendPort")}
                  sx={{ maxWidth: 140 }}
                  helperText="dfy-boot-base 默认 48085"
                />
              </Stack>
              <Typography variant="caption" color="text.secondary">
                反代地址自动拼接为 http://{cfg.backendHost || "host"}:{cfg.backendPort || "port"}/api/
              </Typography>
            </FieldGroup>

            <FieldGroup title="日志路径（一般不动）">
              <Stack direction="row" spacing={1.5}>
                <TextField
                  size="small"
                  label="访问日志（access_log）"
                  value={cfg.accessLog}
                  onChange={set("accessLog")}
                  sx={{ maxWidth: 400 }}
                />
                <TextField
                  size="small"
                  label="错误日志（error_log）"
                  value={cfg.errorLog}
                  onChange={set("errorLog")}
                  sx={{ maxWidth: 400 }}
                />
              </Stack>
            </FieldGroup>

            <Stack direction="row" spacing={1.5} alignItems="center">
              <Button variant="contained" onClick={save} disabled={saving}>
                {saving ? "保存中…" : "保存配置"}
              </Button>
              <Typography variant="caption" color="text.secondary">
                保存后请在上方面板点「重载」或「重启」使 nginx 重新加载配置
              </Typography>
            </Stack>
          </Stack>
        ) : null}
      </CardContent>
    </Card>
  );
}
