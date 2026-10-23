import { useCallback, useEffect, useRef, useState } from "react";
import { alpha } from "@mui/material/styles";
import {
  Alert,
  Box,
  Button,
  Chip,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  Grid,
  InputAdornment,
  IconButton,
  LinearProgress,
  MenuItem,
  Paper,
  Snackbar,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  TextField,
  Tooltip,
  Typography,
} from "@mui/material";
import RefreshIcon from "@mui/icons-material/Refresh";
import SettingsIcon from "@mui/icons-material/Settings";
import KeyIcon from "@mui/icons-material/Key";
import StorageIcon from "@mui/icons-material/Storage";
import MemoryIcon from "@mui/icons-material/Memory";
import GroupIcon from "@mui/icons-material/Group";
import ScheduleIcon from "@mui/icons-material/Schedule";
import SearchIcon from "@mui/icons-material/Search";
import DeleteOutlineIcon from "@mui/icons-material/DeleteOutline";
import TimerIcon from "@mui/icons-material/Timer";
import DataObjectIcon from "@mui/icons-material/DataObject";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import CheckCircleRoundedIcon from "@mui/icons-material/CheckCircleRounded";
import ErrorOutlineRoundedIcon from "@mui/icons-material/ErrorOutlineRounded";
import VpnKeyOutlinedIcon from "@mui/icons-material/VpnKeyOutlined";
import ChevronLeftRoundedIcon from "@mui/icons-material/ChevronLeftRounded";
import ChevronRightRoundedIcon from "@mui/icons-material/ChevronRightRounded";
import ContentCopyRoundedIcon from "@mui/icons-material/ContentCopyRounded";
import RadarIcon from "@mui/icons-material/Radar";
import CircularProgress from "@mui/material/CircularProgress";
import { api } from "../api";
import { MONO_STACK } from "../theme";
import { ServiceDiscoveryDialog } from "../components/ServiceDiscoveryDialog";
import type { RedisKeyDetail, RedisKeyItem, RedisOverview } from "../types";

const btnSx = { textTransform: "none" as const, whiteSpace: "nowrap" as const };

/** 类型徽章文案（short 为列表/详情里的紧凑标记，统一主题色） */
const KIND_META: Record<string, { label: string; short: string }> = {
  string: { label: "字符串", short: "STR" },
  hash: { label: "哈希", short: "HASH" },
  list: { label: "列表", short: "LIST" },
  set: { label: "集合", short: "SET" },
  zset: { label: "有序集合", short: "ZSET" },
  stream: { label: "流", short: "STREAM" },
  none: { label: "不存在", short: "NONE" },
};

/** 类型过滤下拉（SCAN 的 TYPE 参数）：选择即重扫 */
const KIND_FILTERS = [
  { value: "", label: "全部类型" },
  { value: "string", label: "字符串" },
  { value: "hash", label: "哈希" },
  { value: "list", label: "列表" },
  { value: "set", label: "集合" },
  { value: "zset", label: "有序集合" },
  { value: "stream", label: "流" },
];

/** Key 列表本地分页大小（后端一次 SCAN 全量计数，前端分页展示） */
const PAGE_SIZE = 100;

