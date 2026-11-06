import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { MONO_STACK } from "../theme";
import {
  Box,
  Button,
  Chip,
  CircularProgress,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  Divider,
  MenuItem,
  Paper,
  Select,
  Snackbar,
  Stack,
  Switch,
  TextField,
  ToggleButton,
  ToggleButtonGroup,
  Tooltip,
  Typography,
  FormControlLabel,
} from "@mui/material";
import Alert from "@mui/material/Alert";
import Pause from "@mui/icons-material/Pause";
import PlayArrow from "@mui/icons-material/PlayArrow";
import Refresh from "@mui/icons-material/Refresh";
import Description from "@mui/icons-material/Description";
import FolderOpen from "@mui/icons-material/FolderOpen";
import BackupOutlined from "@mui/icons-material/BackupOutlined";
import DeleteOutline from "@mui/icons-material/DeleteOutline";
import { api } from "../api";
import { LOG_BG, LOG_TS_RE, ColoredLogLine, normalizeDate } from "../components/LogTerminal";
import { PageHeader } from "../components/PageHeader";

/**
 * 监控中心：统一管理「本地可收集日志」的服务（2026-11-04 用户定案）。
 * - 左列按服务分组：服务名 + 运行状态 + 组内日志文件（远程服务如 MinIO/VerneMQ
 *   自身日志无法收集，后端已过滤，不在这里运维）
 * - 右侧日志终端：选中文件的尾部 200 行，自动跟随刷新，ERROR/WARN 着色
 * - 日志维护：备份（.bak-时间戳）/ 删除（二次确认），路径白名单由后端校验
 */

type LogFileMeta = {
  path: string;
  name: string;
  size: number;
  modified: number;
  derived?: boolean;
};
type LogFileService = {
  id: string;
  name: string;
  running: boolean;
  archiveCount: number;
  archiveBytes: number;
  files: LogFileMeta[];
};

const REFRESH_OPTIONS = [3, 5, 10];

// ===== 模块级缓存（组件卸载不清空）=====
// 此前每次进监控中心都清空重拉：清单要等接口、日志要等 tail，来回切页一次卡一次。
// 现在切页返回瞬间呈现缓存内容，后台静默刷新覆盖——体感即时打开。
let listCache: LogFileService[] | null = null;
let selCache: { id: string; path: string } | null = null;
const linesCache = new Map<string, string[]>();
/** 日志内容缓存上限：每个文件 200 行，服务多时路径可达几十个，
 *  不设上限会随使用时长无限增长（长行日志尤其吃内存）。超出即淘汰最旧。 */
const LINES_CACHE_MAX = 8;

function cacheLines(path: string, lines: string[]) {
  linesCache.delete(path); // 重新插入到末尾：维持 LRU 顺序
  linesCache.set(path, lines);
  while (linesCache.size > LINES_CACHE_MAX) {
    const oldest = linesCache.keys().next().value;
    if (oldest === undefined) break;
    linesCache.delete(oldest);
  }
}

const fmtSize = (n: number) =>
  n >= 1048576 ? `${(n / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} KB`;

const fmtTime = (unix: number) =>
  unix > 0
    ? new Date(unix * 1000).toLocaleTimeString("zh-CN", { hour12: false })
    : "—";

/** 文件是否「活跃」：最近 60s 内有写入 */
const isActive = (modified: number) => Date.now() / 1000 - modified < 60;

/** 解析行首时间戳；无日期的输入自动补当天/默认日期比较用。
 *  LOG_TS_RE 与关键字着色统一在 components/LogTerminal（全站共享）。 */
function parseTs(line: string): Date | null {
  const m = LOG_TS_RE.exec(line);
  if (!m) return null;
  const d = new Date(`${normalizeDate(m[1])}T${m[2].replace(",", ".")}`);
  return isNaN(d.getTime()) ? null : d;
}

