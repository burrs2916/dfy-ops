import { useCallback, useEffect, useRef, useState } from "react";
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
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import { api } from "../api";
import type { RedisKeyDetail, RedisOverview } from "../types";

const btnSx = { textTransform: "none" as const, whiteSpace: "nowrap" as const };

const KIND_LABEL: Record<string, string> = {
  string: "字符串",
  hash: "哈希",
  list: "列表",
  set: "集合",
  zset: "有序集合",
  stream: "流",
  none: "不存在",
};

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

/** 概览指标卡 */
function StatCard({ icon, label, value }: { icon: React.ReactNode; label: string; value: string }) {
  return (
    <Paper sx={{ p: 1.5, display: "flex", alignItems: "center", gap: 1.5, minWidth: 0 }}>
      <Box sx={{ color: "primary.main", display: "flex" }}>{icon}</Box>
      <Box sx={{ minWidth: 0 }}>
        <Typography variant="caption" color="text.secondary" noWrap>
          {label}
        </Typography>
        <Typography variant="subtitle1" sx={{ fontWeight: 700 }} noWrap>
          {value}
        </Typography>
      </Box>
    </Paper>
  );
}

export default function RedisOps({ active }: { active: boolean }) {
  const [cred, setCred] = useState({ host: "…", port: 6379, password: "" });
  const [connected, setConnected] = useState(false);
  const [flash, setFlash] = useState<{ kind: "success" | "error"; msg: string } | null>(null);
  const flashTimer = useRef<number | null>(null);

  const [overview, setOverview] = useState<RedisOverview | null>(null);
  const [overviewErr, setOverviewErr] = useState("");

  const [pattern, setPattern] = useState("*");
  const [keys, setKeys] = useState<string[]>([]);
  const [keysTotal, setKeysTotal] = useState(0);
  const [keysTruncated, setKeysTruncated] = useState(false);
  const [keysLoading, setKeysLoading] = useState(false);

  const [selected, setSelected] = useState("");
  const [detail, setDetail] = useState<RedisKeyDetail | null>(null);
  const [detailErr, setDetailErr] = useState("");

  const [credOpen, setCredOpen] = useState(false);
  const [credForm, setCredForm] = useState({ host: "", port: 6379, password: "" });
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

  const loadKeys = useCallback(async (pat: string) => {
    setKeysLoading(true);
    try {
      const r = await api.redisKeys(pat || "*");
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
  }, [showFlash]);

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
    await Promise.all([loadOverview(), loadKeys(pattern)]);
  }, [loadOverview, loadKeys, pattern]);

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

  return (
    <Box sx={{ p: 2, height: "100%", overflow: "auto" }}>
      {/* ===== 连接信息栏 ===== */}
      <Paper sx={{ p: 1.5, mb: 2, display: "flex", alignItems: "center", gap: 1.5, flexWrap: "wrap" }}>
        <StorageIcon sx={{ color: "primary.main" }} />
        <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
          Redis 管理
        </Typography>
        <Chip
          size="small"
          label={connected ? "已连接" : overviewErr ? "连接失败" : "未连接"}
          color={connected ? "success" : "default"}
          variant={connected ? "filled" : "outlined"}
        />
        <Chip size="small" variant="outlined" label={`${cred.host}:${cred.port}`} />
        <Box sx={{ flex: 1 }} />
        <Button size="small" startIcon={<SettingsIcon />} onClick={openCredDialog} sx={btnSx}>
          连接配置
        </Button>
        <Button
          size="small"
          startIcon={<RefreshIcon />}
          onClick={() => void loadAll()}
          sx={btnSx}
        >
          刷新
        </Button>
      </Paper>

      {overviewErr && (
        <Alert severity="error" sx={{ mb: 2 }}>
          {overviewErr}
        </Alert>
      )}

      {/* ===== 总览指标 ===== */}
      <Grid container spacing={1.5} sx={{ mb: 2 }}>
        <Grid item xs={6} sm={4} md={2}>
          <StatCard icon={<StorageIcon fontSize="small" />} label="Redis 版本" value={overview?.version || "—"} />
        </Grid>
        <Grid item xs={6} sm={4} md={2}>
          <StatCard icon={<MemoryIcon fontSize="small" />} label="内存使用" value={overview?.usedMemory || "—"} />
        </Grid>
        <Grid item xs={6} sm={4} md={2}>
          <StatCard icon={<KeyIcon fontSize="small" />} label="Key 总数" value={overview ? String(overview.totalKeys) : "—"} />
        </Grid>
        <Grid item xs={6} sm={4} md={2}>
          <StatCard icon={<GroupIcon fontSize="small" />} label="客户端连接" value={overview?.connectedClients || "—"} />
        </Grid>
        <Grid item xs={6} sm={4} md={2}>
          <StatCard icon={<ScheduleIcon fontSize="small" />} label="运行天数" value={overview?.uptimeDays ? `${overview.uptimeDays} 天` : "—"} />
        </Grid>
        <Grid item xs={6} sm={4} md={2}>
          <StatCard
            icon={<TimerIcon fontSize="small" />}
            label="运行模式"
            value={overview?.mode === "cluster" ? "集群" : "单机"}
          />
        </Grid>
      </Grid>

      {/* ===== Key 浏览：左列表 + 右详情 ===== */}
      <Box sx={{ display: "flex", gap: 2, alignItems: "flex-start" }}>
        <Paper sx={{ width: 340, flexShrink: 0, p: 1.5 }}>
          <Stack direction="row" spacing={1} sx={{ mb: 1.5 }}>
            <TextField
              size="small"
              fullWidth
              label="Key 模式（支持 * 通配）"
              placeholder="例如 user:* 或 *"
              value={pattern}
              onChange={(e) => setPattern(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void loadKeys(pattern);
              }}
              InputProps={{
                startAdornment: (
                  <InputAdornment position="start">
                    <SearchIcon fontSize="small" />
                  </InputAdornment>
                ),
              }}
            />
            <Button variant="contained" size="small" sx={btnSx} disabled={keysLoading} onClick={() => void loadKeys(pattern)}>
              {keysLoading ? "扫描中…" : "扫描"}
            </Button>
          </Stack>
          <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 0.5 }}>
            {keysTruncated
              ? `共匹配 ${keysTotal} 个，仅显示前 ${keys.length} 个——请细化模式`
              : `匹配 ${keysTotal} 个 Key`}
          </Typography>
          <Box sx={{ maxHeight: 480, overflow: "auto" }}>
            {keys.map((k) => (
              <Box
                key={k}
                onClick={() => void loadDetail(k)}
                sx={{
                  px: 1.25,
                  py: 0.75,
                  borderRadius: 1,
                  cursor: "pointer",
                  display: "flex",
                  alignItems: "center",
                  gap: 1,
                  bgcolor: selected === k ? "rgba(13,148,136,0.12)" : "transparent",
                  borderLeft: selected === k ? "3px solid" : "3px solid transparent",
                  borderColor: "primary.main",
                  "&:hover": { bgcolor: "rgba(13,148,136,0.06)" },
                  overflow: "hidden",
                }}
              >
                <KeyIcon sx={{ fontSize: 16, color: "text.secondary", flexShrink: 0 }} />
                <Typography variant="body2" noWrap title={k} sx={{ fontFamily: "monospace" }}>
                  {k}
                </Typography>
              </Box>
            ))}
            {keys.length === 0 && !keysLoading && (
              <Typography variant="body2" color="text.secondary" sx={{ p: 2, textAlign: "center" }}>
                无匹配 Key——调整模式后重新扫描
              </Typography>
            )}
          </Box>
        </Paper>

        <Paper sx={{ flex: 1, minWidth: 0, p: 1.5 }}>
          {!selected && (
            <Typography variant="body2" color="text.secondary" sx={{ p: 4, textAlign: "center" }}>
              点击左侧 Key 查看内容详情
            </Typography>
          )}
          {selected && detailErr && <Alert severity="error">{detailErr}</Alert>}
          {selected && !detail && !detailErr && (
            <Typography variant="body2" color="text.secondary" sx={{ p: 4, textAlign: "center" }}>
              加载中…
            </Typography>
          )}
          {selected && detail && (
            <>
              <Stack direction="row" spacing={1} alignItems="center" sx={{ mb: 1.5, flexWrap: "wrap" }}>
                <Typography variant="subtitle1" sx={{ fontWeight: 700, fontFamily: "monospace", wordBreak: "break-all" }}>
                  {detail.name}
                </Typography>
                <Chip size="small" label={KIND_LABEL[detail.kind] || detail.kind} variant="outlined" />
                <Chip size="small" label={`TTL ${fmtTtl(detail.ttl)}`} variant="outlined" />
                <Chip size="small" label={`占用 ${formatBytes(detail.memory)}`} variant="outlined" />
                {detail.total > detail.rows.length && (
                  <Chip size="small" color="warning" label={`共 ${detail.total} 条，仅显示前 ${detail.rows.length} 条`} />
                )}
                <Box sx={{ flex: 1 }} />
                <Button size="small" color="error" startIcon={<DeleteOutlineIcon />} sx={btnSx} onClick={() => { setConfirmTarget({ name: detail.name }); setConfirmText(""); }}>
                  删除
                </Button>
              </Stack>

              <Stack direction="row" spacing={1} alignItems="center" sx={{ mb: 1.5, flexWrap: "wrap" }}>
                <Typography variant="caption" color="text.secondary">
                  过期设置：
                </Typography>
                <TextField select size="small" value={ttlMode} onChange={(e) => setTtlMode(e.target.value)} sx={{ width: 130 }} label="方式">
                  <MenuItem value="expire">N 秒后过期</MenuItem>
                  <MenuItem value="persist">永久保留</MenuItem>
                </TextField>
                {ttlMode === "expire" && (
                  <TextField
                    size="small"
                    type="number"
                    label="秒数"
                    value={ttlEdit}
                    onChange={(e) => setTtlEdit(e.target.value)}
                    sx={{ width: 140 }}
                    placeholder="如 3600"
                  />
                )}
                <Button size="small" variant="outlined" sx={btnSx} onClick={() => void applyTtl()}>
                  应用
                </Button>
              </Stack>

              <TableContainer sx={{ maxHeight: 460, border: 1, borderColor: "divider", borderRadius: 1 }}>
                <Table size="small" stickyHeader>
                  <TableHead>
                    <TableRow>
                      <TableCell sx={{ width: 180, fontWeight: 700 }}>{detail.kind === "zset" ? "成员" : "标签"}</TableCell>
                      <TableCell sx={{ fontWeight: 700 }}>值</TableCell>
                    </TableRow>
                  </TableHead>
                  <TableBody>
                    {detail.rows.map(([k, v], i) => (
                      <TableRow key={i}>
                        <TableCell sx={{ fontFamily: "monospace", wordBreak: "break-all" }}>
                          <Tooltip title={k} enterDelay={400}>
                            <span>{k}</span>
                          </Tooltip>
                        </TableCell>
                        <TableCell sx={{ fontFamily: "monospace", whiteSpace: "pre-wrap", wordBreak: "break-all" }}>
                          {v || <span style={{ color: "#9e9e9e" }}>（空）</span>}
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
      <Dialog open={credOpen} onClose={() => setCredOpen(false)} maxWidth="xs" fullWidth>
        <DialogTitle sx={{ fontWeight: 700 }}>连接配置</DialogTitle>
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
          <Button variant="contained" color="error" disabled={deleting || confirmText !== "删除"} onClick={() => void doDelete()} sx={btnSx}>
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
