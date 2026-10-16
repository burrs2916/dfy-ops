import { useCallback, useEffect, useRef, useState } from "react";
import {
  Alert,
  Box,
  Button,
  Chip,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Divider,
  FormControl,
  InputLabel,
  List,
  ListItemButton,
  ListItemText,
  MenuItem,
  Paper,
  Select,
  Snackbar,
  Stack,
  Tab,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Tabs,
  TextField,
  Tooltip,
  Typography,
} from "@mui/material";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import StopIcon from "@mui/icons-material/Stop";
import RefreshIcon from "@mui/icons-material/Refresh";
import UploadFileIcon from "@mui/icons-material/UploadFile";
import KeyIcon from "@mui/icons-material/Key";
import PersonAddIcon from "@mui/icons-material/PersonAdd";
import TerminalIcon from "@mui/icons-material/Terminal";
import GroupIcon from "@mui/icons-material/Group";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import type { MysqlOverview, MysqlTableInfo, MysqlUserInfo, SqlResult } from "../types";

const btnSx = { textTransform: "none" as const, whiteSpace: "nowrap" as const };

/** SQL 控制台结果最多渲染行数（防止大结果集卡死表格） */
const MAX_ROWS = 200;

export default function MySQLOps({ active, mysqlInstalled }: { active: boolean; mysqlInstalled: boolean }) {
  const [ov, setOv] = useState<MysqlOverview | null>(null);
  const [ovLoading, setOvLoading] = useState(true);
  const [notice, setNotice] = useState<{ text: string; severity: "success" | "error" } | null>(null);
  const [svcBusy, setSvcBusy] = useState(false);

  // 库表浏览
  const [db, setDb] = useState<string | null>(null);
  const [tables, setTables] = useState<MysqlTableInfo[]>([]);
  const [tablesLoading, setTablesLoading] = useState(false);

  // SQL 控制台 / 用户权限（右栏两个 Tab）
  const [rightTab, setRightTab] = useState<"sql" | "users">("sql");
  const [sql, setSql] = useState("SHOW DATABASES");
  const [querying, setQuerying] = useState(false);
  const [result, setResult] = useState<SqlResult | null>(null);

  const [pwdOpen, setPwdOpen] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [users, setUsers] = useState<MysqlUserInfo[] | null>(null);
  const [usersLoading, setUsersLoading] = useState(false);
  const [importing, setImporting] = useState(false);
  const noticeTimer = useRef<number | null>(null);

  const flash = useCallback((text: string, severity: "success" | "error") => {
    setNotice({ text, severity });
    if (noticeTimer.current) window.clearTimeout(noticeTimer.current);
    noticeTimer.current = window.setTimeout(() => setNotice(null), 5000);
  }, []);

  const refreshOverview = useCallback(async () => {
    setOvLoading(true);
    try {
      setOv(await api.mysqlOverview());
    } catch (e) {
      flash(`获取连接信息失败：${String(e)}`, "error");
    } finally {
      setOvLoading(false);
    }
  }, [flash]);

  // 页面可见（子页签切到「数据库运维」）即刷新连接信息——面板是 CSS 隐藏挂载，
  // mount 一次后不重走 useEffect，切页刷新才能反映安装页里刚做的启停
  useEffect(() => {
    if (active) void refreshOverview();
  }, [active, refreshOverview]);

  useEffect(() => {
    return () => {
      if (noticeTimer.current) window.clearTimeout(noticeTimer.current);
    };
  }, []);

  // 选中库后拉取表列表
  useEffect(() => {
    if (!db || !ov?.running) return;
    let cancelled = false;
    setTablesLoading(true);
    api
      .mysqlTables(db)
      .then((r) => {
        if (!cancelled) setTables(r.ok && r.tables ? r.tables : []);
      })
      .catch((e) => {
        if (!cancelled) flash(`获取表列表失败：${String(e)}`, "error");
      })
      .finally(() => {
        if (!cancelled) setTablesLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [db, ov?.running, flash]);

  const runService = useCallback(
    async (action: "start" | "stop") => {
      setSvcBusy(true);
      try {
        const r = await api.serviceAction("mysql", action);
        if (r.ok) flash(r.message ?? "操作成功", "success");
        else flash(r.error ?? "操作失败", "error");
        await refreshOverview();
      } catch (e) {
        flash(`服务操作失败：${String(e)}`, "error");
      } finally {
        setSvcBusy(false);
      }
    },
    [flash, refreshOverview],
  );

  const execSql = useCallback(
    async (statement?: string) => {
      const text = (statement ?? sql).trim();
      if (!text) return;
      if (text.split(";").filter((s) => s.trim()).length > 1) {
        flash("SQL 控制台一次执行一条语句（分号结尾，多语句请用「导入 SQL」）", "error");
        return;
      }
      setQuerying(true);
      setResult(null);
      try {
        const r = await api.mysqlExec(text, db ?? undefined);
        setResult(r);
        if (r.ok && !/^(\s*)(select|show|desc|explain|with)\b/i.test(text)) {
          flash(`执行成功${r.rowCount ? `，返回 ${r.rowCount} 行` : ""}`, "success");
        }
      } catch (e) {
        setResult({ ok: false, error: String(e) });
      } finally {
        setQuerying(false);
      }
    },
    [sql, db, flash],
  );

  const openTable = useCallback(
    (name: string) => {
      const stmt = `SELECT * FROM \`${db ?? ""}\`.\`${name}\` LIMIT 100`;
      setSql(stmt);
      setRightTab("sql");
      void execSql(stmt);
    },
    [db, execSql],
  );

  const loadUsers = useCallback(async () => {
    setUsersLoading(true);
    try {
      const r = await api.mysqlUsers();
      if (r.ok && r.users) setUsers(r.users);
      else flash(r.error ?? "获取用户列表失败", "error");
    } catch (e) {
      flash(`获取用户列表失败：${String(e)}`, "error");
    } finally {
      setUsersLoading(false);
    }
  }, [flash]);

  useEffect(() => {
    if (rightTab === "users" && ov?.running && users === null) void loadUsers();
  }, [rightTab, ov?.running, users, loadUsers]);

  const onImport = useCallback(async () => {
    let picked: string | null = null;
    try {
      const r = await open({
        multiple: false,
        title: "选择要导入的 SQL 文件",
        filters: [{ name: "SQL", extensions: ["sql"] }],
      });
      picked = typeof r === "string" ? r : null;
    } catch {
      return;
    }
    if (!picked) return;
    setImporting(true);
    try {
      const r = await api.mysqlImport(picked);
      if (r.ok) flash(`导入完成：${picked.split(/[\\/]/).pop()}`, "success");
      else flash(r.error ?? "导入失败", "error");
    } catch (e) {
      flash(`导入失败：${String(e)}`, "error");
    } finally {
      setImporting(false);
    }
  }, [flash]);

  const running = ov?.running === true;
  const sysDbs = new Set(["information_schema", "mysql", "performance_schema", "sys"]);

  return (
    <Stack spacing={2}>
      {/* MySQL 未安装：显示引导，隐藏全部操作（启停/密码/导入都无从谈起） */}
      {!mysqlInstalled && (
        <Alert severity="info">
          MySQL 尚未安装——请先切到「安装与维护」页签安装 MySQL，安装完成后回到本页进行数据库运维。
        </Alert>
      )}

      {/* ===== 连接信息与快捷操作 ===== */}
      <Paper variant="outlined" sx={{ p: 2 }}>
        <Stack direction="row" spacing={2} alignItems="center" flexWrap="wrap" rowGap={1}>
          <Stack direction="row" spacing={1} alignItems="center">
            <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
              MySQL {ov?.version ?? (ovLoading ? "检测中…" : "未连接")}
            </Typography>
            {ov && (
              <Chip
                size="small"
                color={running ? "success" : "default"}
                label={running ? "运行中" : "已停止"}
              />
            )}
            {ov?.hasPassword === false && running && (
              <Tooltip title="初始安装为空密码，建议尽快设置">
                <Chip size="small" color="warning" label="root 空密码" />
              </Tooltip>
            )}
          </Stack>
          <Typography variant="body2" color="text.secondary" sx={{ fontFamily: "monospace" }}>
            {ov ? `${ov.user}@${ov.host}:${ov.port}` : ""}
          </Typography>
          <Box sx={{ flex: 1 }} />
          <Stack direction="row" spacing={1}>
            <Button
              size="small"
              variant="outlined"
              startIcon={<RefreshIcon />}
              onClick={() => void refreshOverview()}
              sx={btnSx}
            >
              刷新
            </Button>
            {mysqlInstalled && (running ? (
              <Button
                size="small"
                variant="outlined"
                color="error"
                startIcon={<StopIcon />}
                disabled={svcBusy}
                onClick={() => void runService("stop")}
                sx={btnSx}
              >
                停止服务
              </Button>
            ) : (
              <Button
                size="small"
                variant="contained"
                startIcon={<PlayArrowIcon />}
                disabled={svcBusy || ovLoading}
                onClick={() => void runService("start")}
                sx={btnSx}
              >
                启动服务
              </Button>
            ))}
            <Button
              size="small"
              variant="outlined"
              startIcon={<KeyIcon />}
              disabled={!running}
              onClick={() => setPwdOpen(true)}
              sx={btnSx}
            >
              修改密码
            </Button>
            <Button
              size="small"
              variant="outlined"
              startIcon={<UploadFileIcon />}
              disabled={!running || importing}
              onClick={() => void onImport()}
              sx={btnSx}
            >
              导入 SQL
            </Button>
          </Stack>
        </Stack>
        {ov?.error && (
          <Alert severity="warning" sx={{ mt: 1.5 }}>
            {ov.error}
          </Alert>
        )}
        {ov && running && !ov.hasPassword && (
          <Alert severity="info" sx={{ mt: 1.5 }}>
            当前 root 为初始空密码——平台已接管连接（免密可用）。建议点击「修改密码」设置强密码，
            设置后平台自动托管，导入与控制台仍可无感使用。
          </Alert>
        )}
        {!ovLoading && !running && (
          <Alert severity="info" sx={{ mt: 1.5 }}>
            MySQL 服务未运行——启动服务后即可使用数据库运维功能。
          </Alert>
        )}
      </Paper>

      {/* ===== 主区块：CSS 隐藏挂载（停止服务不卸载，SQL 输入/结果/选中库保留） ===== */}
      <Box sx={{ display: running ? undefined : "none" }}>
        <Stack direction="row" spacing={2} alignItems="stretch" sx={{ alignItems: "flex-start" }}>
          {/* ===== 左栏：库 / 表浏览 ===== */}
          <Paper
            variant="outlined"
            sx={{ width: 300, flexShrink: 0, display: "flex", flexDirection: "column", maxHeight: 640 }}
          >
            <Box sx={{ px: 2, pt: 1.5, pb: 1 }}>
              <Typography variant="subtitle2" sx={{ fontWeight: 700 }}>
                数据库
              </Typography>
              <Typography variant="caption" color="text.secondary">
                点击库名查看表；点击表名预览前 100 行
              </Typography>
            </Box>
            <Divider />
            <List dense disablePadding sx={{ overflow: "auto", maxHeight: 560, width: "100%" }}>
              {(ov?.databases ?? []).map((d) => (
                <Box key={d}>
                  <ListItemButton selected={db === d} onClick={() => setDb(db === d ? null : d)}>
                    <ListItemText
                      primary={d}
                      primaryTypographyProps={{
                        variant: "body2",
                        sx: { fontWeight: sysDbs.has(d) ? 400 : 600, fontFamily: "monospace" },
                      }}
                      secondary={sysDbs.has(d) ? "系统库" : undefined}
                    />
                  </ListItemButton>
                  {db === d && (
                    <Box sx={{ pl: 2, pr: 1, pb: 1 }}>
                      {tablesLoading ? (
                        <Typography variant="caption" color="text.secondary">
                          加载中…
                        </Typography>
                      ) : tables.length === 0 ? (
                        <Typography variant="caption" color="text.secondary">
                          该库没有表
                        </Typography>
                      ) : (
                        tables.map((t) => (
                          <Stack
                            key={t.name}
                            direction="row"
                            spacing={1}
                            alignItems="center"
                            sx={{
                              py: 0.25,
                              cursor: "pointer",
                              borderRadius: 1,
                              px: 1,
                              "&:hover": { bgcolor: "action.hover" },
                            }}
                            onClick={() => openTable(t.name)}
                          >
                            <Typography
                              variant="body2"
                              sx={{ fontFamily: "monospace", minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}
                            >
                              {t.name}
                            </Typography>
                            <Box sx={{ flex: 1 }} />
                            <Tooltip title={`约 ${t.rows} 行 · ${t.sizeKb} KB`}>
                              <Typography variant="caption" color="text.secondary">
                                {t.rows} 行
                              </Typography>
                            </Tooltip>
                          </Stack>
                        ))
                      )}
                    </Box>
                  )}
                </Box>
              ))}
              {(ov?.databases.length ?? 0) === 0 && (
                <Box sx={{ p: 2 }}>
                  <Typography variant="caption" color="text.secondary">
                    暂无数据库
                  </Typography>
                </Box>
              )}
            </List>
          </Paper>

          {/* ===== 右栏：SQL 控制台 / 用户与权限 ===== */}
          <Paper variant="outlined" sx={{ flex: 1, minWidth: 0, p: 2 }}>
            <Tabs value={rightTab} onChange={(_, v: "sql" | "users") => setRightTab(v)}>
              <Tab
                value="sql"
                icon={<TerminalIcon fontSize="small" />}
                iconPosition="start"
                label="SQL 控制台"
              />
              <Tab
                value="users"
                icon={<GroupIcon fontSize="small" />}
                iconPosition="start"
                label="用户与权限"
              />
            </Tabs>

            {rightTab === "sql" && (
              <Stack spacing={1.5} sx={{ mt: 1.5 }}>
                <TextField
                  size="small"
                  multiline
                  minRows={3}
                  maxRows={8}
                  value={sql}
                  onChange={(e) => setSql(e.target.value)}
                  placeholder={db ? `在 ${db} 中执行，如 SELECT * FROM 表名 LIMIT 100` : "输入 SQL（选中左侧库后自动在该库执行）"}
                  sx={{ "& .MuiInputBase-input": { fontFamily: "monospace" } }}
                />
                <Stack direction="row" spacing={1} alignItems="center">
                  <Button
                    size="small"
                    variant="contained"
                    disabled={querying || !sql.trim()}
                    onClick={() => void execSql()}
                    sx={btnSx}
                  >
                    {querying ? "执行中…" : "执行"}
                  </Button>
                  {db && (
                    <Chip
                      size="small"
                      label={`当前库：${db}`}
                      onDelete={() => setDb(null)}
                    />
                  )}
                  <Box sx={{ flex: 1 }} />
                  <Button size="small" onClick={() => setResult(null)} sx={btnSx}>
                    清空结果
                  </Button>
                </Stack>
                {result && !result.ok && (
                  <Alert severity="error">{result.error}</Alert>
                )}
                {result?.ok && (
                  <SqlResultTable result={result} />
                )}
              </Stack>
            )}

            {rightTab === "users" && (
              <Stack spacing={1.5} sx={{ mt: 1.5 }}>
                <Stack direction="row" spacing={1} alignItems="center">
                  <Button
                    size="small"
                    variant="contained"
                    startIcon={<PersonAddIcon />}
                    onClick={() => setCreateOpen(true)}
                    sx={btnSx}
                  >
                    新建用户
                  </Button>
                  <Button size="small" startIcon={<RefreshIcon />} onClick={() => void loadUsers()} sx={btnSx}>
                    刷新
                  </Button>
                </Stack>
                {usersLoading ? (
                  <Typography variant="body2" color="text.secondary">
                    加载中…
                  </Typography>
                ) : (users?.length ?? 0) === 0 ? (
                  <Typography variant="body2" color="text.secondary">
                    暂无业务用户（root 为平台管理员）
                  </Typography>
                ) : (
                  users!.map((u) => (
                    <Paper key={`${u.user}@${u.host}`} variant="outlined" sx={{ p: 1.5 }}>
                      <Stack direction="row" spacing={1} alignItems="center">
                        <Typography variant="body2" sx={{ fontWeight: 600, fontFamily: "monospace" }}>
                          {u.user}@{u.host}
                        </Typography>
                        {u.user === "root" && <Chip size="small" label="平台管理员" />}
                        <Box sx={{ flex: 1 }} />
                      </Stack>
                      {u.grants.length > 0 && (
                        <Box sx={{ mt: 0.5 }}>
                          {u.grants.map((g, i) => (
                            <Typography
                              key={i}
                              variant="caption"
                              sx={{ display: "block", fontFamily: "monospace", color: "text.secondary", wordBreak: "break-all" }}
                            >
                              {g}
                            </Typography>
                          ))}
                        </Box>
                      )}
                    </Paper>
                  ))
                )}
              </Stack>
            )}
          </Paper>
        </Stack>
      </Box>

      <PasswordDialog
        open={pwdOpen}
        onClose={() => setPwdOpen(false)}
        hasPassword={ov?.hasPassword === true}
        onDone={(msg) => {
          setPwdOpen(false);
          flash(msg, "success");
          void refreshOverview();
        }}
        onError={(e) => flash(e, "error")}
      />

      <CreateUserDialog
        open={createOpen}
        onClose={() => setCreateOpen(false)}
        databases={(ov?.databases ?? []).filter((d) => !sysDbs.has(d))}
        onDone={(msg) => {
          setCreateOpen(false);
          flash(msg, "success");
          setUsers(null);
        }}
        onError={(e) => flash(e, "error")}
      />

      <Snackbar
        open={notice !== null}
        autoHideDuration={5000}
        onClose={() => setNotice(null)}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
      >
        <Alert severity={notice?.severity ?? "info"} variant="filled" sx={{ minWidth: 280 }}>
          {notice?.text}
        </Alert>
      </Snackbar>
    </Stack>
  );
}

// ===== SQL 结果表格 =====
function SqlResultTable({ result }: { result: SqlResult }) {
  const rows = (result.rows ?? []).slice(0, MAX_ROWS);
  if ((result.columns ?? []).length === 0) {
    return (
      <Alert severity="success" sx={{ mt: 0.5 }}>
        执行成功（无结果集）
      </Alert>
    );
  }
  return (
    <TableContainer sx={{ maxHeight: 360, border: "1px solid", borderColor: "divider", borderRadius: 1 }}>
      <Table size="small" stickyHeader>
        <TableHead>
          <TableRow>
            {result.columns!.map((c, i) => (
              <TableCell key={i} sx={{ fontWeight: 700, fontFamily: "monospace" }}>
                {c}
              </TableCell>
            ))}
          </TableRow>
        </TableHead>
        <TableBody>
          {rows.map((r, ri) => (
            <TableRow key={ri} hover>
              {r.map((cell, ci) => (
                <TableCell
                  key={ci}
                  sx={{
                    fontFamily: "monospace",
                    color: cell === "NULL" ? "text.disabled" : undefined,
                    whiteSpace: "nowrap",
                    maxWidth: 320,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                  }}
                >
                  {cell}
                </TableCell>
              ))}
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </TableContainer>
  );
}

// ===== 修改 root 密码 =====
function PasswordDialog({
  open,
  onClose,
  hasPassword,
  onDone,
  onError,
}: {
  open: boolean;
  onClose: () => void;
  hasPassword: boolean;
  onDone: (msg: string) => void;
  onError: (msg: string) => void;
}) {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) {
      setCurrent("");
      setNext("");
      setConfirm("");
    }
  }, [open]);

  const submit = async () => {
    if (next !== confirm) {
      onError("两次输入的新密码不一致");
      return;
    }
    setBusy(true);
    try {
      const r = await api.mysqlChangePassword(current, next);
      if (r.ok) onDone(r.message ?? "root 密码修改成功");
      else onError(r.error ?? "修改失败");
    } catch (e) {
      onError(`修改失败：${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onClose={onClose} maxWidth="xs" fullWidth>
      <DialogTitle sx={{ fontWeight: 700 }}>修改 root 密码</DialogTitle>
      <DialogContent>
        <Stack spacing={2} sx={{ mt: 0.5 }}>
          {hasPassword ? (
            <TextField
              label="当前密码"
              type="password"
              size="small"
              value={current}
              onChange={(e) => setCurrent(e.target.value)}
              fullWidth
            />
          ) : (
            <Alert severity="info">
              当前为初始空密码，「当前密码」留空即可。修改后平台自动托管新密码。
            </Alert>
          )}
          <TextField
            label="新密码（至少 6 位）"
            type="password"
            size="small"
            value={next}
            onChange={(e) => setNext(e.target.value)}
            fullWidth
          />
          <TextField
            label="确认新密码"
            type="password"
            size="small"
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
            fullWidth
          />
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} sx={btnSx}>
          取消
        </Button>
        <Button
          variant="contained"
          disabled={busy || !next || (hasPassword && !current)}
          onClick={() => void submit()}
          sx={btnSx}
        >
          {busy ? "提交中…" : "确认修改"}
        </Button>
      </DialogActions>
    </Dialog>
  );
}

// ===== 新建用户与授权 =====
function CreateUserDialog({
  open,
  onClose,
  databases,
  onDone,
  onError,
}: {
  open: boolean;
  onClose: () => void;
  databases: string[];
  onDone: (msg: string) => void;
  onError: (msg: string) => void;
}) {
  const [user, setUser] = useState("");
  const [host, setHost] = useState("localhost");
  const [password, setPassword] = useState("");
  const [privilege, setPrivilege] = useState<"ro" | "rw" | "all">("rw");
  const [database, setDatabase] = useState("*");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) {
      setUser("");
      setHost("localhost");
      setPassword("");
      setPrivilege("rw");
      setDatabase("*");
    }
  }, [open]);

  const submit = async () => {
    setBusy(true);
    try {
      const r = await api.mysqlCreateUser({ user, host, password, privilege, database });
      if (r.ok) onDone(r.message ?? "用户创建成功");
      else onError(r.error ?? "创建失败");
    } catch (e) {
      onError(`创建失败：${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  const PRIV_LABEL: Record<typeof privilege, string> = {
    ro: "只读（SELECT）",
    rw: "读写（增删改查）",
    all: "全部权限",
  };

  return (
    <Dialog open={open} onClose={onClose} maxWidth="xs" fullWidth>
      <DialogTitle sx={{ fontWeight: 700 }}>新建 MySQL 用户</DialogTitle>
      <DialogContent>
        <Stack spacing={2} sx={{ mt: 0.5 }}>
          <TextField
            label="用户名"
            size="small"
            value={user}
            onChange={(e) => setUser(e.target.value)}
            fullWidth
          />
          <TextField
            label="允许连接的主机"
            size="small"
            value={host}
            onChange={(e) => setHost(e.target.value)}
            helperText="localhost 仅本机；% 允许任意主机"
            fullWidth
          />
          <TextField
            label="密码"
            size="small"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            fullWidth
          />
          <FormControl size="small" fullWidth>
            <InputLabel>权限</InputLabel>
            <Select value={privilege} label="权限" onChange={(e) => setPrivilege(e.target.value as typeof privilege)}>
              {(Object.keys(PRIV_LABEL) as Array<typeof privilege>).map((k) => (
                <MenuItem key={k} value={k}>
                  {PRIV_LABEL[k]}
                </MenuItem>
              ))}
            </Select>
          </FormControl>
          <FormControl size="small" fullWidth>
            <InputLabel>授权范围</InputLabel>
            <Select value={database} label="授权范围" onChange={(e) => setDatabase(e.target.value)}>
              <MenuItem value="*">全部数据库</MenuItem>
              {databases.map((d) => (
                <MenuItem key={d} value={d}>
                  {d}
                </MenuItem>
              ))}
            </Select>
          </FormControl>
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} sx={btnSx}>
          取消
        </Button>
        <Button
          variant="contained"
          disabled={busy || !user || !password}
          onClick={() => void submit()}
          sx={btnSx}
        >
          {busy ? "创建中…" : "创建用户"}
        </Button>
      </DialogActions>
    </Dialog>
  );
}
