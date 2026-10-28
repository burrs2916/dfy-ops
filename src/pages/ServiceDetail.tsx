import { useCallback, useEffect, useRef, useState } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { MONO_STACK } from "../theme";
import {
  Alert,
  Box,
  Button,
  Card,
  CardContent,
  Chip,
  CircularProgress,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  IconButton,
  Snackbar,
  Stack,
  Tab,
  Tabs,
  TextField,
  Typography,
} from "@mui/material";
import { ArrowBack, Build, Close, ContentCopy, DnsOutlined, Download, PlayArrow, Settings } from "@mui/icons-material";
import { api } from "../api";
import type { LogsResponse, ServiceEnvReport, ServiceStatus } from "../types";
import { StatusBadge } from "../components/StatusBadge";
import ExternalConfigPanel from "../components/ExternalConfigPanel";
import NginxConfigPanel from "../components/NginxConfigPanel";

/** 单行配置展示：标签 + 等宽值 + 复制按钮。纯展示，不做任何系统变更。 */
function ConfigRow({
  label,
  value,
  onCopy,
}: {
  label: string;
  value: string;
  onCopy: (text: string) => void;
}) {
  return (
    <Stack direction="row" spacing={1} alignItems="center">
      {label && (
        <Typography
          variant="body2"
          sx={{ color: "text.secondary", minWidth: 72, flexShrink: 0 }}
        >
          {label}
        </Typography>
      )}
      <Box
        component="code"
        sx={{
          flex: 1,
          fontFamily: MONO_STACK,
          fontSize: 12,
          bgcolor: "rgba(0,0,0,0.04)",
          p: "4px 8px",
          borderRadius: 1,
          whiteSpace: "pre-wrap",
          wordBreak: "break-all",
        }}
      >
        {value}
      </Box>
      <IconButton
        size="small"
        title="复制"
        onClick={() => onCopy(value)}
      >
        <ContentCopy fontSize="small" sx={{ color: "text.secondary" }} />
      </IconButton>
    </Stack>
  );
}

