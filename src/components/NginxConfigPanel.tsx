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
  /** DFY 后端实际监听端口（外部配置 serverPort）——与反代目标不一致时提示联动 */
  const [backendPort, setBackendPort] = useState("");

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
      // 端口绑定联动：读后端外部配置的实际 serverPort，供反代端口一致性提示。
      // 后端未安装/读取失败静默忽略——联动只在两边都就绪时生效。
      try {
        const b = await api.getExternalConfig("dfy-backend");
        const sp = (b.config as Record<string, string> | undefined)?.serverPort;
        if (sp) setBackendPort(sp);
      } catch {
        /* 后端未安装，忽略 */
      }
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
        // hint 由后端生成：写入成功 + 自动热加载结果（运行中→立即生效；未运行→下次启动生效）
        setMsg({ severity: "success", text: r.hint ?? "已写入 nginx.conf。" });
      } else {
        setMsg({ severity: "error", text: r.error ?? "写入失败" });
      }
    } catch (e) {
      setMsg({ severity: "error", text: "写入 nginx.conf 失败：" + (e as Error).message });
    } finally {
      setSaving(false);
    }
  };

  // 端口绑定检查：反代目标端口 ≠ DFY 后端实际 serverPort（两者都非空时才比对）
  const portMismatch =
    !!backendPort && !!cfg?.backendPort && cfg.backendPort.trim() !== backendPort.trim();

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
            {portMismatch && (
              <Alert
                severity="warning"
                action={
                  <Button
                    size="small"
                    onClick={() => setCfg((c) => (c ? { ...c, backendPort } : c))}
                  >
                    同步为 {backendPort}
                  </Button>
                }
              >
                反代目标端口（{cfg.backendPort}）与 DFY 后端实际监听端口（{backendPort}）不一致——
                前端页面调 /admin-api/ 时会 502。点击右侧按钮对齐后保存。
              </Alert>
            )}
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
              <Box
                sx={{
                  mt: 0.5,
                  fontFamily: "monospace",
                  fontSize: 12,
                  bgcolor: "rgba(0,0,0,0.04)",
                  p: "4px 10px",
                  borderRadius: 1,
                  color: "#1e3f66",
                }}
              >
                proxy_pass http://{cfg.backendHost || "—"}:{cfg.backendPort || "—"}/api/;
              </Box>
            </FieldGroup>

            {/* 日志路径：高级，折叠 */}
            <details style={{ marginTop: 8 }}>
              <summary style={{ cursor: "pointer", fontSize: 12, color: "#6b7280", padding: "2px 0" }}>
                高级 · 日志路径（一般不动）
              </summary>
              <Stack direction="row" spacing={1.5} sx={{ mt: 1 }}>
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
            </details>

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