function formatBytes(n: number | null): string {
  if (n === null || Number.isNaN(n)) return "—";
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

function fmtTtl(ttl: number): string {
  if (ttl === -1) return "永久";
  if (ttl === -2) return "已不存在";
  if (ttl < 0) return "未知";
  if (ttl < 60) return `${ttl} 秒`;
  if (ttl < 3600) return `${Math.floor(ttl / 60)} 分 ${ttl % 60} 秒`;
  if (ttl < 86400) return `${Math.floor(ttl / 3600)} 时 ${Math.floor((ttl % 3600) / 60)} 分`;
  return `${Math.floor(ttl / 86400)} 天 ${Math.floor((ttl % 86400) / 3600)} 时`;
}

/** 类型徽章：紧凑等宽小标记，统一主题青蓝 */
function KindBadge({ kind }: { kind: string }) {
  const meta = KIND_META[kind] ?? { label: kind, short: kind.slice(0, 4).toUpperCase() };
  return (
    <Tooltip title={meta.label} enterDelay={400}>
      <Box
        sx={{
          px: 0.75,
          py: "1px",
          borderRadius: 0.75,
          bgcolor: (t) => alpha(t.palette.primary.main, 0.1),
          color: "primary.main",
          fontSize: 10,
          fontWeight: 700,
          fontFamily: MONO_STACK,
          letterSpacing: 0.3,
          lineHeight: 1.6,
          flexShrink: 0,
        }}
      >
        {meta.short}
      </Box>
    </Tooltip>
  );
}

/** 概览指标卡：主题色图标块 + 加粗大数值，悬停轻微浮起 */
function StatCard({ icon, label, value }: { icon: React.ReactNode; label: string; value: string }) {
  return (
    <Paper
      sx={{
        p: 1.5,
        display: "flex",
        alignItems: "center",
        gap: 1.25,
        minWidth: 0,
        transition: "transform .18s ease, box-shadow .18s ease",
        "&:hover": { transform: "translateY(-2px)", boxShadow: 4 },
      }}
    >
      <Box
        sx={{
          width: 38,
          height: 38,
          borderRadius: 1,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          bgcolor: (t) => alpha(t.palette.primary.main, 0.12),
          color: "primary.main",
          flexShrink: 0,
        }}
      >
        {icon}
      </Box>
      <Box sx={{ minWidth: 0 }}>
        <Typography variant="caption" color="text.secondary" noWrap sx={{ display: "block" }}>
          {label}
        </Typography>
        <Typography
          variant="h6"
          sx={{ fontWeight: 700, lineHeight: 1.25, fontVariantNumeric: "tabular-nums" }}
          noWrap
        >
          {value}
        </Typography>
      </Box>
    </Paper>
  );
}

/** 详情元信息（图标 + 文字的紧凑行，替代一排 Chip 减少视觉噪音） */
function MetaItem({ icon, text }: { icon: React.ReactNode; text: string }) {
  return (
    <Stack direction="row" spacing={0.5} alignItems="center">
      <Box sx={{ display: "flex", color: "primary.main" }}>{icon}</Box>
      <Typography variant="caption" sx={{ whiteSpace: "nowrap" }}>
        {text}
      </Typography>
    </Stack>
  );
}

export default function RedisOps({
  active,
  autoOpenConfig = false,
  onAutoOpenConsumed,
}: {
  active: boolean;
  /** 外部（修复中心问题项）请求落地后自动打开连接配置：检测不连通时的引导闭环 */
  autoOpenConfig?: boolean;
  onAutoOpenConsumed?: () => void;
}) {
  const [cred, setCred] = useState({ host: "…", port: 6379, password: "" });
  const [connected, setConnected] = useState(false);
  const [flash, setFlash] = useState<{ kind: "success" | "error"; msg: string } | null>(null);
  const flashTimer = useRef<number | null>(null);

  const [overview, setOverview] = useState<RedisOverview | null>(null);
  const [overviewErr, setOverviewErr] = useState("");

  const [pattern, setPattern] = useState("*");
  const [keyKind, setKeyKind] = useState("");
  const [keys, setKeys] = useState<RedisKeyItem[]>([]);
  const [keysTotal, setKeysTotal] = useState(0);
  const [keysTruncated, setKeysTruncated] = useState(false);
  const [keysLoading, setKeysLoading] = useState(false);
  const [page, setPage] = useState(0);

  const [selected, setSelected] = useState("");
  const [detail, setDetail] = useState<RedisKeyDetail | null>(null);
  const [detailErr, setDetailErr] = useState("");

  const [credOpen, setCredOpen] = useState(false);
  const [credForm, setCredForm] = useState({ host: "", port: 6379, password: "" });
  const [credDiscover, setCredDiscover] = useState(false);
  const [testing, setTesting] = useState(false);
  const [saving, setSaving] = useState(false);

  const [confirmTarget, setConfirmTarget] = useState<{ name: string } | null>(null);
  const [confirmText, setConfirmText] = useState("");
  const [deleting, setDeleting] = useState(false);

  const [ttlEdit, setTtlEdit] = useState<string>("");
  const [ttlMode, setTtlMode] = useState("expire");

  const showFlash = useCallback((kind: "success" | "error", msg: string) => {
    setFlash({ kind, msg });
    if (flashTimer.current) window.clearTimeout(flashTimer.current);
    flashTimer.current = window.setTimeout(() => setFlash(null), 6000);
  }, []);

  const loadOverview = useCallback(async () => {
    setOverviewErr("");
    try {
      const ov = await api.redisOverview();
      if (ov.error) {
        setOverview(null);
        setOverviewErr(ov.error);
        setConnected(false);
      } else {
        setOverview(ov);
        setConnected(true);
      }
    } catch (e) {
      setOverview(null);
      setOverviewErr(String(e));
      setConnected(false);
    }
  }, []);

  const loadKeys = useCallback(
    async (pat: string, kind: string) => {
      setKeysLoading(true);
      setPage(0);
      try {
        const r = await api.redisKeys(pat || "*", kind);
        if (r.error) {
          showFlash("error", r.error);
          setKeys([]);
          setKeysTotal(0);
        } else {
          setKeys(r.keys);
          setKeysTotal(r.total);
          setKeysTruncated(r.truncated);
        }
      } catch (e) {
        showFlash("error", String(e));
      } finally {
        setKeysLoading(false);
      }
    },
    [showFlash],
  );

  /** 复制到剪贴板（Key 名 / 值）——数据排查场景的高频操作 */
  const copyText = useCallback(
    async (text: string, label: string) => {
      try {
        await navigator.clipboard.writeText(text);
        showFlash("success", `${label}已复制到剪贴板`);
      } catch {
        showFlash("error", "复制失败");
      }
    },
    [showFlash],
  );

  const loadDetail = useCallback(async (name: string) => {
    setSelected(name);
    setDetail(null);
    setDetailErr("");
    try {
      setDetail(await api.redisKey(name));
    } catch (e) {
      setDetailErr(String(e));
    }
  }, []);

  const loadAll = useCallback(async () => {
    await Promise.all([loadOverview(), loadKeys(pattern, keyKind)]);
  }, [loadOverview, loadKeys, pattern, keyKind]);

  // 首次激活：拉凭据展示 + 全量加载（CSS 隐藏挂载，仅首次）
  const bootedRef = useRef(false);
  useEffect(() => {
    if (!active || bootedRef.current) return;
    bootedRef.current = true;
    (async () => {
      try {
        const c = await api.redisCredential();
        setCred(c);
        setCredForm(c);
      } catch {
        /* 凭据读取失败不阻塞页面，默认值兜底 */
      }
      void loadAll();
    })();
  }, [active, loadAll]);

  const openCredDialog = () => {
    setCredForm(cred);
    setCredOpen(true);
  };

  // 本地分页：后端一次 SCAN 全量计数，前端按页展示
  const pageStart = page * PAGE_SIZE;
  const pageItems = keys.slice(pageStart, pageStart + PAGE_SIZE);
  const pageCount = Math.max(1, Math.ceil(keys.length / PAGE_SIZE));

  // 外部引导（修复中心问题项「去连接配置」）：页面激活后自动弹出连接配置，一次性消费。
  // 自行拉取凭据填表——boot 的异步加载可能尚未完成。
  useEffect(() => {
    if (!active || !autoOpenConfig) return;
    onAutoOpenConsumed?.();
    (async () => {
      try {
        setCredForm(await api.redisCredential());
      } catch {
        /* 凭据读取失败保持当前表单值 */
      }
      setCredOpen(true);
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, autoOpenConfig]);

  const testConnection = async () => {
    setTesting(true);
    try {
      const r = await api.redisTest(credForm);
      if (r.ok) {
        showFlash("success", `连接成功${r.version ? `，Redis ${r.version}` : ""}`);
      } else {
        showFlash("error", r.error || "连接失败");
      }
    } catch (e) {
      showFlash("error", String(e));
    } finally {
      setTesting(false);
    }
  };

  const saveCred = async () => {
    setSaving(true);
    try {
      const r = await api.redisSaveCredential(credForm);
      if (r.ok) {
        setCred({ ...credForm });
        setCredOpen(false);
        showFlash("success", "连接配置已保存");
        void loadAll();
      } else {
        showFlash("error", r.error || "保存失败");
      }
    } catch (e) {
      showFlash("error", String(e));
    } finally {
      setSaving(false);
    }
  };

  const doDelete = async () => {
    if (!confirmTarget) return;
    setDeleting(true);
    try {
      const r = await api.redisDelete(confirmTarget.name);
      if (r.ok) {
        showFlash("success", `已删除 ${confirmTarget.name}`);
        setConfirmTarget(null);
        setConfirmText("");
        setSelected("");
        setDetail(null);
        void loadAll();
      } else {
        showFlash("error", r.error || "删除失败");
      }
    } catch (e) {
      showFlash("error", String(e));
    } finally {
      setDeleting(false);
    }
  };

  const applyTtl = async () => {
    if (!detail) return;
    const seconds = ttlMode === "persist" ? -1 : Number(ttlEdit);
    if (ttlMode === "expire" && (!Number.isInteger(seconds) || seconds <= 0)) {
      showFlash("error", "请输入正整数秒数");
      return;
    }
    try {
      const r = await api.redisSetTtl(detail.name, seconds);
      if (r.ok) {
        showFlash("success", r.message || "已更新");
        void loadDetail(detail.name);
      } else {
        showFlash("error", r.error || "设置失败");
      }
    } catch (e) {
      showFlash("error", String(e));
    }
  };

  const connChip = connected ? (
    <Chip
      size="small"
      color="primary"
      variant="filled"
      icon={<CheckCircleRoundedIcon />}
      label="已连接"
      sx={{ "& .MuiChip-icon": { color: "inherit" } }}
    />
  ) : overviewErr ? (
    <Chip
      size="small"
      color="error"
      variant="outlined"
      icon={<ErrorOutlineRoundedIcon />}
      label="连接失败"
      sx={{ "& .MuiChip-icon": { color: "error.main" } }}
    />
  ) : (
    <Chip size="small" variant="outlined" label="未连接" />
  );

  return (
    <Box sx={{ height: "100%", display: "flex", flexDirection: "column", gap: 1.5, overflow: "hidden" }}>
      {/* ===== 连接信息栏（固定） ===== */}
      <Paper sx={{ px: 1.5, py: 1, display: "flex", alignItems: "center", gap: 1.25, flexShrink: 0 }}>
        <Box sx={{ color: "primary.main", display: "flex" }}>
          <StorageIcon fontSize="small" />
        </Box>
        {connChip}
        <Tooltip title="平台托管的 Redis 连接端点（在连接配置中修改）">
          <Typography variant="body2" sx={{ fontFamily: MONO_STACK, color: "text.secondary" }}>
            {cred.host}:{cred.port}
          </Typography>
        </Tooltip>
        <Box sx={{ flex: 1 }} />
        <Button size="small" startIcon={<SettingsIcon />} onClick={openCredDialog} sx={btnSx}>
          连接配置
        </Button>
        <Button size="small" startIcon={<RefreshIcon />} onClick={() => void loadAll()} sx={btnSx}>
          刷新
        </Button>
      </Paper>

      {/* ===== 连接失败引导（与检测引导闭环：直接去连接配置） ===== */}
      {overviewErr && (
        <Alert
          severity="error"
          icon={<WarningAmberIcon fontSize="inherit" />}
          sx={{ flexShrink: 0, "& .MuiAlert-message": { alignItems: "center" } }}
          action={
            <Stack direction="row" spacing={1}>
              <Button color="inherit" size="small" variant="outlined" onClick={openCredDialog} sx={btnSx}>
                去连接配置
              </Button>
              <Button color="inherit" size="small" onClick={() => void loadOverview()} sx={btnSx}>
                重试
              </Button>
            </Stack>
          }
        >
          {overviewErr}
        </Alert>
      )}

      {/* ===== 总览指标（固定） ===== */}
      <Grid container spacing={1.5} sx={{ flexShrink: 0 }}>
        <Grid item xs={6} sm={4} md={2}>
          <StatCard icon={<StorageIcon fontSize="small" />} label="Redis 版本" value={overview?.version || "—"} />
        </Grid>
        <Grid item xs={6} sm={4} md={2}>
          <StatCard icon={<MemoryIcon fontSize="small" />} label="内存使用" value={overview?.usedMemory || "—"} />
        </Grid>
        <Grid item xs={6} sm={4} md={2}>
          <StatCard
            icon={<KeyIcon fontSize="small" />}
            label="Key 总数"
            value={overview ? String(overview.totalKeys) : "—"}
          />
        </Grid>
        <Grid item xs={6} sm={4} md={2}>
          <StatCard
            icon={<GroupIcon fontSize="small" />}
            label="客户端连接"
            value={overview?.connectedClients || "—"}
          />
        </Grid>
        <Grid item xs={6} sm={4} md={2}>
          <StatCard
            icon={<ScheduleIcon fontSize="small" />}
            label="运行天数"
            value={overview?.uptimeDays ? `${overview.uptimeDays} 天` : "—"}
          />
        </Grid>
        <Grid item xs={6} sm={4} md={2}>
          <StatCard
            icon={<TimerIcon fontSize="small" />}
            label="运行模式"
            value={overview?.mode === "cluster" ? "集群" : "单机"}
          />
        </Grid>
      </Grid>

      {/* ===== Key 工作台：占满剩余高度，左右面板独立滚动 ===== */}
      <Box sx={{ flex: 1, minHeight: 0, display: "flex", gap: 1.5 }}>
        {/* 左：Key 列表 */}
        <Paper sx={{ width: 340, flexShrink: 0, display: "flex", flexDirection: "column", overflow: "hidden" }}>
          <Box sx={{ p: 1.25, pb: 1, flexShrink: 0, display: "flex", gap: 0.75 }}>
            <TextField
              size="small"
              placeholder="按模式过滤，如 user:*"
              value={pattern}
              onChange={(e) => setPattern(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void loadKeys(pattern, keyKind);
              }}
              sx={{ flex: 1, minWidth: 0 }}
              InputProps={{
                startAdornment: (
                  <InputAdornment position="start">
                    <SearchIcon fontSize="small" sx={{ color: "text.secondary" }} />
                  </InputAdornment>
                ),
                endAdornment: (
                  <InputAdornment position="end">
                    <Tooltip title="扫描（回车）">
                      <IconButton
                        size="small"
                        edge="end"
                        disabled={keysLoading}
                        onClick={() => void loadKeys(pattern, keyKind)}
                      >
                        {keysLoading ? (
                          <CircularProgress size={13} />
                        ) : (
                          <RefreshIcon sx={{ fontSize: 16 }} />
                        )}
                      </IconButton>
                    </Tooltip>
                  </InputAdornment>
                ),
              }}
            />
            <TextField
              select
              size="small"
              value={keyKind}
              onChange={(e) => {
                setKeyKind(e.target.value);
                void loadKeys(pattern, e.target.value);
              }}
              sx={{ width: 106, flexShrink: 0 }}
            >
              {KIND_FILTERS.map((k) => (
                <MenuItem key={k.value} value={k.value}>
                  {k.label}
                </MenuItem>
              ))}
            </TextField>
          </Box>
          <LinearProgress
            sx={{ height: 2, visibility: keysLoading ? "visible" : "hidden", flexShrink: 0 }}
          />
          <Box sx={{ px: 1.5, py: 0.75, flexShrink: 0 }}>
            {keysTruncated ? (
              <Typography variant="caption" color="warning.main">
                共匹配 {keysTotal} 个，仅显示前 {keys.length} 个——请细化模式
              </Typography>
            ) : (
              <Typography variant="caption" color="text.secondary">
                匹配 {keysTotal} 个 Key
              </Typography>
            )}
          </Box>
          <Box sx={{ flex: 1, overflow: "auto", px: 1, pb: 1 }}>
            {pageItems.map((item) => (
              <Box
                key={item.name}
                onClick={() => void loadDetail(item.name)}
                title={item.ttl > 0 ? `${item.name} — ${fmtTtl(item.ttl)}后过期` : item.name}
                sx={{
                  px: 1,
                  py: 0.7,
                  mb: 0.25,
                  borderRadius: 1,
                  cursor: "pointer",
                  display: "flex",
                  alignItems: "center",
                  gap: 1,
                  bgcolor:
                    selected === item.name
                      ? (t) => alpha(t.palette.primary.main, 0.12)
                      : "transparent",
                  borderLeft: "3px solid",
                  borderColor: selected === item.name ? "primary.main" : "transparent",
                  transition: "background-color .15s ease",
                  "&:hover": { bgcolor: (t) => alpha(t.palette.primary.main, 0.06) },
                  overflow: "hidden",
                }}
              >
                <KindBadge kind={item.kind} />
                <Typography variant="body2" noWrap sx={{ flex: 1, minWidth: 0, fontFamily: MONO_STACK }}>
                  {item.name}
                </Typography>
                {item.ttl > 0 && (
                  <ScheduleIcon sx={{ fontSize: 13, color: "text.disabled", flexShrink: 0 }} />
                )}
              </Box>
            ))}
            {keys.length === 0 && !keysLoading && (
              <Typography variant="body2" color="text.secondary" sx={{ p: 2, textAlign: "center" }}>
                无匹配 Key——调整模式后重新扫描
              </Typography>
            )}
          </Box>
          {pageCount > 1 && (
            <Stack
              direction="row"
              alignItems="center"
              justifyContent="space-between"
              sx={{ px: 1.5, py: 0.5, borderTop: "1px solid", borderColor: "divider", flexShrink: 0 }}
            >
              <Typography variant="caption" color="text.secondary">
                第 {pageStart + 1}-{Math.min(pageStart + PAGE_SIZE, keys.length)} 个 / 共 {keys.length}
              </Typography>
              <Stack direction="row" alignItems="center">
                <IconButton size="small" disabled={page === 0} onClick={() => setPage((p) => p - 1)}>
                  <ChevronLeftRoundedIcon sx={{ fontSize: 18 }} />
                </IconButton>
                <Typography variant="caption" sx={{ px: 0.5, minWidth: 42, textAlign: "center" }}>
                  {page + 1} / {pageCount}
                </Typography>
                <IconButton
                  size="small"
                  disabled={page >= pageCount - 1}
                  onClick={() => setPage((p) => p + 1)}
                >
                  <ChevronRightRoundedIcon sx={{ fontSize: 18 }} />
                </IconButton>
              </Stack>
            </Stack>
          )}
        </Paper>

        {/* 右：Key 详情 */}
        <Paper sx={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", overflow: "hidden" }}>
          {!selected && (
            <Box
              sx={{
                flex: 1,
                display: "flex",
                flexDirection: "column",
                alignItems: "center",
                justifyContent: "center",
                gap: 1,
                color: "text.disabled",
              }}
            >
              <VpnKeyOutlinedIcon sx={{ fontSize: 44 }} />
              <Typography variant="body2" color="text.secondary">
                从左侧选择一个 Key 查看内容
              </Typography>
            </Box>
          )}
          {selected && detailErr && <Alert severity="error">{detailErr}</Alert>}
          {selected && !detail && !detailErr && (
            <Box
              sx={{
                flex: 1,
                display: "flex",
                flexDirection: "column",
                alignItems: "center",
                justifyContent: "center",
                gap: 1,
                color: "text.secondary",
              }}
            >
              <LinearProgress sx={{ width: 180 }} />
              <Typography variant="body2">加载中…</Typography>
            </Box>
          )}
          {selected && detail && (
            <>
              <Box sx={{ p: 1.5, pb: 1, flexShrink: 0, borderBottom: 1, borderColor: "divider" }}>
                {/* 标题行：key 名 + 类型徽章 + 删除 */}
                <Stack direction="row" spacing={1} alignItems="center">
                  <Typography
                    variant="subtitle1"
                    noWrap
                    title={detail.name}
                    sx={{ fontWeight: 700, fontFamily: MONO_STACK, flex: 1, minWidth: 0 }}
                  >
                    {detail.name}
                  </Typography>
                  <Tooltip title="复制 Key 名">
                    <IconButton size="small" onClick={() => void copyText(detail.name, "Key 名")}>
                      <ContentCopyRoundedIcon sx={{ fontSize: 15 }} />
                    </IconButton>
                  </Tooltip>
                  <KindBadge kind={detail.kind} />
                  <Button
                    size="small"
                    color="error"
                    startIcon={<DeleteOutlineIcon />}
                    sx={btnSx}
                    onClick={() => {
                      setConfirmTarget({ name: detail.name });
                      setConfirmText("");
                    }}
                  >
                    删除
                  </Button>
                </Stack>
                {/* 元信息行：TTL / 内存 / 条数 */}
                <Stack direction="row" spacing={2.5} alignItems="center" sx={{ mt: 1, flexWrap: "wrap" }}>
                  <MetaItem icon={<ScheduleIcon sx={{ fontSize: 15 }} />} text={`TTL ${fmtTtl(detail.ttl)}`} />
                  <MetaItem icon={<MemoryIcon sx={{ fontSize: 15 }} />} text={`占用 ${formatBytes(detail.memory)}`} />
                  <MetaItem icon={<DataObjectIcon sx={{ fontSize: 15 }} />} text={`共 ${detail.total} 条`} />
                  {detail.total > detail.rows.length && (
                    <Typography variant="caption" color="warning.main">
                      仅显示前 {detail.rows.length} 条
                    </Typography>
                  )}
                </Stack>
                {/* 过期设置（紧凑行内编辑） */}
                <Stack direction="row" spacing={1} alignItems="center" sx={{ mt: 1.25, flexWrap: "wrap" }}>
                  <Typography variant="caption" color="text.secondary">
                    过期设置
                  </Typography>
                  <TextField
                    select
                    size="small"
                    value={ttlMode}
                    onChange={(e) => setTtlMode(e.target.value)}
                    sx={{ width: 120 }}
                    SelectProps={{ MenuProps: { disableScrollLock: true } }}
                  >
                    <MenuItem value="expire">N 秒后过期</MenuItem>
                    <MenuItem value="persist">永久保留</MenuItem>
                  </TextField>
                  {ttlMode === "expire" && (
                    <TextField
                      size="small"
                      type="number"
                      value={ttlEdit}
                      onChange={(e) => setTtlEdit(e.target.value)}
                      sx={{ width: 130 }}
                      placeholder="秒数，如 3600"
                    />
                  )}
                  <Button size="small" variant="outlined" sx={btnSx} onClick={() => void applyTtl()}>
                    应用
                  </Button>
                </Stack>
              </Box>

              <TableContainer sx={{ flex: 1, minHeight: 0 }}>
                <Table size="small" stickyHeader>
                  <TableHead>
                    <TableRow>
                      <TableCell sx={{ width: 180, fontWeight: 700 }}>
                        {detail.kind === "zset" ? "成员" : "标签"}
                      </TableCell>
                      <TableCell sx={{ fontWeight: 700 }}>值</TableCell>
                    </TableRow>
                  </TableHead>
                  <TableBody>
                    {detail.rows.map(([k, v], i) => (
                      <TableRow key={i}>
                        <TableCell sx={{ fontFamily: MONO_STACK, wordBreak: "break-all" }}>
                          <Tooltip title={k} enterDelay={400}>
                            <span>{k}</span>
                          </Tooltip>
                        </TableCell>
                        <TableCell sx={{ padding: 0 }}>
                          <Box
                            sx={{
                              display: "flex",
                              alignItems: "flex-start",
                              gap: 0.5,
                              px: "14px",
                              py: "7px",
                              "& .copy-btn": { opacity: 0, transition: "opacity .15s ease" },
                              "&:hover .copy-btn": { opacity: 1 },
                            }}
                          >
                            <Typography
                              sx={{
                                flex: 1,
                                minWidth: 0,
                                fontFamily: MONO_STACK,
                                fontSize: "0.8125rem",
                                whiteSpace: "pre-wrap",
                                wordBreak: "break-all",
                              }}
                            >
                              {v || <span style={{ color: "#9aa3af" }}>（空）</span>}
                            </Typography>
                            <IconButton
                              className="copy-btn"
                              size="small"
                              title="复制值"
                              onClick={() => void copyText(v, "值")}
                            >
                              <ContentCopyRoundedIcon sx={{ fontSize: 14 }} />
                            </IconButton>
                          </Box>
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </TableContainer>
            </>
          )}
        </Paper>
      </Box>

      {/* ===== 连接配置 ===== */}
      <Dialog open={credOpen} onClose={() => setCredOpen(false)} maxWidth="sm" fullWidth>
        <DialogTitle sx={{ fontWeight: 700, display: "flex", alignItems: "center", gap: 1 }}>
          <SettingsIcon color="primary" />
          连接配置
        </DialogTitle>
        <DialogContent>
          <DialogContentText sx={{ mb: 2 }}>
            平台通过远程连接管理 Redis，连接凭据由平台托管。生产环境请修改为实际地址。
          </DialogContentText>
          <Stack spacing={2}>
            <TextField
              size="small"
              label="主机地址"
              value={credForm.host}
              onChange={(e) => setCredForm({ ...credForm, host: e.target.value })}
              placeholder="192.168.56.101"
              InputProps={{
                endAdornment: (
                  <Button
                    size="small"
                    variant="outlined"
                    startIcon={<RadarIcon />}
                    onClick={() => setCredDiscover(true)}
                    sx={{ textTransform: "none", whiteSpace: "nowrap", flexShrink: 0, mr: 0.5 }}
                  >
                    自动发现
                  </Button>
                ),
              }}
            />
            <TextField
              size="small"
              type="number"
              label="端口"
              value={credForm.port}
              onChange={(e) => setCredForm({ ...credForm, port: Number(e.target.value) || 6379 })}
            />
            <TextField
              size="small"
              type="password"
              label="密码（无密码可留空）"
              value={credForm.password}
              onChange={(e) => setCredForm({ ...credForm, password: e.target.value })}
              autoComplete="off"
            />
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setCredOpen(false)} sx={btnSx}>
            取消
          </Button>
          <Button variant="outlined" disabled={testing} onClick={() => void testConnection()} sx={btnSx}>
            {testing ? "测试中…" : "测试连接"}
          </Button>
          <Button variant="contained" disabled={saving} onClick={() => void saveCred()} sx={btnSx}>
            保存
          </Button>
        </DialogActions>
      </Dialog>

      {/* 连接配置的自动发现：只回填表单（密码等其余字段由用户确认后统一保存） */}
      <ServiceDiscoveryDialog
        open={credDiscover}
        onClose={() => setCredDiscover(false)}
        targets={["redis"]}
        fillOnly
        onAdopted={(host, port) => {
          setCredForm((f) => ({ ...f, host, port: port ?? f.port }));
          setCredDiscover(false);
        }}
      />

      {/* ===== 删除强确认 ===== */}
      <Dialog open={confirmTarget !== null} onClose={() => setConfirmTarget(null)} maxWidth="xs" fullWidth>
        <DialogTitle sx={{ fontWeight: 700, display: "flex", alignItems: "center", gap: 1 }}>
          <WarningAmberIcon color="warning" />
          删除 Key
        </DialogTitle>
        <DialogContent>
          <DialogContentText sx={{ whiteSpace: "pre-wrap" }}>
            {`即将删除：\n${confirmTarget?.name ?? ""}\n\n该操作立即生效且不可恢复。`}
          </DialogContentText>
          <TextField
            size="small"
            fullWidth
            sx={{ mt: 2 }}
            label='输入 "删除" 以确认'
            value={confirmText}
            onChange={(e) => setConfirmText(e.target.value)}
          />
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setConfirmTarget(null)} sx={btnSx}>
            取消
          </Button>
          <Button
            variant="contained"
            color="error"
            disabled={deleting || confirmText !== "删除"}
            onClick={() => void doDelete()}
            sx={btnSx}
          >
            {deleting ? "删除中…" : "确认删除"}
          </Button>
        </DialogActions>
      </Dialog>

      <Snackbar
        open={flash !== null}
        autoHideDuration={6000}
        onClose={() => setFlash(null)}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
      >
        <Alert severity={flash?.kind === "success" ? "success" : "error"} variant="filled" sx={{ minWidth: 240 }}>
          {flash?.msg}
        </Alert>
      </Snackbar>
    </Box>
  );
}