/** 时间输入 → 比较用 Date。支持完整 `yyyy-MM-dd HH:mm:ss`（斜杠日期也认）或只有 `HH:mm:ss`（补当天）。 */
function parseQuery(q: string, fallbackDay: string): Date | null {
  const t = q.trim().replace("T", " ");
  const full = /^\d{4}[-/]\d{2}[-/]\d{2} \d{2}:\d{2}:\d{2}/.test(t)
    ? normalizeDate(t).replace(" ", "T")
    : /^\d{2}:\d{2}(:\d{2})?$/.test(t)
      ? `${fallbackDay}T${t.length === 5 ? `${t}:00` : t}`
      : null;
  if (!full) return null;
  const d = new Date(full);
  return isNaN(d.getTime()) ? null : d;
}

/** 服务名拆层：「DFY 后端（Spring Boot）」→ 主名「DFY 后端」+ 副题「Spring Boot」。
 *  主名深色粗体、副题灰色小字——长服务名不再把左列行挤换行（用户定案：不换行）。 */
function splitServiceName(name: string): [string, string | null] {
  const m = name.match(/^(.+?)（(.+)）$/);
  return m ? [m[1], m[2]] : [name, null];
}

export default function MonitorCenter() {
  // 初始值直接吃模块级缓存：有缓存时跳过骨架屏，回来就是上次的画面
  const [services, setServices] = useState<LogFileService[]>(listCache ?? []);
  const [loading, setLoading] = useState(!listCache);
  const [error, setError] = useState<string | null>(null);
  const [sel, setSel] = useState<{ id: string; path: string } | null>(selCache);
  const [lines, setLines] = useState<string[]>([]);
  const [live, setLive] = useState(true);
  const [intervalSecs, setIntervalSecs] = useState(3);
  const [tailBusy, setTailBusy] = useState(false);
  const [lastFetch, setLastFetch] = useState<number | null>(null);
  const [autoScroll, setAutoScroll] = useState(true);
  // 日志维护
  const [confirmDel, setConfirmDel] = useState(false);
  // 删除失败（文件被占用）后的兜底：清空内容（文件保留、长度归零）
  const [confirmClear, setConfirmClear] = useState(false);
  // 备用日志管理弹窗：备份/归档衍生文件不占主列表，集中在这里看和删
  const [backupMgr, setBackupMgr] = useState<{ id: string; name: string; files: LogFileMeta[] } | null>(null);
  // 归档管理：批量清理滚动归档/备份衍生（keepDays=0 全清；主日志永不删）
  const [cleanup, setCleanup] = useState<{ id: string; name: string; count: number; bytes: number } | null>(null);
  const [cleanupKeep, setCleanupKeep] = useState(0);
  const [cleaning, setCleaning] = useState(false);
  const [snack, setSnack] = useState<{ open: boolean; msg: string; severity: "success" | "error" }>({
    open: false,
    msg: "",
    severity: "success",
  });
  // 时间搜索定位：输入时间片段（HH:mm:ss 或完整日期时间），定位首条 >= 该时间的行
  const [timeQuery, setTimeQuery] = useState("");
  const [jumpIndex, setJumpIndex] = useState<number | null>(null);
  const jumpRef = useRef<HTMLDivElement | null>(null);
  const boxRef = useRef<HTMLDivElement | null>(null);
  const tickRef = useRef<number | null>(null);

  const fetchList = useCallback(async () => {
    try {
      const r = await api.logFiles();
      listCache = r.services;
      setServices(r.services);
      setError(null);
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
    const id = window.setInterval(() => void fetchList(), 10000);
    return () => window.clearInterval(id);
  }, [fetchList]);

  const tail = useCallback(async () => {
    if (!sel) return;
    setTailBusy(true);
    try {
      const r = await api.logs(sel.id, sel.path);
      const text = (r as { logs?: string }).logs ?? "";
      const next = text.length ? text.replace(/\r/g, "").split("\n") : [];
      cacheLines(sel.path, next); // 存模块缓存：切页返回免重拉
      setLines(next);
      setLastFetch(Date.now());
      setJumpIndex(null); // 内容已刷新，旧的定位高亮行号失效
    } catch (e) {
      setLines([`读取失败：${e instanceof Error ? e.message : String(e)}`]);
    } finally {
      setTailBusy(false);
    }
  }, [sel]);

  useEffect(() => {
    selCache = sel; // 记住最后查看的文件，切页返回直接恢复
    if (!sel) return;
    // 有缓存先呈现缓存（不闪空白），tail 成功后静默覆盖
    const cached = linesCache.get(sel.path);
    setLines(cached ?? []);
    void tail();
  }, [sel, tail]);

  useEffect(() => {
    if (tickRef.current) window.clearInterval(tickRef.current);
    if (live && intervalSecs > 0 && sel) {
      tickRef.current = window.setInterval(() => void tail(), intervalSecs * 1000);
    }
    return () => {
      if (tickRef.current) window.clearInterval(tickRef.current);
    };
  }, [live, intervalSecs, sel, tail]);

  useEffect(() => {
    if (autoScroll && boxRef.current) {
      boxRef.current.scrollTop = boxRef.current.scrollHeight;
    }
  }, [lines, autoScroll]);

  // 定位行高亮后滚到可见
  useEffect(() => {
    if (jumpIndex !== null && jumpRef.current) {
      jumpRef.current.scrollIntoView({ block: "center" });
    }
  }, [jumpIndex]);

  /** 按时间定位：首条时间 >= 输入的日志行。无日期输入按日志里出现最多的日期补。 */
  const jumpToTime = () => {
    if (!lines.length) return;
    const dayCounts = new Map<string, number>();
    for (const l of lines) {
      const m = LOG_TS_RE.exec(l);
      if (m) {
        const day = normalizeDate(m[1]); // nginx 斜杠日期统一成横杠，避免同一日期分裂成两个 key
        dayCounts.set(day, (dayCounts.get(day) ?? 0) + 1);
      }
    }
    const fallbackDay =
      [...dayCounts.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ??
      new Date().toISOString().slice(0, 10);
    const target = parseQuery(timeQuery, fallbackDay);
    if (!target) {
      setSnack({
        open: true,
        msg: "时间格式：HH:mm:ss 或 yyyy-MM-dd HH:mm:ss",
        severity: "error",
      });
      return;
    }
    const idx = lines.findIndex((l) => {
      const t = parseTs(l);
      return t && t >= target;
    });
    if (idx < 0) {
      setSnack({ open: true, msg: "该时间之后没有日志（检查日期/时间是否正确）", severity: "error" });
      return;
    }
    setJumpIndex(idx);
  };

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
    // 传当前日志文件路径，后端打开其所在目录（任意服务通用）；
    // 失败必须提示出来——此前 .catch(() => {}) 静默吞错，用户点按钮像「没反应」
    try {
      const r = await api.openConfigDir(sel.id, undefined, sel.path);
      if (!r.ok) {
        setSnack({ open: true, msg: r.error ?? "打开目录失败", severity: "error" });
      }
    } catch (e) {
      setSnack({
        open: true,
        msg: "打开目录失败：" + (e instanceof Error ? e.message : String(e)),
        severity: "error",
      });
    }
  };

  const doBackup = async () => {
    if (!sel) return;
    try {
      const r = await api.backupLog(sel.id, sel.path);
      if (r.ok) {
        setSnack({ open: true, msg: `已备份为 ${(r.backup ?? "").split(/[\\/]/).pop()}`, severity: "success" });
        void fetchList();
      } else {
        setSnack({ open: true, msg: r.error ?? "备份失败", severity: "error" });
      }
    } catch (e) {
      setSnack({ open: true, msg: "备份失败：" + (e instanceof Error ? e.message : String(e)), severity: "error" });
    }
  };

  const doDelete = async () => {
    if (!sel) return;
    setConfirmDel(false);
    try {
      const r = await api.deleteLog(sel.id, sel.path);
      if (r.ok) {
        setSnack({
          open: true,
          msg: "已删除。正在写入的服务（logback/nginx）会自动重建日志文件",
          severity: "success",
        });
        setLines([]);
        void fetchList();
      } else {
        setSnack({ open: true, msg: r.error ?? "删除失败", severity: "error" });
        // 文件被占用删不掉 → 弹「清空内容」兜底：文件保留、长度归零，服务继续写
        if (r.error && (r.error.includes("占用") || r.error.includes("锁定") || r.error.includes("busy"))) {
          setConfirmClear(true);
        }
      }
    } catch (e) {
      setSnack({ open: true, msg: "删除失败：" + (e instanceof Error ? e.message : String(e)), severity: "error" });
    }
  };

  // 清空当前日志文件内容（删除被占用时的兜底；文件本体保留，服务无需重建）
  const doClear = async () => {
    if (!sel) return;
    setConfirmClear(false);
    try {
      const r = await api.clearLog(sel.id, sel.path);
      if (r.ok) {
        setSnack({
          open: true,
          msg: `已清空文件内容，释放 ${fmtSize(r.freed ?? 0)}。文件保留，服务继续写入不受影响`,
          severity: "success",
        });
        setLines([]);
        void tail();
      } else {
        setSnack({ open: true, msg: r.error ?? "清空失败", severity: "error" });
      }
    } catch (e) {
      setSnack({ open: true, msg: "清空失败：" + (e instanceof Error ? e.message : String(e)), severity: "error" });
    }
  };

  // 备用日志弹窗里的单文件删除（都是备份/归档衍生，低风险，直接删）
  const deleteBackupFile = async (id: string, path: string) => {
    try {
      const r = await api.deleteLog(id, path);
      if (r.ok) {
        setSnack({ open: true, msg: "已删除", severity: "success" });
        setBackupMgr((b) => (b ? { ...b, files: b.files.filter((f) => f.path !== path) } : b));
        void fetchList();
      } else {
        setSnack({ open: true, msg: r.error ?? "删除失败", severity: "error" });
      }
    } catch (e) {
      setSnack({ open: true, msg: "删除失败：" + (e instanceof Error ? e.message : String(e)), severity: "error" });
    }
  };

  // 归档管理：按保留天数批量清理该服务的滚动归档/备份衍生（keepDays=0 全清）
  const doCleanup = async () => {
    if (!cleanup) return;
    setCleaning(true);
    try {
      const r = await api.cleanupArchives(cleanup.id, cleanupKeep);
      if (r.ok) {
        setSnack({
          open: true,
          msg: `已清理 ${r.removed} 个归档，释放 ${fmtSize(r.bytes)}`,
          severity: "success",
        });
        setCleanup(null);
        void fetchList();
      } else {
        setSnack({ open: true, msg: "清理失败：" + (r.errors?.join("；") ?? "未知错误"), severity: "error" });
      }
    } catch (e) {
      setSnack({ open: true, msg: "清理失败：" + (e instanceof Error ? e.message : String(e)), severity: "error" });
    } finally {
      setCleaning(false);
    }
  };

  return (
    <Box sx={{ p: 2.5, maxWidth: 1680, mx: "auto" }}>
      <PageHeader
        title="监控中心"
        description="统一管理本地服务的运行日志：按服务分组，活跃（60 秒内有写入）的文件带绿灯；支持备份与删除。远程部署的服务（MinIO/VerneMQ/Redis 等）自身日志无法收集，不在此运维。"
      />

      <Stack direction="row" spacing={1.5} sx={{ mt: 2, mb: 1.5, flexWrap: "wrap" }}>
        <Chip size="small" color="success" label={`本地服务运行中 ${runningCount}/${services.length}`} />
        <Chip size="small" variant="outlined" label={`日志文件 ${fileCount} 个`} />
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
          {/* 左列：按服务分组的日志清单 */}
          <Paper
            variant="outlined"
            sx={{
              width: 400,
              flexShrink: 0,
              overflowY: "auto",
              maxHeight: "calc(100vh - 250px)",
              // 左列淡蓝灰底与右侧白底形成明暗层次（品牌蓝体系同源）
              bgcolor: "#fafbfd",
            }}
          >
            {services.map((s, i) => {
              // 主列表只放实时业务日志；备份/归档衍生收进「备用」弹窗，不占日常视线
              const mainFiles = s.files.filter((f) => !f.derived);
              const extraFiles = s.files.filter((f) => f.derived);
              // 服务名拆层：主名深色粗体 + 括号副题灰色小字，整行不再换行
              const [mainName, subName] = splitServiceName(s.name);
              return (
              <Box key={s.id}>
                {i > 0 && <Divider />}
                <Stack
                  spacing={0.25}
                  sx={{
                    px: 1.5,
                    py: 0.9,
                    // 组头品牌蓝浅底（#1e3f66 体系）：与左列淡灰底再叠一层，标题区层次分明
                    bgcolor: "rgba(30,63,102,0.07)",
                    borderBottom: "1px solid",
                    borderColor: "divider",
                    position: "sticky",
                    top: 0,
                    zIndex: 1,
                  }}
                >
                  <Stack direction="row" spacing={1} alignItems="center" sx={{ flexWrap: "nowrap" }}>
                    <Box
                      sx={{
                        width: 9,
                        height: 9,
                        borderRadius: "50%",
                        bgcolor: s.running ? "success.main" : "grey.400",
                        flexShrink: 0,
                      }}
                    />
                    <Typography
                      variant="subtitle2"
                      noWrap
                      sx={{ fontWeight: 700, color: "primary.main", flexShrink: 0 }}
                    >
                      {mainName}
                    </Typography>
                    {subName && (
                      <Typography
                        variant="caption"
                        noWrap
                        sx={{ color: "text.secondary", minWidth: 0 }}
                        title={subName}
                      >
                        {subName}
                      </Typography>
                    )}
                    <Chip
                      size="small"
                      variant="outlined"
                      label={s.running ? "运行中" : "已停止"}
                      color={s.running ? "success" : "default"}
                      sx={{ height: 20, fontSize: 11, ml: "auto", flexShrink: 0 }}
                    />
                  </Stack>
                  <Stack direction="row" spacing={0.75} alignItems="center" sx={{ pl: 2, flexWrap: "wrap", rowGap: 0.25 }}>
                    <Chip size="small" label={`${mainFiles.length} 个日志`} sx={{ height: 20, fontSize: 11 }} />
                    {extraFiles.length > 0 && (
                      <Tooltip title={`备份/归档日志 ${extraFiles.length} 个（${fmtSize(extraFiles.reduce((n, f) => n + f.size, 0))}），点开可查看与清理`}>
                        <Chip
                          size="small"
                          variant="outlined"
                          label={`备用 ${extraFiles.length}`}
                          sx={{ height: 20, fontSize: 11 }}
                          onClick={() => setBackupMgr({ id: s.id, name: s.name, files: extraFiles })}
                        />
                      </Tooltip>
                    )}
                  </Stack>
                </Stack>
                {mainFiles.length === 0 ? (
                  <Typography variant="caption" color="text.disabled" sx={{ px: 2.5, py: 0.5, display: "block" }}>
                    （该服务未声明日志文件）
                  </Typography>
                ) : (
                  mainFiles.map((f) => {
                    const active = sel?.path === f.path;
                    return (
                      <Stack
                        key={f.path}
                        direction="row"
                        spacing={1}
                        alignItems="center"
                        onClick={() => setSel({ id: s.id, path: f.path })}
                        sx={{
                          pl: 3,
                          pr: 1.5,
                          py: 0.75,
                          cursor: "pointer",
                          bgcolor: active ? "rgba(30,63,102,0.09)" : "transparent",
                          borderLeft: 3,
                          borderColor: active ? "primary.main" : "transparent",
                          "&:hover": { bgcolor: "rgba(30,63,102,0.05)" },
                        }}
                      >
                        <Description
                          sx={{ fontSize: 15, color: active ? "primary.main" : "text.secondary" }}
                        />
                        <Tooltip title={f.path} placement="right" arrow>
                          <Box sx={{ minWidth: 0, flex: 1 }}>
                            {/* 文件名用等宽字体：与界面中文区隔，运维数据感 */}
                            <Typography
                              variant="body2"
                              noWrap
                              sx={{ fontFamily: MONO_STACK, fontSize: 12.5, color: active ? "primary.main" : "text.primary" }}
                            >
                              {f.name}
                              {f.derived && (
                                <Box
                                  component="span"
                                  sx={{
                                    ml: 0.75,
                                    px: 0.5,
                                    borderRadius: 0.5,
                                    fontSize: 10,
                                    bgcolor: "action.selected",
                                    color: "text.secondary",
                                    verticalAlign: "middle",
                                  }}
                                >
                                  归档
                                </Box>
                              )}
                            </Typography>
                            <Typography variant="caption" color="text.secondary" noWrap display="block">
                              {fmtSize(f.size)} · {fmtTime(f.modified)}
                              {isActive(f.modified) && (
                                <Box component="span" sx={{ color: "success.main", fontWeight: 700, ml: 0.5 }}>
                                  ● 活跃
                                </Box>
                              )}
                            </Typography>
                          </Box>
                        </Tooltip>
                      </Stack>
                    );
                  })
                )}
              </Box>
              );
            })}
          </Paper>

          {/* 右侧：日志终端 + 维护操作 */}
          <Paper variant="outlined" sx={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
            <Stack
              spacing={0.75}
              // 布局稳定（2026-11-05 用户定案）：上下两层——第一行「服务名+活跃+文件名」
              // 占满整行，文件名独享剩余宽度基本不截断；第二行工具按钮，窄窗口时在
              // 按钮区内自行折行（rowGap 保持间距），标题与按钮互不挤压、位置稳定。
              sx={{ px: 1.5, py: 1, borderBottom: "1px solid", borderColor: "divider" }}
            >
              <Stack direction="row" spacing={1} alignItems="center">
                <Typography variant="subtitle1" sx={{ fontWeight: 700, lineHeight: 1.2, flexShrink: 0 }}>
                  {selMeta ? selMeta.service.name : "未选择日志文件"}
                </Typography>
                {selMeta && isActive(selMeta.file.modified) && (
                  <Chip size="small" color="success" label="日志活跃" sx={{ height: 18, fontSize: 10, flexShrink: 0 }} />
                )}
                <Tooltip title={selMeta?.file.path ?? ""}>
                  <Typography
                    variant="caption"
                    color="text.secondary"
                    sx={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
                  >
                    {selMeta ? selMeta.file.name : "从左侧选择要查看的日志"}
                  </Typography>
                </Tooltip>
              </Stack>
              <Stack direction="row" spacing={1} alignItems="center" sx={{ flexWrap: "wrap", rowGap: 0.75 }}>
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
                {REFRESH_OPTIONS.map((v) => (
                  <MenuItem key={v} value={v}>
                    {v} 秒
                  </MenuItem>
                ))}
              </Select>
              <Tooltip title="按时间定位日志行：输入 HH:mm:ss 或 yyyy-MM-dd HH:mm:ss，跳到该时间之后的第一条">
                <Stack direction="row" spacing={0.5} alignItems="center">
                  <TextField
                    size="small"
                    placeholder="10:23:45"
                    value={timeQuery}
                    onChange={(e) => setTimeQuery(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && jumpToTime()}
                    sx={{ width: 120, "& input": { fontFamily: MONO_STACK, fontSize: 12 } }}
                  />
                  <Button size="small" onClick={jumpToTime} disabled={!sel || !lines.length}>
                    定位
                  </Button>
                </Stack>
              </Tooltip>
              <Button size="small" startIcon={<Refresh />} onClick={() => void tail()} disabled={tailBusy || !sel}>
                刷新
              </Button>
              <Tooltip title="保存当前显示的尾部日志（最多 200 行）">
                <Button size="small" onClick={downloadLog} disabled={!sel}>
                  保存尾部
                </Button>
              </Tooltip>
              <Tooltip title="复制一份为 <原名>.bak-时间戳，留一份现场再清理">
                <Button size="small" startIcon={<BackupOutlined />} onClick={() => void doBackup()} disabled={!sel}>
                  备份
                </Button>
              </Tooltip>
              <Tooltip title="删除此日志文件；正在写入的服务会自动重建">
                <Button size="small" color="error" startIcon={<DeleteOutline />} onClick={() => setConfirmDel(true)} disabled={!sel}>
                  删除
                </Button>
              </Tooltip>
              <Button size="small" startIcon={<FolderOpen />} onClick={() => void openLogDir()} disabled={!sel}>
                打开目录
              </Button>
              </Stack>
            </Stack>
            <Box
              ref={boxRef}
              sx={{
                flex: 1,
                overflow: "auto",
                bgcolor: LOG_BG,
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
                  <ColoredLogLine
                    key={i}
                    line={l}
                    noWrap // 长行不换行：横向滚动，时间列始终对齐（换行是乱的根源）
                    highlight={i === jumpIndex}
                    onClick={() => setJumpIndex(i)}
                    innerRef={i === jumpIndex ? jumpRef : undefined}
                  />
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

      {/* 删除确认：日志删了就没了（除非先备份） */}
      <Dialog open={confirmDel} onClose={() => setConfirmDel(false)}>
        <DialogTitle>删除日志文件？</DialogTitle>
        <DialogContent>
          <DialogContentText component="div">
            <Typography variant="body2" sx={{ fontFamily: MONO_STACK, mb: 1 }}>
              {selMeta?.file.path}
            </Typography>
            <Typography variant="body2" color="text.secondary">
              删除后不可恢复。如需留存现场请先「备份」。正在写入此文件的服务（如 logback/nginx）会在下次写入时自动重建空文件，不影响运行。
            </Typography>
          </DialogContentText>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setConfirmDel(false)}>取消</Button>
          <Button onClick={() => void doDelete()} color="error" variant="contained">
            删除
          </Button>
        </DialogActions>
      </Dialog>

      {/* 归档清理确认：主日志永不删，只清滚动归档/备份衍生 */}
      <Dialog open={!!cleanup} onClose={() => setCleanup(null)}>
        <DialogTitle>清理归档日志？</DialogTitle>
        <DialogContent>
          <DialogContentText component="div">
            <Typography variant="body2" sx={{ mb: 1 }}>
              {cleanup?.name}：{cleanup?.count} 个归档文件，共 {fmtSize(cleanup?.bytes ?? 0)}
            </Typography>
            <Typography variant="body2" color="text.secondary" sx={{ mb: 1.5 }}>
              只清理滚动归档（.gz）与备份衍生（.prev / .bak），正在写入的主日志文件不会删除，服务运行不受影响。
            </Typography>
            <Select
              size="small"
              fullWidth
              value={cleanupKeep}
              onChange={(e) => setCleanupKeep(Number(e.target.value))}
            >
              <MenuItem value={0}>全部清理</MenuItem>
              <MenuItem value={7}>只清理 7 天前的</MenuItem>
              <MenuItem value={30}>只清理 30 天前的</MenuItem>
            </Select>
          </DialogContentText>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setCleanup(null)}>取消</Button>
          <Button onClick={() => void doCleanup()} color="error" variant="contained" disabled={cleaning}>
            {cleaning ? "清理中…" : "清理"}
          </Button>
        </DialogActions>
      </Dialog>

      {/* 清空内容兜底：文件被占用删不掉时，保留文件只清内容，服务继续写 */}
      <Dialog open={confirmClear} onClose={() => setConfirmClear(false)}>
        <DialogTitle>文件被占用，无法删除</DialogTitle>
        <DialogContent>
          <DialogContentText component="div">
            <Typography variant="body2" sx={{ mb: 1 }}>
              {selMeta?.file.name ?? "该日志"} 正在被服务进程写入，删除会失败。
            </Typography>
            <Typography variant="body2" color="text.secondary">
              可以改为「清空内容」：文件保留、长度归零，立即释放磁盘空间，服务继续写入不受影响。
            </Typography>
          </DialogContentText>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setConfirmClear(false)}>取消</Button>
          <Button onClick={() => void doClear()} color="warning" variant="contained">
            清空内容
          </Button>
        </DialogActions>
      </Dialog>

      {/* 备用日志管理：备份/归档衍生集中在此查看与删除，不占主列表 */}
      <Dialog open={!!backupMgr} onClose={() => setBackupMgr(null)} maxWidth="sm" fullWidth>
        <DialogTitle>
          {backupMgr?.name} · 备用日志（{backupMgr?.files.length ?? 0}）
        </DialogTitle>
        <DialogContent dividers>
          <DialogContentText component="div">
            <Typography variant="body2" color="text.secondary" sx={{ mb: 1.5 }}>
              备份（.bak-时间戳）与滚动归档（.gz）集中在这里管理；正在写入的业务日志在左侧主列表。
            </Typography>
            {backupMgr?.files.length === 0 && (
              <Typography variant="body2" color="text.disabled" sx={{ py: 2, textAlign: "center" }}>
                已全部清理
              </Typography>
            )}
            {backupMgr?.files.map((f) => (
              <Stack
                key={f.path}
                direction="row"
                spacing={1}
                alignItems="center"
                sx={{ py: 0.75, borderBottom: "1px solid", borderColor: "divider" }}
              >
                <Description sx={{ fontSize: 16, color: "text.secondary" }} />
                <Box sx={{ minWidth: 0, flex: 1 }}>
                  <Tooltip title={f.path} placement="top" arrow>
                    <Typography variant="body2" noWrap>
                      {f.name}
                    </Typography>
                  </Tooltip>
                  <Typography variant="caption" color="text.secondary">
                    {fmtSize(f.size)}
                  </Typography>
                </Box>
                <Button size="small" color="error" onClick={() => void deleteBackupFile(backupMgr.id, f.path)}>
                  删除
                </Button>
              </Stack>
            ))}
          </DialogContentText>
        </DialogContent>
        <DialogActions>
          <Button
            onClick={() => {
              if (!backupMgr) return;
              const bytes = backupMgr.files.reduce((n, f) => n + f.size, 0);
              setCleanup({ id: backupMgr.id, name: backupMgr.name, count: backupMgr.files.length, bytes });
              setBackupMgr(null);
            }}
            disabled={!backupMgr?.files.length}
          >
            按保留天数批量清理…
          </Button>
          <Button onClick={() => setBackupMgr(null)}>关闭</Button>
        </DialogActions>
      </Dialog>

      <Snackbar
        open={snack.open}
        autoHideDuration={5000}
        onClose={() => setSnack((s) => ({ ...s, open: false }))}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
      >
        <Alert severity={snack.severity} variant="filled" sx={{ minWidth: 300 }}>
          {snack.msg}
        </Alert>
      </Snackbar>
    </Box>
  );
}
