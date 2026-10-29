import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Alert,
  Box,
  Button,
  Card,
  CardContent,
  Chip,
  Collapse,
  CircularProgress,
  Divider,
  Grid,
  IconButton,
  InputAdornment,
  Paper,
  Stack,
  TextField,
  Tooltip,
  Typography,
} from "@mui/material";
import CableIcon from "@mui/icons-material/Cable";
import DnsIcon from "@mui/icons-material/Dns";
import FolderOpenIcon from "@mui/icons-material/FolderOpen";
import DescriptionIcon from "@mui/icons-material/Description";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import UndoIcon from "@mui/icons-material/Undo";
import { api } from "../api";
import type { NginxConfig, ExternalConfigResponse, ExternalConfigWriteResult } from "../types";
import { MONO_STACK } from "../theme";

/**
 * Nginx 反向代理配置可视化运维面板。
 *
 * 直接读写 {install_dir}/conf/nginx.conf，字段覆盖 listen/server_name/root/proxy_pass/
 * 日志路径。保存后由后端自动 `nginx -s reload`（运行中立即生效）。
 * 分组卡片布局：接入层 / 前端静态资源 / API 反代（实时预览）/ 日志（高级）。
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

/** 配置分组卡片：图标圆底 + 标题 + 一句话说明 + 字段区 */
function ConfCard({
  icon,
  title,
  desc,
  badge,
  children,
}: {
  icon: React.ReactNode;
  title: string;
  desc: string;
  badge?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <Paper variant="outlined" sx={{ height: "100%", display: "flex", flexDirection: "column" }}>
      <Stack direction="row" alignItems="center" spacing={1} sx={{ px: 1.75, pt: 1.5 }}>
        <Box
          sx={{
            width: 28,
            height: 28,
            borderRadius: 1,
            display: "grid",
            placeItems: "center",
            bgcolor: "primary.main",
            color: "primary.contrastText",
            flexShrink: 0,
          }}
        >
          {icon}
        </Box>
        <Box sx={{ flex: 1, minWidth: 0 }}>
          <Typography variant="subtitle2" sx={{ fontWeight: 700, lineHeight: 1.2 }}>
            {title}
          </Typography>
          <Typography variant="caption" color="text.secondary">
            {desc}
          </Typography>
        </Box>
        {badge}
      </Stack>
      <Divider sx={{ my: 1.25, mx: 1.75 }} />
      <Stack spacing={1.5} sx={{ px: 1.75, pb: 1.75, flex: 1 }}>
        {children}
      </Stack>
    </Paper>
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
  /** 已保存基线：dirty 判定 + 「重置」恢复到上次写盘的内容 */
  const [saved, setSaved] = useState<NginxConfig | null>(null);
  /** Nginx 运行状态：true=运行中（保存即热加载）/ false=已停止（下次启动生效）/ null=探测失败 */
  const [running, setRunning] = useState<boolean | null>(null);
  /** 正在打开的目录（conf/logs）：按钮防抖 loading */
  const [openingDir, setOpeningDir] = useState<string | null>(null);
  /** 高级日志路径折叠区 */
  const [advOpen, setAdvOpen] = useState(false);

  const set = (k: keyof NginxConfig) => (e: React.ChangeEvent<HTMLInputElement>) => {
    setCfg((c) => (c ? { ...c, [k]: e.target.value } : c));
  };

  /** 探测 Nginx 运行状态。失败静默：只影响状态角标，不阻塞配置编辑。 */
  const refreshRunning = useCallback(async () => {
    try {
      const s = await api.refreshService(serviceId);
      setRunning(s.status.running);
    } catch {
      setRunning(null);
    }
  }, [serviceId]);

  const load = useCallback(async () => {
    setLoading(true);
    setMsg(null);
    void refreshRunning();
    try {
      const r: ExternalConfigResponse = await api.getExternalConfig(serviceId);
      setConfigPath(r.configPath);
      setExists(r.exists);
      setHint(r.hint ?? "");
      const loaded = (r.config as unknown as NginxConfig) ?? EMPTY;
      setCfg(loaded);
      setSaved(loaded);
      // 端口绑定联动：读后端外部配置的实际 serverPort，供反代端口一致性提示。
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
  }, [serviceId, refreshRunning]);

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
        // 写盘成功 → 当前表单即成为新基线；热加载结果顺带刷新运行状态角标
        setSaved({ ...cfg });
        setMsg({ severity: "success", text: r.hint ?? "已写入 nginx.conf。" });
        void refreshRunning();
      } else {
        setMsg({ severity: "error", text: r.error ?? "写入失败" });
      }
    } catch (e) {
      setMsg({ severity: "error", text: "写入 nginx.conf 失败：" + (e as Error).message });
    } finally {
      setSaving(false);
    }
  };

  /** 在系统文件管理器中打开目录：conf=配置目录（nginx.conf），logs=日志目录（access/error.log）。 */
  const openDir = async (dir?: "logs") => {
    setOpeningDir(dir ?? "conf");
    try {
      const r = await api.openConfigDir(serviceId, dir);
      if (!r.ok) setMsg({ severity: "error", text: r.error ?? "打开目录失败" });
    } catch (e) {
      setMsg({ severity: "error", text: "打开目录失败：" + (e as Error).message });
    } finally {
      setOpeningDir(null);
    }
  };

  // ===== 校验 =====
  // 端口合法性：空 = 保留 nginx.conf 原值（后端 apply 的语义），非空必须是 1–65535 整数
  const portInvalid = (v: string | undefined) => {
    const t = (v ?? "").trim();
    if (!t) return false;
    const n = Number(t);
    return !Number.isInteger(n) || n < 1 || n > 65535;
  };
  const listenPortT = cfg?.listenPort.trim() ?? "";
  const backendHostT = cfg?.backendHost.trim() ?? "";
  const backendPortT = cfg?.backendPort.trim() ?? "";
  // 同机端口占用：反代目标在本机（回环/默认原值）且监听端口 == 反代端口时，
  // nginx 与 Java 抢同一端口必然有一个起不来——面板里唯一必须拦下的硬错误
  const sameMachine =
    backendHostT === "" || ["127.0.0.1", "localhost", "::1"].includes(backendHostT);
  const portClash = sameMachine && listenPortT !== "" && listenPortT === backendPortT;
  const listenErr = portInvalid(cfg?.listenPort)
    ? "端口需为 1–65535"
    : portClash
      ? "与后端端口相同，会端口占用"
      : undefined;
  const backendErr = portInvalid(cfg?.backendPort)
    ? "端口需为 1–65535"
    : portClash
      ? "与监听端口相同，会端口占用"
      : undefined;
  const hasErrors = !!(listenErr || backendErr);

  // 端口绑定检查：反代目标端口 ≠ DFY 后端实际 serverPort（两者都非空时才比对）
  const portMismatch =
    !!backendPort && !!cfg?.backendPort && cfg.backendPort.trim() !== backendPort.trim();

  // dirty：与已保存基线比较（键序一致，JSON 对比即逐字段对比）
  const dirty = useMemo(
    () => !!cfg && !!saved && JSON.stringify(cfg) !== JSON.stringify(saved),
    [cfg, saved],
  );

  const reset = () => setCfg(saved ? { ...saved } : null);

  const proxyPreview = `proxy_pass http://${cfg?.backendHost || "—"}:${cfg?.backendPort || "—"}/admin-api/;`;

  return (
    <Card variant="outlined" sx={{ position: "relative" }}>
      <CardContent>
        <Stack direction="row" alignItems="center" spacing={1.25} sx={{ mb: 1.5 }}>
          <Box
            sx={{
              width: 34,
              height: 34,
              borderRadius: 1.25,
              display: "grid",
              placeItems: "center",
              bgcolor: "primary.main",
              color: "primary.contrastText",
            }}
          >
            <DnsIcon fontSize="small" />
          </Box>
          <Box sx={{ flex: 1, minWidth: 0 }}>
            <Typography variant="subtitle1" sx={{ fontWeight: 800, lineHeight: 1.2 }}>
              Nginx 反向代理
            </Typography>
            {loading ? (
              <Typography variant="caption" color="text.secondary">
                读取 nginx.conf…
              </Typography>
            ) : (
              <Tooltip title={configPath}>
                <Typography
                  variant="caption"
                  sx={{ color: "text.secondary", fontFamily: MONO_STACK, display: "block" }}
                  noWrap
                >
                  {configPath}
                </Typography>
              </Tooltip>
            )}
          </Box>
          {!loading && (
            <Stack direction="row" alignItems="center" spacing={0.5}>
              {exists && (
                <>
                  <Tooltip title="打开配置目录（nginx.conf）">
                    <IconButton size="small" onClick={() => openDir()} disabled={openingDir !== null}>
                      {openingDir === "conf" ? (
                        <CircularProgress size={16} />
                      ) : (
                        <FolderOpenIcon fontSize="small" />
                      )}
                    </IconButton>
                  </Tooltip>
                  <Tooltip title="打开日志目录（access.log / error.log）">
                    <IconButton size="small" onClick={() => openDir("logs")} disabled={openingDir !== null}>
                      {openingDir === "logs" ? (
                        <CircularProgress size={16} />
                      ) : (
                        <DescriptionIcon fontSize="small" />
                      )}
                    </IconButton>
                  </Tooltip>
                </>
              )}
              <Chip
                size="small"
                variant="outlined"
                color={
                  !exists ? "default" : running === true ? "success" : running === false ? "warning" : "default"
                }
                label={
                  !exists
                    ? "未安装 / 无配置"
                    : running === true
                      ? "运行中"
                      : running === false
                        ? "已停止"
                        : "配置就绪"
                }
              />
            </Stack>
          )}
        </Stack>

        {loading ? (
          <Stack alignItems="center" spacing={1} sx={{ py: 5 }}>
            <CircularProgress size={24} />
            <Typography variant="body2" color="text.secondary">
              正在解析配置…
            </Typography>
          </Stack>
        ) : (
          <Stack spacing={2}>
            {msg && (
              <Alert severity={msg.severity} onClose={() => setMsg(null)}>
                {msg.text}
              </Alert>
            )}
            {!exists ? (
              <Alert severity="info">
                {hint || "nginx.conf 不存在，请先安装 Nginx（修复中心 → 待安装 → 一键安装）。"}
                <Box component="code" sx={{ display: "block", mt: 1, fontSize: 12, fontFamily: MONO_STACK }}>
                  {configPath}
                </Box>
              </Alert>
            ) : (
              <>
                {portMismatch && cfg && (
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

                <Grid container spacing={2}>
                  {/* ① 接入层 */}
                  <Grid item xs={12} md={6}>
                    <ConfCard
                      icon={<DnsIcon sx={{ fontSize: 17 }} />}
                      title="接入层"
                      desc="浏览器访问入口"
                      badge={
                        cfg?.listenPort === "80" ? (
                          <Chip size="small" label="HTTP 80" variant="outlined" sx={{ height: 20 }} />
                        ) : undefined
                      }
                    >
                      <Stack direction="row" spacing={1.5}>
                        <TextField
                          size="small"
                          label="监听端口"
                          value={cfg?.listenPort ?? ""}
                          onChange={set("listenPort")}
                          type="number"
                          sx={{ width: 140 }}
                          error={!!listenErr}
                          helperText={listenErr ?? "默认 80；留空保留原值"}
                        />
                        <TextField
                          size="small"
                          label="主机名（server_name）"
                          value={cfg?.serverName ?? ""}
                          onChange={set("serverName")}
                          fullWidth
                          helperText="localhost / 域名 / _ 通配"
                        />
                      </Stack>
                      <Typography variant="caption" color="text.secondary" sx={{ fontFamily: MONO_STACK }}>
                        访问入口：http://localhost:{cfg?.listenPort || "80"}/
                      </Typography>
                    </ConfCard>
                  </Grid>

                  {/* ② 前端静态资源 */}
                  <Grid item xs={12} md={6}>
                    <ConfCard
                      icon={<FolderOpenIcon sx={{ fontSize: 17 }} />}
                      title="前端静态资源"
                      desc="Vue3 dist 解压目录"
                    >
                      <TextField
                        size="small"
                        label="前端根目录（root）"
                        value={cfg?.frontendRoot ?? ""}
                        onChange={set("frontendRoot")}
                        fullWidth
                        InputProps={{
                          sx: { fontFamily: MONO_STACK, fontSize: 13 },
                        }}
                        helperText="指向 index.html 所在目录，SPA 路由自动兜底"
                      />
                    </ConfCard>
                  </Grid>

                  {/* ③ API 反代 */}
                  <Grid item xs={12}>
                    <ConfCard
                      icon={<CableIcon sx={{ fontSize: 17 }} />}
                      title="后端 API 反代"
                      desc="/admin-api/ 请求转发到 Spring Boot 服务"
                      badge={
                        portMismatch ? (
                          <Chip size="small" color="warning" label="端口不一致" sx={{ height: 20 }} />
                        ) : (
                          <Chip size="small" color="success" label="端口一致" variant="outlined" sx={{ height: 20 }} />
                        )
                      }
                    >
                      <Stack direction={{ xs: "column", sm: "row" }} spacing={1.5}>
                        <TextField
                          size="small"
                          label="后端主机"
                          value={cfg?.backendHost ?? ""}
                          onChange={set("backendHost")}
                          InputProps={{
                            sx: { fontFamily: MONO_STACK, fontSize: 13 },
                            startAdornment: <InputAdornment position="start">http://</InputAdornment>,
                          }}
                          sx={{ flex: 1 }}
                          helperText="127.0.0.1 或内网 IP"
                        />
                        <TextField
                          size="small"
                          label="后端端口"
                          value={cfg?.backendPort ?? ""}
                          onChange={set("backendPort")}
                          type="number"
                          sx={{ width: 140 }}
                          error={!!backendErr}
                          helperText={
                            backendErr ??
                            (backendPort ? `后端实际端口 ${backendPort}` : "dfy-boot-base 默认 48085")
                          }
                        />
                      </Stack>
                      <Box
                        sx={{
                          fontFamily: MONO_STACK,
                          fontSize: 12,
                          bgcolor: "action.hover",
                          px: 1.25,
                          py: 0.75,
                          borderRadius: 1,
                          wordBreak: "break-all",
                        }}
                      >
                        {proxyPreview}
                      </Box>
                    </ConfCard>
                  </Grid>

                  {/* ④ 日志（高级，默认折叠） */}
                  <Grid item xs={12}>
                    <Paper variant="outlined" sx={{ px: 1.75, py: 0.75 }}>
                      <Button
                        fullWidth
                        size="small"
                        color="inherit"
                        onClick={() => setAdvOpen((o) => !o)}
                        sx={{
                          justifyContent: "space-between",
                          textTransform: "none",
                          color: "text.secondary",
                          px: 0.5,
                        }}
                        endIcon={
                          <ExpandMoreIcon
                            sx={{
                              transform: advOpen ? "rotate(180deg)" : "none",
                              transition: "transform 0.2s",
                            }}
                          />
                        }
                      >
                        高级 · 日志路径（一般不用改）
                      </Button>
                      <Collapse in={advOpen}>
                        <Stack direction={{ xs: "column", sm: "row" }} spacing={1.5} sx={{ pb: 1.25, pt: 0.5 }}>
                          <TextField
                            size="small"
                            label="访问日志（access_log）"
                            value={cfg?.accessLog ?? ""}
                            onChange={set("accessLog")}
                            fullWidth
                            InputProps={{ sx: { fontFamily: MONO_STACK, fontSize: 13 } }}
                          />
                          <TextField
                            size="small"
                            label="错误日志（error_log）"
                            value={cfg?.errorLog ?? ""}
                            onChange={set("errorLog")}
                            fullWidth
                            InputProps={{ sx: { fontFamily: MONO_STACK, fontSize: 13 } }}
                          />
                        </Stack>
                      </Collapse>
                    </Paper>
                  </Grid>
                </Grid>

                {/* 保存栏：状态随 dirty / 校验结果 / 运行状态联动 */}
                <Paper
                  elevation={0}
                  sx={{
                    display: "flex",
                    alignItems: "center",
                    gap: 1.5,
                    px: 2,
                    py: 1.25,
                    border: "1px dashed",
                    borderColor: hasErrors ? "error.main" : dirty ? "warning.main" : "divider",
                    borderRadius: 1.5,
                  }}
                >
                  <DescriptionIcon
                    fontSize="small"
                    color={hasErrors ? "error" : dirty ? "warning" : "action"}
                  />
                  <Typography variant="caption" color="text.secondary" sx={{ flex: 1 }}>
                    {hasErrors
                      ? "存在无效配置项（见红框提示），修正后才能保存"
                      : running === false
                        ? "Nginx 未运行：保存写入 nginx.conf，下次启动时生效"
                        : "保存即写入 nginx.conf 并自动热加载（nginx 运行中立即生效）"}
                  </Typography>
                  {dirty && !hasErrors && (
                    <Chip
                      size="small"
                      color="warning"
                      variant="outlined"
                      label="有未保存更改"
                      sx={{ height: 20 }}
                    />
                  )}
                  <Button
                    variant="outlined"
                    onClick={reset}
                    disabled={!dirty || saving}
                    startIcon={<UndoIcon />}
                  >
                    重置
                  </Button>
                  <Button
                    variant="contained"
                    onClick={save}
                    disabled={saving || !dirty || hasErrors}
                    startIcon={saving ? <CircularProgress size={14} color="inherit" /> : undefined}
                  >
                    {saving ? "保存中…" : "保存配置"}
                  </Button>
                </Paper>
              </>
            )}
          </Stack>
        )}
      </CardContent>
    </Card>
  );
}
