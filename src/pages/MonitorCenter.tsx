import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { MONO_STACK } from "../theme";
import {
  Box,
  Button,
  Chip,
  CircularProgress,
  Divider,
  MenuItem,
  Paper,
  Select,
  Stack,
  Switch,
  ToggleButton,
  ToggleButtonGroup,
  Typography,
  FormControlLabel,
} from "@mui/material";
import Pause from "@mui/icons-material/Pause";
import PlayArrow from "@mui/icons-material/PlayArrow";
import Refresh from "@mui/icons-material/Refresh";
import Description from "@mui/icons-material/Description";
import FolderOpen from "@mui/icons-material/FolderOpen";
import { api } from "../api";
import { PageHeader } from "../components/PageHeader";

/**
 * 监控中心：统一管理所有服务的运行状况与日志。
 * 左列 = 服务 × 日志文件清单（运行状态点 + 大小/最后修改，修改时间倒序——
 * 「日志还在不在动」一眼可辨，停更通常意味着进程卡死或退出）；
 * 右侧 = 选中文件的日志终端（可选自动跟随刷新，默认 3s）。
 * 单文件读取走 /api/services/:id/logs?path=（后端按 registry 日志声明白名单校验）。
 */

type LogFileMeta = { path: string; name: string; size: number; modified: number };
type LogFileService = {
  id: string;
  name: string;
  running: boolean;
  files: LogFileMeta[];
};

const REFRESH_OPTIONS = [0, 3, 5, 10];

const fmtSize = (n: number) =>
  n >= 1048576 ? `${(n / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} KB`;

const fmtTime = (unix: number) =>
  unix > 0
    ? new Date(unix * 1000).toLocaleTimeString("zh-CN", { hour12: false })
    : "—";

/** 文件是否「活跃」：最近 60s 内有写入 */
const isActive = (modified: number) => Date.now() / 1000 - modified < 60;