export default function ServiceDetail() {
  const { id = "" } = useParams();
  const navigate = useNavigate();
  const [svc, setSvc] = useState<ServiceStatus | null>(null);
  const [logs, setLogs] = useState("");
  const [busy, setBusy] = useState(false);
  const [env, setEnv] = useState<ServiceEnvReport | null>(null);
  const [envBusy, setEnvBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [snack, setSnack] = useState<{ open: boolean; msg: string; severity: "success" | "error" | "info" }>({
    open: false,
    msg: "",
    severity: "info",
  });
  const [logFilter, setLogFilter] = useState("");
  const logRef = useRef<HTMLDivElement>(null);
  const [buildOpen, setBuildOpen] = useState(false);
  const [buildLogs, setBuildLogs] = useState("");
  const [buildDone, setBuildDone] = useState(false);
  const [tab, setTab] = useState(0);

  // 日志更新后自动滚动到底部（跟随最新输出）。
  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [logs]);

  const downloadLogs = useCallback(() => {
    const blob = new Blob([logs], { type: "text/plain;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${id}-logs.txt`;
    a.click();
    URL.revokeObjectURL(url);
  }, [id, logs]);

  const copyText = useCallback((text: string) => {
    navigator.clipboard
      ?.writeText(text)
      .then(() => setSnack({ open: true, msg: "已复制", severity: "success" }))
      .catch(() => setSnack({ open: true, msg: "复制失败", severity: "error" }));
  }, []);

  const load = useCallback(async () => {
    try {
      const s = await api.getService(id);
      setSvc(s);
      setErr(null);
    } catch (e) {
      setErr((e as Error).message);
    }
  }, [id]);

  const loadLogs = useCallback(async () => {
    try {
      const l: LogsResponse = await api.logs(id);
      setLogs(l.logs);
    } catch {
      /* 忽略日志轮询错误 */
    }
  }, [id]);

  const loadEnv = useCallback(async () => {
    try {
      const r = await api.envCheck(id);
      setEnv(r);
    } catch {
      /* 环境检查依赖后端，失败静默忽略，不影响服务详情展示 */
    }
  }, [id]);

  useEffect(() => {
    load();
    loadLogs();
    loadEnv();
    const t = setInterval(() => {
      load();
      loadLogs();
    }, 5000);
    return () => clearInterval(t);
  }, [load, loadLogs, loadEnv]);

  // 构建弹窗打开时，每 1.5s 轮询一次构建日志；检测到结束标记后停止轮询。
  useEffect(() => {
    if (!buildOpen) return;
    let active = true;
    let timer: ReturnType<typeof setInterval> | undefined;
    const tick = async () => {
      try {
        const r = await api.buildLog(id);
        if (!active) return;
        const logs = r.logs || "（暂无构建输出）";
        setBuildLogs(logs);
        if (logs.includes("==== 构建结束")) {
          setBuildDone(true);
          if (timer) clearInterval(timer);
        }
      } catch {
        /* 忽略轮询错误 */
      }
    };
    tick();
    timer = setInterval(tick, 1500);
    return () => {
      active = false;
      if (timer) clearInterval(timer);
    };
  }, [buildOpen, id]);

  const startBuild = async () => {
    setBuildOpen(true);
    setBuildDone(false);
    setBuildLogs("（已触发构建，正在拉取日志…）");
    try {
      const r = await api.build(id);
      if (!r.ok) {
        setSnack({ open: true, msg: `构建触发失败：${r.error ?? "未知错误"}`, severity: "error" });
      }
    } catch (e) {
      setSnack({ open: true, msg: "构建请求失败：" + (e as Error).message, severity: "error" });
    }
  };

  const downloadBuild = useCallback(() => {
    const blob = new Blob([buildLogs], { type: "text/plain;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${id}-build.log`;
    a.click();
    URL.revokeObjectURL(url);
  }, [id, buildLogs]);

  const act = async (fn: () => Promise<{ ok: boolean; error?: string }>, label: string) => {
    setBusy(true);
    try {
      const r = await fn();
      if (!r.ok) {
        setSnack({ open: true, msg: `${label}失败：${r.error ?? "未知错误"}`, severity: "error" });
      } else {
        setSnack({ open: true, msg: `${label}成功`, severity: "success" });
      }
      await load();
    } finally {
      setBusy(false);
    }
  };

  if (!svc) {
    return (
      <Box sx={{ display: "flex", justifyContent: "center", mt: 8 }}>
        <CircularProgress />
      </Box>
    );
  }

  const checkEnv = async () => {
    setEnvBusy(true);
    try {
      const r = await api.envCheck(id);
      setEnv(r);
    } finally {
      setEnvBusy(false);
    }
  };

  // 致命缺失（fail）意味着该服务无法运行/构建，需阻断启动与构建动作。
  const fatalMissing = env?.items.some((it) => it.status === "fail") ?? false;
  const warnOnly = env?.overall === "warn" && !fatalMissing;

  return (
    <Stack spacing={2}>
      {/* 顶部：标题 + 状态徽章 + 关键操作（任何 Tab 都能看到） */}
      <Stack direction="row" alignItems="center" spacing={1}>
        <Button startIcon={<ArrowBack />} onClick={() => navigate("/")}>
          返回
        </Button>
        <Typography variant="h5" sx={{ fontWeight: 700 }}>
          {svc.name}
        </Typography>
        <StatusBadge
          running={svc.running}
          healthy={svc.healthy}
          runState={svc.runState}
          installed={svc.installed}
        />
        {!svc.isDaemon && <Chip size="small" label="桌面客户端" color="info" />}
      </Stack>

      {err && <Alert severity="error">{err}</Alert>}

      {/* 环境致命/警告横条（任何 Tab 都显示） */}
      {fatalMissing && (
        <Alert severity="error">
          运行环境存在致命缺失，无法启动 / 构建。请先按下方「环境与日志 → 运行环境检查」中的修复建议处理。
        </Alert>
      )}
      {warnOnly && !fatalMissing && (
        <Alert severity="warning">
          运行环境存在非致命警告，可尝试启动但部分功能可能受限；建议先处理。
        </Alert>
      )}

      {/* Tab 分段 */}
      <Box sx={{ borderBottom: 1, borderColor: "divider" }}>
        <Tabs value={tab} onChange={(_, v) => setTab(v)}>
          <Tab label="运行控制" icon={<PlayArrow fontSize="small" />} iconPosition="start" />
          <Tab label="配置运维" icon={<Settings fontSize="small" />} iconPosition="start" />
          <Tab label="环境与日志" icon={<Build fontSize="small" />} iconPosition="start" />
        </Tabs>
      </Box>

      {tab === 0 && (
        <Card variant="outlined">
          <CardContent>
            <Typography variant="body2" color="text.secondary">
              {svc.description}
            </Typography>
            <Stack direction="row" spacing={1} sx={{ mt: 1.5 }} flexWrap="wrap" useFlexGap>
              {svc.ports.length > 0 && (
                <Chip size="small" label={`端口 ${svc.ports.join(", ")}`} />
              )}
              {svc.dependsOn.length > 0 && (
                <Chip size="small" label={`依赖 ${svc.dependsOn.join(", ")}`} />
              )}
            </Stack>
            {svc.thirdParty ? (
              <Alert severity="info" sx={{ mt: 2 }}>
                {svc.remoteOnly
                  ? "远程部署依赖：由虚拟机/远程主机承载，平台只检测远程端点可达性，不提供启停与构建。"
                  : "第三方依赖：由 Windows 服务管理器 / 容器运行时管理，平台只检测运行状态，不提供启停与构建。"}
              </Alert>
            ) : (
              <Stack direction="row" spacing={1.5} sx={{ mt: 2.5 }} flexWrap="wrap">
                <Button
                  variant="contained"
                  disabled={busy || fatalMissing}
                  onClick={() => act(() => api.start(svc.id), "启动")}
                >
                  启动
                </Button>
                <Button
                  variant="outlined"
                  disabled={busy}
                  onClick={() => act(() => api.stop(svc.id), "停止")}
                >
                  停止
                </Button>
                <Button
                  variant="outlined"
                  disabled={busy || fatalMissing}
                  onClick={() => act(() => api.restart(svc.id), "重启")}
                >
                  重启
                </Button>
                <Button
                  variant="text"
                  disabled={busy || fatalMissing}
                  onClick={startBuild}
                >
                  构建
                </Button>
              </Stack>
            )}
          </CardContent>
        </Card>
      )}

      {tab === 1 && (
        <Stack spacing={2}>
          <Card variant="outlined">
            <CardContent>
              <Typography variant="subtitle2" sx={{ mb: 1 }}>
                服务配置 · 预期配置
              </Typography>
              <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 1.5 }}>
                以下为该服务在目标 Windows 主机上的预期安装位置与运行参数，仅作排障参考。
              </Typography>
              <Stack spacing={1}>
                {svc.remoteOnly ? (
                  <Alert severity="info" icon={<DnsOutlined fontSize="inherit" />}>
                    纯远程部署服务：本机无安装目录与运行命令，检测目标为「运维中心 → 远程部署」配置的远程端点。
                  </Alert>
                ) : (
                  <>
                    <ConfigRow label="安装目录" value={svc.installDir} onCopy={copyText} />
                    <ConfigRow label="启动命令" value={svc.runCmd} onCopy={copyText} />
                    {svc.buildCmd && <ConfigRow label="构建命令" value={svc.buildCmd} onCopy={copyText} />}
                  </>
                )}
                {svc.healthUrl && (
                  <ConfigRow label="健康检查" value={svc.healthUrl} onCopy={copyText} />
                )}
                {svc.logPaths.length > 0 && (
                  <Box>
                    <Typography variant="body2" sx={{ color: "text.secondary", mb: 0.5 }}>
                      日志路径
                    </Typography>
                    <Stack spacing={0.5}>
                      {svc.logPaths.map((p) => (
                        <ConfigRow key={p} label="" value={p} onCopy={copyText} />
                      ))}
                    </Stack>
                  </Box>
                )}
              </Stack>
            </CardContent>
          </Card>

          {/* dfy-backend: Java 外部化配置运维 */}
          {id === "dfy-backend" && <ExternalConfigPanel serviceId={id} />}

          {/* nginx: 反向代理可视化配置运维 */}
          {id === "nginx" && <NginxConfigPanel serviceId={id} />}

          {/* 其他服务：提示暂无外部化配置面板 */}
          {id !== "dfy-backend" && id !== "nginx" && (
            <Alert severity="info">
              此服务暂无外部化配置面板。安装目录下的配置文件请手动编辑。
            </Alert>
          )}
        </Stack>
      )}

      {tab === 2 && (
        <Stack spacing={2}>
          {/* 运行环境检查 */}
          <Card variant="outlined">
            <CardContent>
              <Stack direction="row" justifyContent="space-between" alignItems="center">
                <Typography variant="subtitle2">运行环境检查</Typography>
                <Button size="small" variant="outlined" disabled={envBusy} onClick={checkEnv}>
                  {envBusy ? "检查中…" : "重新检查"}
                </Button>
              </Stack>
              {env && (
                <Stack spacing={0.75} sx={{ mt: 1 }}>
                  {env.items.map((it) => (
                    <Box key={it.name}>
                      <Stack direction="row" spacing={1} alignItems="center">
                        <Chip
                          size="small"
                          label={it.status === "pass" ? "满足" : it.status === "warn" ? "警告" : "缺失"}
                          color={
                            it.status === "pass"
                              ? "success"
                              : it.status === "warn"
                              ? "warning"
                              : "error"
                          }
                        />
                        <Typography variant="body2">{it.name}</Typography>
                        <Typography variant="caption" color="text.secondary" sx={{ wordBreak: "break-all" }}>
                          — {it.actual}
                        </Typography>
                      </Stack>
                      {it.status !== "pass" && it.fixHint && (
                        <Stack direction="row" spacing={0.5} alignItems="flex-start" sx={{ mt: 0.25, mb: 0.5, ml: 0.5 }}>
                          <Box
                            component="code"
                            sx={{
                              flex: 1,
                              fontFamily: MONO_STACK,
                              fontSize: 11,
                              bgcolor: "rgba(0,0,0,0.04)",
                              p: "3px 6px",
                              borderRadius: 1,
                              whiteSpace: "pre-wrap",
                              wordBreak: "break-all",
                            }}
                          >
                            {it.fixHint}
                          </Box>
                          <IconButton
                            size="small"
                            title="复制修复命令"
                            onClick={() => navigator.clipboard?.writeText(it.fixHint)}
                          >
                            <ContentCopy fontSize="small" sx={{ color: "text.secondary" }} />
                          </IconButton>
                        </Stack>
                      )}
                    </Box>
                  ))}
                </Stack>
              )}
            </CardContent>
          </Card>

          {/* 实时日志 */}
          <Card variant="outlined">
            <CardContent>
              <Stack direction="row" justifyContent="space-between" alignItems="center" spacing={1} sx={{ mb: 1 }}>
                <Typography variant="subtitle2">日志（每 5 秒刷新）</Typography>
                <Stack direction="row" spacing={1} alignItems="center">
                  <TextField
                    size="small"
                    placeholder="过滤关键字"
                    value={logFilter}
                    onChange={(e) => setLogFilter(e.target.value)}
                    sx={{ width: 160 }}
                  />
                  <Button
                    size="small"
                    variant="outlined"
                    startIcon={<Download />}
                    disabled={!logs}
                    onClick={downloadLogs}
                  >
                    下载
                  </Button>
                </Stack>
              </Stack>
              <Box
                ref={logRef}
                component="pre"
                sx={{
                  m: 0,
                  mt: 1,
                  p: 1.5,
                  bgcolor: "#1e1e1e",
                  color: "#d4d4d4",
                  borderRadius: 1,
                  fontSize: 12,
                  maxHeight: 360,
                  overflow: "auto",
                  whiteSpace: "pre-wrap",
                  fontFamily: MONO_STACK,
                }}
              >
                {logs
                  ? logFilter
                    ? logs
                        .split("\n")
                        .filter((l) => l.toLowerCase().includes(logFilter.toLowerCase()))
                        .join("\n") || "（无匹配行）"
                    : logs
                  : "（暂无日志或日志路径未配置）"}
              </Box>
            </CardContent>
          </Card>
        </Stack>
      )}

      <Snackbar
        open={snack.open}
        autoHideDuration={3500}
        onClose={() => setSnack((s) => ({ ...s, open: false }))}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
      >
        <Alert severity={snack.severity} variant="filled" onClose={() => setSnack((s) => ({ ...s, open: false }))}>
          {snack.msg}
        </Alert>
      </Snackbar>

      <Dialog open={buildOpen} onClose={() => setBuildOpen(false)} maxWidth="md" fullWidth>
        <DialogTitle sx={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          构建日志 - {svc?.name}
          <IconButton size="small" onClick={() => setBuildOpen(false)} aria-label="关闭">
            <Close fontSize="small" />
          </IconButton>
        </DialogTitle>
        <DialogContent dividers>
          <Stack direction="row" spacing={1} alignItems="center" sx={{ mb: 1 }}>
            {!buildDone && <Chip size="small" color="info" label="构建中…" />}
            {buildDone && buildLogs.includes("exit code 0") && (
              <Chip size="small" color="success" label="构建成功" />
            )}
            {buildDone && !buildLogs.includes("exit code 0") && (
              <Chip size="small" color="error" label="构建失败" />
            )}
          </Stack>
          <Box
            component="pre"
            sx={{
              m: 0,
              p: 1.5,
              bgcolor: "#1e1e1e",
              color: "#d4d4d4",
              borderRadius: 1,
              fontSize: 12,
              maxHeight: 420,
              overflow: "auto",
              whiteSpace: "pre-wrap",
              fontFamily: MONO_STACK,
            }}
          >
            {buildLogs || "（暂无构建输出）"}
          </Box>
        </DialogContent>
        <DialogActions>
          <Button startIcon={<Download />} disabled={!buildLogs} onClick={downloadBuild}>
            下载
          </Button>
          <Button onClick={() => setBuildOpen(false)}>关闭</Button>
        </DialogActions>
      </Dialog>
    </Stack>
  );
}
