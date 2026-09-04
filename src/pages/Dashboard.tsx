import { useCallback, useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  Alert,
  Box,
  Button,
  Chip,
  CircularProgress,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Divider,
  Grid,
  IconButton,
  Paper,
  Snackbar,
  Stack,
  Typography,
} from "@mui/material";
import { Close, ContentCopy, Download, Pause, PlayArrow, Refresh, ScienceOutlined } from "@mui/icons-material";
import { useOpsStore, useAppSettingsStore } from "../store";
import { ServiceCard } from "../components/ServiceCard";
import ReadinessCard from "../components/ReadinessCard";
import { api } from "../api";
import type { DiagnosisResponse, ServiceEnvReport } from "../types";

export default function Dashboard() {
  const { services, loading, error, refresh } = useOpsStore();
  const statusRefreshMs = useAppSettingsStore((s) => s.settings.statusRefreshSecs) * 1000;
  const envRefreshMs = useAppSettingsStore((s) => s.settings.envRefreshSecs) * 1000;
  const navigate = useNavigate();
  const [envMap, setEnvMap] = useState<Record<string, ServiceEnvReport>>({});
  const [snack, setSnack] = useState<{ open: boolean; msg: string; severity: "success" | "error" | "info" }>({
    open: false,
    msg: "",
    severity: "info",
  });
  const [diag, setDiag] = useState<DiagnosisResponse | null>(null);
  const [diagnosing, setDiagnosing] = useState(false);
  const [bulkBusy, setBulkBusy] = useState<null | "start" | "stop">(null);
  const [confirmStop, setConfirmStop] = useState(false);
  const [live, setLive] = useState(true);
  const [lastUpdate, setLastUpdate] = useState<Date | null>(null);

  const refreshEnv = useCallback(async () => {
    try {
      const all = await api.envCheckAll();
      const m: Record<string, ServiceEnvReport> = {};
      all.forEach((r) => (m[r.id] = r));
      setEnvMap(m);
    } catch {
      /* 环境检查依赖后端，失败时静默忽略，不影响服务列表 */
    }
  }, []);

  // 手动刷新：状态 + 环境 + 时间戳。
  const manualRefresh = useCallback(async () => {
    await refresh();
    await refreshEnv();
    setLastUpdate(new Date());
  }, [refresh, refreshEnv]);

  // 环境检查仅在挂载时拉取一次（依赖不常变，避免每 5s 重复探测 java/nginx 等子进程）。
  useEffect(() => {
    refreshEnv();
  }, [refreshEnv]);

  // 实时刷新：状态按「设置 → 通用」的状态间隔轮询；环境检查按独立的环境间隔轮询
  //（修复：此前写死"每 3 个状态周期复探一次"，envRefreshSecs 设置实际无效）。
  useEffect(() => {
    if (!live) return;
    const id = window.setInterval(async () => {
      await refresh();
      setLastUpdate(new Date());
    }, statusRefreshMs);
    return () => window.clearInterval(id);
  }, [live, refresh, statusRefreshMs]);

  useEffect(() => {
    if (!live) return;
    const id = window.setInterval(async () => {
      await refreshEnv();
    }, envRefreshMs);
    return () => window.clearInterval(id);
  }, [live, refreshEnv, envRefreshMs]);

  const diagnosis = async () => {
    setDiagnosing(true);
    try {
      const d = await api.diagnosis();
      setDiag(d);
    } catch (e) {
      setSnack({ open: true, msg: "诊断请求失败：" + (e as Error).message, severity: "error" });
    } finally {
      setDiagnosing(false);
    }
  };

  const copyText = (text: string) => {
    navigator.clipboard?.writeText(text).catch(() => {});
  };

  const exportDiagnosis = () => {
    if (!diag) return;
    const ts = new Date().toLocaleString("zh-CN");
    const lines: string[] = [
      "# DFY 运维平台 - 诊断报告",
      "",
      `生成时间：${ts}`,
      `服务总数：${services.length}`,
      `发现项：${diag.findings.length}`,
      "",
      "## 环境检查",
    ];
    if (diag.envs.length === 0) {
      lines.push("（无环境检查数据）");
    } else {
      diag.envs.forEach((e) =>
        lines.push(`- ${e.name}（${e.id}）：总体 ${e.overall}，缺失项 ${e.items.filter((i) => i.status !== "pass").length}`)
      );
    }
    lines.push("", "## 发现项");
    if (diag.findings.length === 0) {
      lines.push("未发现明显异常。");
    } else {
      diag.findings.forEach((f) => {
        lines.push(`- [${f.severity}] ${f.service}：${f.message}`);
        if (f.fixHint) lines.push(`  修复命令：${f.fixHint}`);
      });
    }
    lines.push("");
    if (diag.llmSummary) {
      lines.push("## AI 分析", "", diag.llmSummary);
    } else if (diag.llmNote) {
      lines.push(`（${diag.llmNote}）`);
    }
    const blob = new Blob([lines.join("\n")], { type: "text/markdown;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `dfy-ops-diagnosis-${Date.now()}.md`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const summarize = (r: { ok: boolean; id: string; error?: string | null }[]) =>
    `成功 ${r.filter((x) => x.ok).length}/${r.length}：` +
    r.map((x) => `${x.ok ? "✓" : "✗"} ${x.id}${x.error ? " " + x.error : ""}`).join("，");

  const startAll = async () => {
    setBulkBusy("start");
    try {
      const r = await api.startAll();
      setSnack({ open: true, msg: "全部启动 " + summarize(r), severity: "success" });
      refresh();
    } catch (e) {
      setSnack({ open: true, msg: "启动失败：" + (e as Error).message, severity: "error" });
    } finally {
      setBulkBusy(null);
    }
  };

  const stopAll = async () => {
    setBulkBusy("stop");
    setConfirmStop(false);
    try {
      const r = await api.stopAll();
      setSnack({ open: true, msg: "全部停止 " + summarize(r), severity: "success" });
      refresh();
    } catch (e) {
      setSnack({ open: true, msg: "停止失败：" + (e as Error).message, severity: "error" });
    } finally {
      setBulkBusy(null);
    }
  };

  useEffect(() => {
    refresh();
  }, [refresh]);

  if (loading && services.length === 0) {
    return (
      <Box sx={{ display: "flex", justifyContent: "center", mt: 8 }}>
        <CircularProgress />
      </Box>
    );
  }

  const healthyCount = services.filter((s) => s.healthy).length;

  return (
    <Stack spacing={3}>
      <Box>
        <Stack direction="row" justifyContent="space-between" alignItems="center">
          <Box>
            <Typography variant="h5" sx={{ fontWeight: 700 }}>
              服务总览
            </Typography>
          </Box>
          <Stack direction="row" spacing={1} alignItems="center">
            <Button variant="outlined" startIcon={<Refresh />} onClick={manualRefresh}>
              刷新
            </Button>
            <Button
              variant="outlined"
              color={live ? "success" : "inherit"}
              startIcon={live ? <Pause /> : <PlayArrow />}
              onClick={() => setLive((v) => !v)}
            >
              {live ? "实时中" : "已暂停"}
            </Button>
            <Button
              variant="outlined"
              onClick={startAll}
              disabled={bulkBusy !== null}
              startIcon={bulkBusy === "start" ? <CircularProgress size={14} color="inherit" /> : undefined}
            >
              全部启动
            </Button>
            <Button
              variant="outlined"
              color="warning"
              onClick={() => setConfirmStop(true)}
              disabled={bulkBusy !== null}
            >
              全部停止
            </Button>
            <Button
              variant="outlined"
              startIcon={<ScienceOutlined />}
              onClick={() => navigate("/env")}
            >
              环境检查
            </Button>
            <Button
              variant="contained"
              onClick={diagnosis}
              disabled={diagnosing}
              startIcon={diagnosing ? <CircularProgress size={14} color="inherit" /> : undefined}
            >
              一键诊断
            </Button>
          </Stack>
        </Stack>
        <Typography variant="caption" color="text.secondary" sx={{ mt: 0.5, display: "block" }}>
          共 {services.length} 个服务，{healthyCount} 个健康 · 实时刷新{live ? "中" : "已暂停"} · 上次更新{" "}
          {lastUpdate ? lastUpdate.toLocaleTimeString("zh-CN") : "—"}
        </Typography>
      </Box>

      <ReadinessCard services={services} envMap={envMap} />

      {error && <Alert severity="error">{error}（请确认运维后端已在 127.0.0.1:18721 运行）</Alert>}

      {Object.values(envMap).some((r) => r.overall !== "pass") && (
        <Alert severity="warning">
          部分服务的运行环境存在缺失，启动 / 构建可能失败。请进入「环境检查」按修复建议在运维中心自行补齐依赖。
        </Alert>
      )}

      <Grid container spacing={2}>
        {services.map((s) => (
          <Grid item xs={12} sm={6} md={4} key={s.id}>
            <ServiceCard s={s} env={envMap[s.id]} />
          </Grid>
        ))}
      </Grid>

      <Paper variant="outlined" sx={{ p: 2 }}>
        <Typography variant="subtitle2" gutterBottom>
          说明
        </Typography>
        <Typography variant="body2" color="text.secondary">
          本平台为运维控制台，统一管理 DFY 系列异构服务的构建、启停、健康检查与日志。
          未包含任何远程运维（终端/SSH/SFTP/远程桌面）功能。
        </Typography>
      </Paper>

      <Snackbar
        open={snack.open}
        autoHideDuration={4000}
        onClose={() => setSnack((s) => ({ ...s, open: false }))}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
      >
        <Alert
          severity={snack.severity}
          variant="filled"
          onClose={() => setSnack((s) => ({ ...s, open: false }))}
        >
          {snack.msg}
        </Alert>
      </Snackbar>

      <Dialog open={confirmStop} onClose={() => setConfirmStop(false)} maxWidth="xs" fullWidth>
        <DialogTitle>全部停止</DialogTitle>
        <DialogContent>
          <Typography variant="body2">
            确定停止所有服务？将按依赖逆序依次停止，进行中的请求会中断。
          </Typography>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setConfirmStop(false)}>取消</Button>
          <Button variant="contained" color="warning" onClick={stopAll} disabled={bulkBusy !== null}>
            全部停止
          </Button>
        </DialogActions>
      </Dialog>

      <Dialog open={diag !== null} onClose={() => setDiag(null)} maxWidth="sm" fullWidth>
        <DialogTitle sx={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          诊断结果
          <IconButton size="small" onClick={() => setDiag(null)} aria-label="关闭">
            <Close fontSize="small" />
          </IconButton>
        </DialogTitle>
        <DialogContent dividers>
          {diag && (
            <Stack spacing={1.5}>
              {diag.envs.length > 0 && (
                <Box>
                  <Typography variant="overline" color="text.secondary">
                    环境概览
                  </Typography>
                  <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
                    {diag.envs.map((e) => (
                      <Chip
                        key={e.id}
                        size="small"
                        variant="outlined"
                        label={`${e.name}：${
                          e.overall === "pass" ? "满足" : e.overall === "warn" ? "警告" : "缺失"
                        }`}
                        color={
                          e.overall === "pass" ? "success" : e.overall === "warn" ? "warning" : "error"
                        }
                      />
                    ))}
                  </Stack>
                </Box>
              )}
              {diag.findings.length === 0 ? (
                <Alert severity="success">未发现明显异常。</Alert>
              ) : (
                diag.findings.map((f, i) => (
                  <Alert
                    key={i}
                    severity={f.severity === "critical" ? "error" : f.severity === "warning" ? "warning" : "info"}
                    icon={false}
                  >
                    <Stack direction="row" spacing={1} alignItems="center">
                      <Chip size="small" label={f.severity} />
                      <Typography variant="subtitle2">{f.service}</Typography>
                    </Stack>
                    <Typography variant="body2" sx={{ mt: 0.5 }}>
                      {f.message}
                    </Typography>
                    {f.fixHint && (
                      <Box
                        sx={{
                          mt: 1,
                          display: "flex",
                          alignItems: "center",
                          gap: 1,
                          bgcolor: "action.hover",
                          borderRadius: 1,
                          px: 1,
                          py: 0.5,
                        }}
                      >
                        <Typography
                          component="code"
                          variant="caption"
                          sx={{ flexGrow: 1, fontFamily: "monospace", wordBreak: "break-all" }}
                        >
                          {f.fixHint}
                        </Typography>
                        <IconButton size="small" onClick={() => copyText(f.fixHint as string)} aria-label="复制修复命令">
                          <ContentCopy fontSize="small" />
                        </IconButton>
                      </Box>
                    )}
                  </Alert>
                ))
              )}
              {diag.llmSummary && (
                <Box sx={{ mt: 1 }}>
                  <Divider sx={{ mb: 1 }} />
                  <Typography variant="subtitle2" gutterBottom>
                    AI 分析
                  </Typography>
                  <Typography variant="body2" sx={{ whiteSpace: "pre-wrap" }}>
                    {diag.llmSummary}
                  </Typography>
                </Box>
              )}
              {!diag.llmSummary && diag.llmNote && (
                <Typography variant="caption" color="text.secondary">
                  {diag.llmNote}
                </Typography>
              )}
            </Stack>
          )}
        </DialogContent>
        <DialogActions>
          <Button startIcon={<Download />} onClick={exportDiagnosis} disabled={!diag}>
            导出报告
          </Button>
          <Button onClick={() => setDiag(null)}>关闭</Button>
        </DialogActions>
      </Dialog>
    </Stack>
  );
}