export default function MonitorCenter() {
  const [services, setServices] = useState<LogFileService[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  // 选中：{服务id, 文件path}。默认选第一个有日志的服务最新文件（多数场景就是 dfy-server.log）
  const [sel, setSel] = useState<{ id: string; path: string } | null>(null);
  // 日志终端状态
  const [lines, setLines] = useState<string[]>([]);
  const [live, setLive] = useState(true);
  const [intervalSecs, setIntervalSecs] = useState(3);
  const [tailBusy, setTailBusy] = useState(false);
  const [lastFetch, setLastFetch] = useState<number | null>(null);
  const [autoScroll, setAutoScroll] = useState(true);
  const boxRef = useRef<HTMLDivElement | null>(null);
  const tickRef = useRef<number | null>(null);

  const fetchList = useCallback(async () => {
    try {
      const r = await api.logFiles();
      setServices(r.services);
      setError(null);
      // 首次加载：默认选中修改时间最新的文件
      setSel((cur) => {
        if (cur) return cur;
        const all = r.services.flatMap((s) =>
          s.files.map((f) => ({ id: s.id, path: f.path, modified: f.modified })),
        );
        const newest = all.sort((a, b) => b.modified - a.modified)[0];
        return newest ? { id: newest.id, path: newest.path } : null;
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void fetchList();
    const id = window.setInterval(() => void fetchList(), 10000); // 清单 10s 刷一次（状态点/大小/修改时间）
    return () => window.clearInterval(id);
  }, [fetchList]);

  const tail = useCallback(async () => {
    if (!sel) return;
    setTailBusy(true);
    try {
      const r = await api.logs(sel.id, sel.path);
      const text = (r as { logs?: string }).logs ?? "";
      setLines(text.length ? text.replace(/\r/g, "").split("\n") : []);
      setLastFetch(Date.now());
    } catch (e) {
      setLines([`读取失败：${e instanceof Error ? e.message : String(e)}`]);
    } finally {
      setTailBusy(false);
    }
  }, [sel]);

  // 选中变化立即拉一次
  useEffect(() => {
    setLines([]);
    if (sel) void tail();
  }, [sel, tail]);

  // 自动跟随轮询
  useEffect(() => {
    if (tickRef.current) window.clearInterval(tickRef.current);
    if (live && intervalSecs > 0 && sel) {
      tickRef.current = window.setInterval(() => void tail(), intervalSecs * 1000);
    }
    return () => {
      if (tickRef.current) window.clearInterval(tickRef.current);
    };
  }, [live, intervalSecs, sel, tail]);

  // 自动滚动到最新行
  useEffect(() => {
    if (autoScroll && boxRef.current) {
      boxRef.current.scrollTop = boxRef.current.scrollHeight;
    }
  }, [lines, autoScroll]);

  // 当前选中文件的元信息（活跃灯/大小/时间显示）
  const selMeta = useMemo(() => {
    for (const s of services) {
      const f = s.files.find((x) => x.path === sel?.path);
      if (f) return { service: s, file: f };
    }
    return null;
  }, [services, sel]);

  const runningCount = services.filter((s) => s.running).length;
  const fileCount = services.reduce((n, s) => n + s.files.length, 0);

  const downloadLog = () => {
    if (!selMeta) return;
    const blob = new Blob([lines.join("\n")], { type: "text/plain;charset=utf-8" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = selMeta.file.name;
    a.click();
    URL.revokeObjectURL(a.href);
  };

  const openLogDir = async () => {
    if (!sel) return;
    await api.openConfigDir(sel.id, "logs").catch(() => {});
  };

  return (
    <Box sx={{ p: 2.5, maxWidth: 1400, mx: "auto" }}>
      <PageHeader
        title="监控中心"
        description="统一查看各服务运行状况与运行日志：列表按最后修改时间排序，活跃（60 秒内有写入）的文件带绿灯；选中文件可自动跟随刷新。"
      />

      {/* 总览条 */}
      <Stack direction="row" spacing={1.5} sx={{ mt: 2, mb: 1.5, flexWrap: "wrap" }}>
        <Chip size="small" color="success" label={`运行中 ${runningCount}/${services.length}`} />
        <Chip size="small" variant="outlined" label={`日志文件 ${fileCount} 个`} />
        {selMeta && (
          <Chip
            size="small"
            variant="outlined"
            label={`正在查看：${selMeta.service.name} / ${selMeta.file.name}`}
            onDelete={() => setSel(null)}
          />
        )}
      </Stack>

      {loading ? (
        <Stack alignItems="center" sx={{ py: 8 }}>
          <CircularProgress size={28} />
          <Typography variant="body2" color="text.secondary" sx={{ mt: 1.5 }}>
            正在收集服务与日志清单…
          </Typography>
        </Stack>
      ) : error ? (
        <Paper variant="outlined" sx={{ p: 3 }}>
          <Typography color="error">加载失败：{error}</Typography>
        </Paper>
      ) : (
        <Stack direction="row" spacing={2} alignItems="stretch">
          {/* 左列：服务 × 文件清单 */}
          <Paper
            variant="outlined"
            sx={{ width: 340, flexShrink: 0, overflowY: "auto", maxHeight: "calc(100vh - 250px)" }}
          >
            {services.map((s, i) => (
              <Box key={s.id}>
                {i > 0 && <Divider />}
                <Stack
                  direction="row"
                  spacing={1}
                  alignItems="center"
                  sx={{ px: 1.5, py: 1, bgcolor: "action.hover" }}
                >
                  <Box
                    sx={{
                      width: 8,
                      height: 8,
                      borderRadius: "50%",
                      bgcolor: s.running ? "success.main" : "grey.400",
                      flexShrink: 0,
                    }}
                  />
                  <Typography variant="body2" sx={{ fontWeight: 600 }}>
                    {s.name}
                  </Typography>
                  <Typography variant="caption" color="text.secondary" sx={{ ml: "auto" }}>
                    {s.running ? "运行中" : "已停止"}
                  </Typography>
                </Stack>
                {s.files.length === 0 ? (
                  <Typography variant="caption" color="text.disabled" sx={{ px: 2.5, py: 0.5, display: "block" }}>
                    （该服务未声明日志文件）
                  </Typography>
                ) : (
                  s.files.map((f) => {
                    const active = sel?.path === f.path;
                    return (
                      <Stack
                        key={f.path}
                        direction="row"
                        spacing={1}
                        alignItems="center"
                        onClick={() => setSel({ id: s.id, path: f.path })}
                        sx={{
                          px: 2.5,
                          py: 0.75,
                          cursor: "pointer",
                          bgcolor: active ? "rgba(22,58,94,0.08)" : "transparent",
                          borderLeft: 3,
                          borderColor: active ? "primary.main" : "transparent",
                          "&:hover": { bgcolor: "action.hover" },
                        }}
                      >
                        <Description sx={{ fontSize: 15, color: "text.secondary" }} />
                        <Box sx={{ minWidth: 0, flex: 1 }}>
                          <Typography variant="body2" noWrap>
                            {f.name}
                          </Typography>
                          <Typography variant="caption" color="text.secondary">
                            {fmtSize(f.size)} · {fmtTime(f.modified)}
                            {isActive(f.modified) && (
                              <Box
                                component="span"
                                sx={{ color: "success.main", fontWeight: 700, ml: 0.5 }}
                              >
                                ● 活跃
                              </Box>
                            )}
                          </Typography>
                        </Box>
                      </Stack>
                    );
                  })
                )}
              </Box>
            ))}
          </Paper>

          {/* 右侧：日志终端 */}
          <Paper variant="outlined" sx={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
            <Stack
              direction="row"
              spacing={1}
              alignItems="center"
              sx={{ px: 1.5, py: 1, borderBottom: "1px solid", borderColor: "divider", flexWrap: "wrap" }}
            >
              <Typography variant="subtitle2" sx={{ mr: "auto" }}>
                {selMeta ? `${selMeta.service.name} / ${selMeta.file.name}` : "未选择日志文件"}
              </Typography>
              {selMeta?.file && isActive(selMeta.file.modified) && (
                <Chip size="small" color="success" label="日志活跃" />
              )}
              <FormControlLabel
                control={<Switch size="small" checked={live} onChange={(e) => setLive(e.target.checked)} />}
                label={<Typography variant="caption">自动刷新</Typography>}
                sx={{ mr: 1 }}
              />
              <Select
                size="small"
                value={intervalSecs}
                onChange={(e) => setIntervalSecs(Number(e.target.value))}
                disabled={!live}
                sx={{ height: 30, fontSize: 12, width: 82 }}
              >
                {REFRESH_OPTIONS.filter((v) => v > 0).map((v) => (
                  <MenuItem key={v} value={v}>
                    {v} 秒
                  </MenuItem>
                ))}
              </Select>
              <Button size="small" startIcon={<Refresh />} onClick={() => void tail()} disabled={tailBusy || !sel}>
                刷新
              </Button>
              <Button size="small" onClick={downloadLog} disabled={!sel}>
                下载
              </Button>
              <Button size="small" startIcon={<FolderOpen />} onClick={() => void openLogDir()} disabled={!sel}>
                打开目录
              </Button>
            </Stack>
            <Box
              ref={boxRef}
              sx={{
                flex: 1,
                overflow: "auto",
                bgcolor: "grey.900",
                minHeight: 360,
                px: 1.5,
                py: 1,
                maxHeight: "calc(100vh - 320px)",
              }}
            >
              {!sel ? (
                <Typography variant="caption" sx={{ fontFamily: MONO_STACK, color: "grey.500" }}>
                  ← 从左侧选择要查看的日志文件
                </Typography>
              ) : lines.length === 0 ? (
                <Typography variant="caption" sx={{ fontFamily: MONO_STACK, color: "grey.500" }}>
                  {tailBusy ? "读取中…" : "（日志为空——服务可能从未启动过，或启动后尚未写入）"}
                </Typography>
              ) : (
                lines.map((l, i) => (
                  <Typography
                    key={i}
                    variant="caption"
                    component="div"
                    sx={{
                      fontFamily: MONO_STACK,
                      color: l.includes("ERROR") || l.includes("Exception")
                        ? "error.light"
                        : l.includes("WARN")
                          ? "warning.light"
                          : "grey.300",
                      whiteSpace: "pre-wrap",
                      wordBreak: "break-all",
                      lineHeight: 1.6,
                      opacity: autoScroll && i < lines.length - 1 ? 0.8 : 1,
                    }}
                  >
                    {l}
                  </Typography>
                ))
              )}
            </Box>
            <Stack
              direction="row"
              spacing={1}
              alignItems="center"
              sx={{ px: 1.5, py: 0.75, borderTop: "1px solid", borderColor: "divider" }}
            >
              <Typography variant="caption" color="text.secondary" sx={{ mr: "auto" }}>
                {lines.length} 行 · 尾部 200 行
                {lastFetch && ` · 更新于 ${new Date(lastFetch).toLocaleTimeString("zh-CN", { hour12: false })}`}
              </Typography>
              <ToggleButtonGroup
                size="small"
                exclusive
                value={autoScroll}
                onChange={(_, v) => v !== null && setAutoScroll(v)}
              >
                <ToggleButton value={true} sx={{ px: 1.5, py: 0.25, fontSize: 12 }}>
                  <PlayArrow sx={{ fontSize: 14, mr: 0.25 }} />
                  跟随
                </ToggleButton>
                <ToggleButton value={false} sx={{ px: 1.5, py: 0.25, fontSize: 12 }}>
                  <Pause sx={{ fontSize: 14, mr: 0.25 }} />
                  停留
                </ToggleButton>
              </ToggleButtonGroup>
            </Stack>
          </Paper>
        </Stack>
      )}
    </Box>
  );
}
