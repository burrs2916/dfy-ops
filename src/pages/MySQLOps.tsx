import { useCallback, useEffect, useRef, useState } from "react";
import { MONO_STACK } from "../theme";
import {
  Alert,
  Box,
  Button,
  Checkbox,
  Chip,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  Divider,
  FormControl,
  FormControlLabel,
  IconButton,
  InputLabel,
  LinearProgress,
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
import StorageIcon from "@mui/icons-material/Storage";
import AddCircleOutlineIcon from "@mui/icons-material/AddCircleOutline";
import DeleteOutlineIcon from "@mui/icons-material/DeleteOutline";
import TableChartIcon from "@mui/icons-material/TableChart";
import SchemaIcon from "@mui/icons-material/Schema";
import MonitorHeartIcon from "@mui/icons-material/MonitorHeart";
import BuildCircleIcon from "@mui/icons-material/BuildCircle";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import SettingsIcon from "@mui/icons-material/Settings";
import VpnKeyIcon from "@mui/icons-material/VpnKey";
import TuneIcon from "@mui/icons-material/Tune";
import BackupIcon from "@mui/icons-material/Backup";
import RestoreIcon from "@mui/icons-material/Restore";
import ChevronRightIcon from "@mui/icons-material/ChevronRight";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import DescriptionIcon from "@mui/icons-material/Description";
import SaveOutlinedIcon from "@mui/icons-material/SaveOutlined";
import NetworkCheckIcon from "@mui/icons-material/NetworkCheck";
import AdminPanelSettingsIcon from "@mui/icons-material/AdminPanelSettings";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import type {
  MysqlBackupFile,
  MysqlOverview,
  MysqlTableInfo,
  MysqlUserInfo,
  MysqlVariableItem,
  SqlResult,
} from "../types";
import MysqlTablePanel from "./MysqlTablePanel";

const btnSx = { textTransform: "none" as const, whiteSpace: "nowrap" as const };

/** 系统库清单：备份范围下拉排除（与后端 SYS_DATABASES 同口径） */
const SYS_DBS = ["information_schema", "mysql", "performance_schema", "sys"];

type RightTab = "data" | "schema" | "sql" | "users" | "server";

/** 强确认对话框：删除类操作必须显式输入「确认」之外的强提示文案确认 */
function ConfirmDialog({
  target,
  onClose,
}: {
  target: { title: string; content: string; confirmText: string; action: () => Promise<void> } | null;
  onClose: () => void;
}) {
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => setTyped(""), [target]);
  return (
    <Dialog open={target !== null} onClose={onClose} maxWidth="xs" fullWidth>
      <DialogTitle sx={{ fontWeight: 700, display: "flex", alignItems: "center", gap: 1 }}>
        <WarningAmberIcon color="warning" />
        {target?.title}
      </DialogTitle>
      <DialogContent>
        <DialogContentText sx={{ whiteSpace: "pre-wrap" }}>{target?.content}</DialogContentText>
        <TextField
          size="small"
          fullWidth
          sx={{ mt: 2 }}
          label={`输入 ${target?.confirmText ?? ""} 以确认`}
          value={typed}
          onChange={(e) => setTyped(e.target.value)}
        />
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} sx={btnSx}>
          取消
        </Button>
        <Button
          variant="contained"
          color="error"
          disabled={busy || typed !== target?.confirmText}
          onClick={async () => {
            setBusy(true);
            try {
              await target!.action();
              onClose();
            } finally {
              setBusy(false);
            }
          }}
          sx={btnSx}
        >
          {busy ? "执行中…" : "确认执行"}
        </Button>
      </DialogActions>
    </Dialog>
  );
}

export default function MySQLOps({ active, mysqlInstalled }: { active: boolean; mysqlInstalled: boolean }) {
  const [ov, setOv] = useState<MysqlOverview | null>(null);
  const [ovLoading, setOvLoading] = useState(true);
  const [notice, setNotice] = useState<{ text: string; severity: "success" | "error" } | null>(null);
  const [svcBusy, setSvcBusy] = useState(false);

  // 库表浏览
  const [db, setDb] = useState<string | null>(null);
  const [table, setTable] = useState<string | null>(null);
  const [tables, setTables] = useState<MysqlTableInfo[]>([]);
  const [tablesLoading, setTablesLoading] = useState(false);
  const [expandedDbs, setExpandedDbs] = useState<Set<string>>(new Set());
  const [newDbOpen, setNewDbOpen] = useState(false);
  const [newDbName, setNewDbName] = useState("");

  // 右栏视图
  const [rightTab, setRightTab] = useState<RightTab>("sql");

  // SQL 控制台
  const [sql, setSql] = useState("SHOW DATABASES");
  const [querying, setQuerying] = useState(false);
  const [result, setResult] = useState<SqlResult | null>(null);
  const [history, setHistory] = useState<string[]>([]);

  // 用户与权限
  const [users, setUsers] = useState<MysqlUserInfo[] | null>(null);
  const [usersLoading, setUsersLoading] = useState(false);
  const [pwdOpen, setPwdOpen] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [credOpen, setCredOpen] = useState(false);

  // 服务器（processlist + 常用参数动态配置）
  const [procs, setProcs] = useState<{ columns: string[]; rows: string[][] } | null>(null);
  const [procsLoading, setProcsLoading] = useState(false);
  const [vars, setVars] = useState<MysqlVariableItem[] | null>(null);
  const [varsLoading, setVarsLoading] = useState(false);

  // 备份与恢复（业务库一键备份 + 备份文件管理）
  const [backups, setBackups] = useState<MysqlBackupFile[]>([]);
  const [backupsLoading, setBackupsLoading] = useState(false);
  const [backupDb, setBackupDb] = useState("*");
  const [backupBusy, setBackupBusy] = useState(false);

  // my.ini 配置文件（服务器页在线编辑；保存由后端自动备份）
  const [iniPath, setIniPath] = useState<string | null>(null);
  const [iniContent, setIniContent] = useState("");
  const [iniOriginal, setIniOriginal] = useState("");
  const [iniError, setIniError] = useState<string | null>(null);
  const [iniLoading, setIniLoading] = useState(false);
  const [iniSaving, setIniSaving] = useState(false);

  // 导入 SQL 向导：选文件 → 预分析（是否含建库/USE 语句）→ 选目标库 → 执行 → 结果
  const [importOpen, setImportOpen] = useState(false);
  const [importFile, setImportFile] = useState<string | null>(null);
  const [importAnalyze, setImportAnalyze] = useState<{
    ok: boolean;
    sizeBytes?: number;
    hasCreateDb?: boolean;
    createDbName?: string | null;
    useDb?: string | null;
    scriptTables?: string[];
    scriptHasDrop?: boolean;
    error?: string;
  } | null>(null);
  const [importAnalyzing, setImportAnalyzing] = useState(false);
  const [importDb, setImportDb] = useState("");
  const [importNewDbName, setImportNewDbName] = useState("");
  const [importOnError, setImportOnError] = useState<"stop" | "continue">("stop");
  // 冲突检测（选库后自动查询）：重名表 → 用户选策略；无冲突 → 直接导入不打扰
  const [importConflicts, setImportConflicts] = useState<{
    dbExists: boolean;
    conflicts: string[];
    scriptHasDrop: boolean;
  } | null>(null);
  const [importChecking, setImportChecking] = useState(false);
  const [importStrategy, setImportStrategy] = useState<"direct" | "rebuild" | "append" | "recreate_db">("direct");
  const [importConfirmDanger, setImportConfirmDanger] = useState(false);
  const [importing, setImporting] = useState(false);
  // 导入结果：状态 + 错误明细；导入结束后自动验证库表结构与行数
  const [importResult, setImportResult] = useState<{
    ok: boolean;
    message: string;
    skippedCount: number;
    errors: string[];
  } | null>(null);
  const [importVerify, setImportVerify] = useState<{
    tables: { name: string; rows: string | null }[];
    missing: string[];
  } | null>(null);
  const [importVerifying, setImportVerifying] = useState(false);
  // 导入过程实时进度（后端轮询）：阶段 + 字节 + 实时错误数
  const [importProgress, setImportProgress] = useState<{
    phase: string;
    bytes: number;
    total: number;
    errors: number;
  } | null>(null);
  const [confirm, setConfirm] = useState<{
    title: string;
    content: string;
    confirmText: string;
    action: () => Promise<void>;
  } | null>(null);
  const noticeTimer = useRef<number | null>(null);

  const flash = useCallback((text: string, severity: "success" | "error") => {
    setNotice({ text, severity });
    if (noticeTimer.current) window.clearTimeout(noticeTimer.current);
    noticeTimer.current = window.setTimeout(() => setNotice(null), 6000);
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

  // 页面可见（子页签切到「MySQL 管理」）即刷新连接信息——面板是 CSS 隐藏挂载，
  // mount 一次后不重走 useEffect，切页刷新才能反映安装页里刚做的启停
  useEffect(() => {
    if (active) void refreshOverview();
  }, [active, refreshOverview]);

  useEffect(() => {
    return () => {
      if (noticeTimer.current) window.clearTimeout(noticeTimer.current);
    };
  }, []);

  // 拉取当前浏览库的表列表（useEffect 依赖变化时 + 导入完成后自动刷新共用）
  const reloadTables = useCallback(
    (targetDb?: string) => {
      const d = targetDb ?? db;
      if (!d || !ov?.running) return;
      let cancelled = false;
      setTablesLoading(true);
      api
        .mysqlTables(d)
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
    },
    [db, ov?.running, flash],
  );

  // 选中库后拉取表列表
  useEffect(() => reloadTables(), [reloadTables]);

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
        flash("SQL 控制台一次执行一条语句（分号结尾，批量语句请用「导入 SQL」）", "error");
        return;
      }
      setQuerying(true);
      setResult(null);
      try {
        const r = await api.mysqlExec(text, db ?? undefined);
        setResult(r);
        if (r.ok) {
          setHistory((h) => [text, ...h.filter((x) => x !== text)].slice(0, 10));
          if (!/^(\s*)(select|show|desc|explain|with)\b/i.test(text)) {
            flash(`执行成功${r.rowCount ? `，返回 ${r.rowCount} 行` : ""}`, "success");
          }
        }
      } catch (e) {
        setResult({ ok: false, error: String(e) });
      } finally {
        setQuerying(false);
      }
    },
    [sql, db, flash],
  );

  const openTable = useCallback((t: string) => {
    setTable(t);
    setRightTab("data");
  }, []);

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

  const loadProcs = useCallback(async () => {
    setProcsLoading(true);
    try {
      const r = await api.mysqlProcesslist();
      if (r.ok) setProcs({ columns: r.columns ?? [], rows: r.rows ?? [] });
      else flash(r.error ?? "获取会话列表失败", "error");
    } catch (e) {
      flash(`获取会话列表失败：${String(e)}`, "error");
    } finally {
      setProcsLoading(false);
    }
  }, [flash]);

  const loadVars = useCallback(async () => {
    setVarsLoading(true);
    try {
      const r = await api.mysqlVariables();
      if (r.ok && r.items) setVars(r.items);
      else flash(r.error ?? "获取参数失败", "error");
    } catch (e) {
      flash(`获取参数失败：${String(e)}`, "error");
    } finally {
      setVarsLoading(false);
    }
  }, [flash]);

  const loadIni = useCallback(async () => {
    setIniLoading(true);
    setIniError(null);
    try {
      const r = await api.mysqlConfigFile();
      if (r.ok && r.content != null) {
        setIniPath(r.path ?? "");
        setIniContent(r.content);
        setIniOriginal(r.content);
      } else {
        setIniPath(null);
        setIniContent("");
        setIniOriginal("");
        setIniError(r.error ?? "未找到 my.ini");
      }
    } catch (e) {
      setIniError(`读取配置失败：${String(e)}`);
    } finally {
      setIniLoading(false);
    }
  }, []);

  const saveIni = useCallback(async () => {
    setIniSaving(true);
    try {
      const r = await api.mysqlSaveConfigFile(iniContent);
      if (r.ok) {
        setIniOriginal(iniContent);
        flash(r.message ?? "已保存，重启 MySQL 服务后生效", "success");
      } else flash(r.error ?? "保存失败", "error");
    } catch (e) {
      flash(`保存失败：${String(e)}`, "error");
    } finally {
      setIniSaving(false);
    }
  }, [iniContent, flash]);

  const loadBackups = useCallback(async () => {
    setBackupsLoading(true);
    try {
      const r = await api.mysqlBackupsList();
      setBackups(r.files ?? []);
    } catch {
      setBackups([]);
    } finally {
      setBackupsLoading(false);
    }
  }, []);

  const doBackup = useCallback(async () => {
    setBackupBusy(true);
    try {
      const r = await api.mysqlBackupNow(backupDb);
      if (r.ok) {
        flash(r.message ?? "备份完成", "success");
        void loadBackups();
      } else {
        flash(r.error ?? "备份失败", "error");
      }
    } catch (e) {
      flash(`备份失败：${String(e)}`, "error");
    } finally {
      setBackupBusy(false);
    }
  }, [backupDb, flash, loadBackups]);

  const restoreBackup = useCallback(
    (f: MysqlBackupFile) => {
      setConfirm({
        title: "恢复备份",
        content: `将使用备份文件 ${f.file}（${f.sizeKb} KB）恢复数据库。\n同名表的数据会被备份内容覆盖，此操作不可撤销。\n输入「恢复」以确认。`,
        confirmText: "恢复",
        action: async () => {
          try {
            const r = await api.mysqlRestoreBackup(f.file);
            if (r.ok) {
              flash(r.message ?? "恢复完成", "success");
              void loadBackups();
            } else {
              flash(r.error ?? "恢复失败", "error");
            }
          } catch (e) {
            flash(`恢复失败：${String(e)}`, "error");
          }
        },
      });
    },
    [flash, loadBackups],
  );

  useEffect(() => {
    if (rightTab === "server" && ov?.running) {
      void loadProcs();
      void loadVars();
      void loadIni();
      void loadBackups();
    }
  }, [rightTab, ov?.running, loadProcs, loadVars, loadIni, loadBackups]);

  const openImport = useCallback(() => {
    setImportFile(null);
    setImportAnalyze(null);
    setImportAnalyzing(false);
    setImportDb("");
    setImportNewDbName("");
    setImportOnError("stop");
    setImportConflicts(null);
    setImportChecking(false);
    setImportStrategy("direct");
    setImportConfirmDanger(false);
    setImportResult(null);
    setImportVerify(null);
    setImportVerifying(false);
    setImporting(false);
    setImportOpen(true);
  }, []);

  const pickImportFile = useCallback(async () => {
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
    setImportFile(picked);
    setImportAnalyze(null);
    setImportResult(null);
    setImportVerify(null);
    setImportConflicts(null);
    setImportStrategy("direct");
    setImportConfirmDanger(false);
    setImportAnalyzing(true);
    try {
      setImportAnalyze(await api.mysqlImportAnalyze(picked));
    } catch (e) {
      setImportAnalyze({ ok: false, error: String(e) });
    } finally {
      setImportAnalyzing(false);
    }
  }, []);

  // 选库变化 → 自动查目标库冲突（新建库不查，永远无冲突）
  const onImportDbChange = useCallback(
    async (v: string) => {
      setImportDb(v);
      setImportConflicts(null);
      setImportStrategy("direct");
      setImportConfirmDanger(false);
      if (!v || v === "__new__" || !importFile) return;
      setImportChecking(true);
      try {
        const r = await api.mysqlImportConflicts(importFile, v);
        if (r.ok) {
          const conflicts = r.conflicts ?? [];
          setImportConflicts({
            dbExists: !!r.dbExists,
            conflicts,
            scriptHasDrop: !!r.scriptHasDrop,
          });
          // 有重名表 → 默认推荐"重建涉及的表"（幂等刷新这批表）
          if (conflicts.length > 0) setImportStrategy("rebuild");
        }
      } catch {
        setImportConflicts(null);
      } finally {
        setImportChecking(false);
      }
    },
    [importFile],
  );

  const doImport = useCallback(async () => {
    if (!importFile) return;
    const isNew = importDb === "__new__";
    const hasConflicts = (importConflicts?.conflicts.length ?? 0) > 0;
    // 实际目标库：显式选库 > 新建库 > 脚本 USE 的库（验证与表列表刷新都对准它）
    const targetDb = isNew ? importNewDbName.trim() : importDb || importAnalyze?.useDb || "";
    setImporting(true);
    setImportResult(null);
    setImportVerify(null);
    // 每秒轮询后端进度槽：阶段 + 字节进度 + 实时错误计数
    const poll = window.setInterval(() => {
      api
        .mysqlImportProgress()
        .then((p) => {
          if (p.phase !== "done") setImportProgress(p);
        })
        .catch(() => {});
    }, 800);
    try {
      const r = await api.mysqlImport(importFile, {
        database: isNew ? undefined : importDb || undefined,
        newDatabase: isNew ? importNewDbName.trim() : undefined,
        onError: hasConflicts ? (importStrategy === "append" ? "continue" : "stop") : importOnError,
        conflictStrategy: isNew || !hasConflicts ? "direct" : importStrategy,
      });
      if (r.ok) {
        setImportResult({
          ok: true,
          message: r.message ?? "导入完成",
          skippedCount: r.skippedCount ?? 0,
          errors: r.errors ?? [],
        });
      } else {
        setImportResult({
          ok: false,
          message: r.error ?? "导入失败",
          skippedCount: 0,
          errors: r.errors ?? [],
        });
      }
      // 导入后自动验证：目标库实际表清单与行数，对照脚本表名找出缺失表
      if (targetDb && validDbNameLike(targetDb)) {
        setImportVerifying(true);
        try {
          const v = await api.mysqlImportVerify(targetDb);
          if (v.ok && v.tables) {
            const scriptTables = importAnalyze?.scriptTables ?? [];
            const names = v.tables.map((t) => t.name);
            const missing = scriptTables.filter(
              (t) => !names.some((n) => n.toLowerCase() === t.toLowerCase()),
            );
            setImportVerify({ tables: v.tables, missing });
          }
        } catch {
          setImportVerify(null);
        } finally {
          setImportVerifying(false);
        }
      }
      // 导入完成自动刷新右侧表列表（目标库是当前浏览库时立即可见新表）
      reloadTables(targetDb);
      setImporting(false);
      void refreshOverview();
    } catch (e) {
      setImportResult({ ok: false, message: `导入失败：${String(e)}`, skippedCount: 0, errors: [] });
      setImporting(false);
    } finally {
      window.clearInterval(poll);
      setImportProgress(null);
    }
  }, [importFile, importDb, importNewDbName, importOnError, importStrategy, importConflicts, importAnalyze, reloadTables, refreshOverview]);

  // 与后端 valid_db_name 同口径的前端宽松校验（仅用于决定是否发起验证请求）
  const validDbNameLike = (s: string) => s.length > 0 && s.length <= 64;

  const importNeedDb =
    !!importAnalyze?.ok && !importAnalyzing && !importAnalyze.hasCreateDb && !importAnalyze.useDb;
  const hasConflicts = (importConflicts?.conflicts.length ?? 0) > 0;
  const importCanSubmit =
    !!importFile &&
    !!importAnalyze?.ok &&
    !importAnalyzing &&
    !importChecking &&
    !importing &&
    (!importNeedDb ||
      (importDb === "__new__" ? importNewDbName.trim().length > 0 : !!importDb)) &&
    (importStrategy !== "recreate_db" || importConfirmDanger);

  const tableAction = useCallback(
    (action: "optimize" | "analyze" | "truncate" | "drop") => {
      if (!db || !table) return;
      const label = { optimize: "优化", analyze: "分析", truncate: "清空", drop: "删除" }[action];
      const destructive = action === "truncate" || action === "drop";
      const run = async () => {
        try {
          const r = await api.mysqlTableAction(db, table, action);
          if (r.ok) flash(r.message ?? `${label}完成`, "success");
          else flash(r.error ?? `${label}失败`, "error");
        } catch (e) {
          flash(`${label}失败：${String(e)}`, "error");
        }
      };
      if (destructive) {
        setConfirm({
          title: `${label}表 ${table}`,
          content: destructive
            ? `即将${label}表 ${db}.${table}。\n平台会先自动备份该库（mysqldump），备份成功后才执行${label}；备份失败将中止操作。`
            : "",
          confirmText: table,
          action: run,
        });
      } else {
        void run();
      }
    },
    [db, table, flash],
  );

  const dropDb = useCallback(
    (name: string) => {
      setConfirm({
        title: `删除数据库 ${name}`,
        content: `即将删除数据库 ${name} 及其全部数据。\n平台会先自动备份该库（mysqldump），备份成功后才执行删除；备份失败将中止。`,
        confirmText: name,
        action: async () => {
          try {
            const r = await api.mysqlDropDb(name);
            if (r.ok) {
              flash(r.message ?? "删除完成", "success");
              if (db === name) {
                setDb(null);
                setTable(null);
              }
              void refreshOverview();
            } else flash(r.error ?? "删除失败", "error");
          } catch (e) {
            flash(`删除失败：${String(e)}`, "error");
          }
        },
      });
    },
    [db, flash, refreshOverview],
  );

  const createDb = useCallback(async () => {
    if (!newDbName.trim()) return;
    try {
      const r = await api.mysqlCreateDb(newDbName.trim());
      if (r.ok) {
        flash(r.message ?? "创建成功", "success");
        setNewDbOpen(false);
        setNewDbName("");
        void refreshOverview();
      } else flash(r.error ?? "创建失败", "error");
    } catch (e) {
      flash(`创建失败：${String(e)}`, "error");
    }
  }, [newDbName, flash, refreshOverview]);

  const running = ov?.running === true;
  const sysDbs = new Set(["information_schema", "mysql", "performance_schema", "sys"]);
  const idColIndex = procs?.columns.findIndex((c) => c.toLowerCase() === "id") ?? -1;

  return (
    <Stack spacing={2}>
      {/* MySQL 未安装：显示引导，隐藏全部操作（启停/密码/导入都无从谈起） */}
      {!mysqlInstalled && (
        <Alert severity="info">
          MySQL 尚未安装——请先切到「安装与维护」页签安装 MySQL，安装完成后回到本页即可使用。
        </Alert>
      )}

      {/* ===== 连接信息与快捷操作 ===== */}
      <Paper variant="outlined" sx={{ p: 2, position: "relative", overflow: "hidden" }}>
        {/* 品牌渐变饰条（#0D9488 → #0EA5E9），呼应 logo 提升页面质感 */}
        <Box
          sx={{
            position: "absolute",
            top: 0,
            left: 0,
            right: 0,
            height: 3,
            background: "linear-gradient(90deg, #0D9488, #0EA5E9)",
          }}
        />
        <Stack direction="row" spacing={2} alignItems="center" flexWrap="wrap" rowGap={1}>
          <Stack direction="row" spacing={1} alignItems="center">
            <StorageIcon color="primary" fontSize="small" />
            <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
              MySQL {ov?.version ?? (ovLoading ? "检测中…" : "未连接")}
            </Typography>
            {ov && (
              <Chip size="small" color={running ? "success" : "default"} label={running ? "运行中" : "已停止"} />
            )}
            {ov?.hasPassword === false && running && (
              <Tooltip title="初始安装为空密码，建议尽快设置">
                <Chip size="small" color="warning" label="root 空密码" />
              </Tooltip>
            )}
          </Stack>
          <Typography variant="body2" color="text.secondary" sx={{ fontFamily: MONO_STACK }}>
            {ov ? `${ov.user}@${ov.host}:${ov.port}` : ""}
          </Typography>
          <Box sx={{ flex: 1 }} />
          <Stack direction="row" spacing={1}>
            <Button size="small" variant="outlined" startIcon={<RefreshIcon />} onClick={() => void refreshOverview()} sx={btnSx}>
              刷新
            </Button>
            {mysqlInstalled &&
              (running ? (
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
            <Button size="small" variant="outlined" startIcon={<KeyIcon />} disabled={!running} onClick={() => setPwdOpen(true)} sx={btnSx}>
              修改密码
            </Button>
            <Tooltip title="平台连接 MySQL 的用户/密码/端口（平台外改过密码后在这里重新对上）">
              <span>
                <Button
                  size="small"
                  variant="outlined"
                  startIcon={<SettingsIcon />}
                  disabled={!mysqlInstalled}
                  onClick={() => setCredOpen(true)}
                  sx={btnSx}
                >
                  连接配置
                </Button>
              </span>
            </Tooltip>
            <Button size="small" variant="outlined" startIcon={<UploadFileIcon />} disabled={!running} onClick={openImport} sx={btnSx}>
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
        {!ovLoading && mysqlInstalled && !running && (
          <Alert severity="info" sx={{ mt: 1.5 }}>
            MySQL 服务未运行——启动服务后即可使用本页管理功能。
          </Alert>
        )}
      </Paper>

      {/* ===== 主区块：CSS 隐藏挂载（停止服务不卸载，SQL 输入/结果/选中库保留） ===== */}
      <Box sx={{ display: running ? undefined : "none" }}>
        <Stack direction="row" spacing={2} alignItems="flex-start">
          {/* ===== 左栏：Navicat 风格库表树（箭头展开 · 图标分层 · 悬浮操作） ===== */}
          <Paper
            variant="outlined"
            sx={{ width: 300, flexShrink: 0, display: "flex", flexDirection: "column", maxHeight: 660 }}
          >
            <Box sx={{ px: 2, pt: 1.5, pb: 1 }}>
              <Stack direction="row" spacing={1} alignItems="center">
                <Typography variant="subtitle2" sx={{ fontWeight: 700 }}>
                  数据库
                </Typography>
                {(ov?.databases.length ?? 0) > 0 && (
                  <Chip
                    size="small"
                    variant="outlined"
                    label={ov!.databases.length}
                    sx={{ height: 18, fontSize: 11, color: "text.secondary" }}
                  />
                )}
                <Box sx={{ flex: 1 }} />
                <Tooltip title="新建数据库">
                  <IconButton size="small" color="primary" disabled={!running} onClick={() => setNewDbOpen(true)}>
                    <AddCircleOutlineIcon fontSize="small" />
                  </IconButton>
                </Tooltip>
              </Stack>
              <Typography variant="caption" color="text.secondary">
                点击数据库展开表，点击表查看数据
              </Typography>
            </Box>
            <Divider />
            <List dense disablePadding sx={{ overflow: "auto", maxHeight: 580, width: "100%", pb: 1 }}>
              {(ov?.databases ?? []).map((d) => {
                const open = expandedDbs.has(d);
                return (
                  <Box key={d}>
                    <ListItemButton
                      dense
                      selected={db === d}
                      onClick={() => {
                        setDb(d);
                        setTable(null);
                        setExpandedDbs((s) => {
                          const n = new Set(s);
                          if (n.has(d)) n.delete(d);
                          else n.add(d);
                          return n;
                        });
                      }}
                    >
                      {open ? (
                        <ExpandMoreIcon sx={{ fontSize: 16, color: "text.secondary", mr: 0.5 }} />
                      ) : (
                        <ChevronRightIcon sx={{ fontSize: 16, color: "text.secondary", mr: 0.5 }} />
                      )}
                      <StorageIcon
                        sx={{ fontSize: 15, mr: 1, color: sysDbs.has(d) ? "text.disabled" : "primary.main" }}
                      />
                      <ListItemText
                        primary={d}
                        primaryTypographyProps={{
                          variant: "body2",
                          sx: { fontWeight: sysDbs.has(d) ? 400 : 600, fontFamily: MONO_STACK },
                        }}
                        secondary={sysDbs.has(d) ? "系统库" : undefined}
                      />
                      {!sysDbs.has(d) && (
                        <Tooltip title="删除该库（先自动备份，需输入库名确认）">
                          <IconButton
                            size="small"
                            onClick={(e) => {
                              e.stopPropagation();
                              dropDb(d);
                            }}
                            sx={{
                              opacity: 0,
                              transition: "opacity 0.15s",
                              ".MuiListItemButton-root:hover &": { opacity: 1 },
                            }}
                          >
                            <DeleteOutlineIcon sx={{ fontSize: 16 }} color="action" />
                          </IconButton>
                        </Tooltip>
                      )}
                    </ListItemButton>
                    {open && (
                      <Box sx={{ pl: 4.5, pr: 1, pb: 1, bgcolor: "action.hover" }}>
                        {tablesLoading ? (
                          <Typography variant="caption" color="text.secondary">
                            加载中…
                          </Typography>
                        ) : tables.length === 0 ? (
                          <Typography variant="caption" color="text.secondary">
                            该库没有表
                          </Typography>
                        ) : (
                          tables.map((t) => {
                            const active = table === t.name;
                            return (
                              <Stack
                                key={t.name}
                                direction="row"
                                spacing={0.75}
                                alignItems="center"
                                sx={{
                                  py: 0.4,
                                  px: 1,
                                  mb: 0.25,
                                  cursor: "pointer",
                                  borderRadius: 1,
                                  bgcolor: active ? "primary.main" : undefined,
                                  "&:hover": { bgcolor: active ? "primary.dark" : "action.selected" },
                                }}
                                onClick={() => openTable(t.name)}
                              >
                                <TableChartIcon
                                  sx={{ fontSize: 14, color: active ? "common.white" : "text.secondary" }}
                                />
                                <Typography
                                  variant="body2"
                                  sx={{
                                    fontFamily: MONO_STACK,
                                    minWidth: 0,
                                    overflow: "hidden",
                                    textOverflow: "ellipsis",
                                    fontWeight: active ? 600 : undefined,
                                    color: active ? "common.white" : undefined,
                                  }}
                                >
                                  {t.name}
                                </Typography>
                                <Box sx={{ flex: 1 }} />
                                <Tooltip title={`约 ${t.rows} 行 · ${t.sizeKb} KB`}>
                                  <Typography
                                    variant="caption"
                                    sx={{ color: active ? "common.white" : "text.secondary", opacity: active ? 0.85 : 1 }}
                                  >
                                    {t.rows} 行
                                  </Typography>
                                </Tooltip>
                              </Stack>
                            );
                          })
                        )}
                      </Box>
                    )}
                  </Box>
                );
              })}
              {(ov?.databases.length ?? 0) === 0 && (
                <Box sx={{ p: 2 }}>
                  <Typography variant="caption" color="text.secondary">
                    暂无数据库
                  </Typography>
                </Box>
              )}
            </List>
          </Paper>

          {/* ===== 右栏：表数据 / 表结构 / SQL 控制台 / 用户与权限 / 服务器 ===== */}
          <Paper
            variant="outlined"
            sx={{ flex: 1, minWidth: 0, p: 2, display: "flex", flexDirection: "column", overflow: "hidden" }}
          >
            <Tabs value={rightTab} onChange={(_, v: RightTab) => setRightTab(v)} variant="scrollable" scrollButtons="auto">
              <Tab value="data" icon={<TableChartIcon fontSize="small" />} iconPosition="start" label="表数据" />
              <Tab value="schema" icon={<SchemaIcon fontSize="small" />} iconPosition="start" label="表结构" />
              <Tab value="sql" icon={<TerminalIcon fontSize="small" />} iconPosition="start" label="SQL 控制台" />
              <Tab value="users" icon={<GroupIcon fontSize="small" />} iconPosition="start" label="用户与权限" />
              <Tab value="server" icon={<MonitorHeartIcon fontSize="small" />} iconPosition="start" label="服务器" />
            </Tabs>

            {(rightTab === "data" || rightTab === "schema") && !(db && table) && (
              <Alert severity="info" sx={{ mt: 1.5 }}>
                请先在左侧选择数据库并点击表名，即可在此浏览表数据或表结构。
              </Alert>
            )}

            {(rightTab === "data" || rightTab === "schema") && db && table && (
              <Box sx={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column", minWidth: 0 }}>
                {/* 表工具条：当前表 + 维护操作 */}
                <Stack direction="row" spacing={1} alignItems="center" sx={{ mt: 1.5 }} flexWrap="wrap" rowGap={1}>
                  <Chip size="small" icon={<StorageIcon />} label={`${db}.${table}`} sx={{ fontFamily: MONO_STACK }} />
                  <Box sx={{ flex: 1 }} />
                  <Button size="small" startIcon={<BuildCircleIcon />} onClick={() => tableAction("optimize")} sx={btnSx}>
                    优化
                  </Button>
                  <Button size="small" startIcon={<BuildCircleIcon />} onClick={() => tableAction("analyze")} sx={btnSx}>
                    分析
                  </Button>
                  <Button size="small" color="warning" onClick={() => tableAction("truncate")} sx={btnSx}>
                    清空
                  </Button>
                  <Button size="small" color="error" onClick={() => tableAction("drop")} sx={btnSx}>
                    删除表
                  </Button>
                </Stack>
                <MysqlTablePanel db={db} table={table} view={rightTab === "schema" ? "schema" : "data"} onNotify={flash} />
              </Box>
            )}

            {rightTab === "sql" && (
              <Stack spacing={1.5} sx={{ mt: 1.5 }}>
                <TextField
                  size="small"
                  multiline
                  minRows={3}
                  maxRows={8}
                  value={sql}
                  onChange={(e) => setSql(e.target.value)}
                  onKeyDown={(e) => {
                    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
                      e.preventDefault();
                      if (sql.trim() && !querying) void execSql();
                    }
                  }}
                  placeholder={db ? `在 ${db} 中执行，如 SELECT * FROM 表名 LIMIT 100` : "输入 SQL（选中左侧库后自动在该库执行）"}
                  sx={{ "& .MuiInputBase-input": { fontFamily: MONO_STACK } }}
                />
                <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap" rowGap={1}>
                  <Button size="small" variant="contained" disabled={querying || !sql.trim()} onClick={() => void execSql()} sx={btnSx}>
                    {querying ? "执行中…" : "执行"}
                  </Button>
                  <Typography variant="caption" color="text.secondary">
                    Ctrl+Enter
                  </Typography>
                  {db && <Chip size="small" label={`当前库：${db}`} onDelete={() => setDb(null)} />}
                  <Box sx={{ flex: 1 }} />
                  {history.length > 0 && (
                    <FormControl size="small" sx={{ minWidth: 180 }}>
                      <Select
                        displayEmpty
                        value=""
                        onChange={(e) => {
                          const h = e.target.value as string;
                          if (h) {
                            setSql(h);
                            void execSql(h);
                          }
                        }}
                      >
                        <MenuItem value="" disabled>
                          执行历史（最近 10 条）
                        </MenuItem>
                        {history.map((h, i) => (
                          <MenuItem key={i} value={h} sx={{ maxWidth: 420 }}>
                            <Typography variant="caption" sx={{ fontFamily: MONO_STACK, overflow: "hidden", textOverflow: "ellipsis" }}>
                              {h}
                            </Typography>
                          </MenuItem>
                        ))}
                      </Select>
                    </FormControl>
                  )}
                  <Button size="small" onClick={() => setResult(null)} sx={btnSx}>
                    清空结果
                  </Button>
                </Stack>
                {result && !result.ok && <Alert severity="error">{result.error}</Alert>}
                {result?.ok && (result.columns?.length ?? 0) === 0 ? (
                  <Alert severity="success">执行成功（无结果集）</Alert>
                ) : result?.ok ? (
                  <TableContainer sx={{ maxHeight: 360, border: "1px solid", borderColor: "divider", borderRadius: 1 }}>
                    <Table size="small" stickyHeader>
                      <TableHead>
                        <TableRow>
                          {result.columns!.map((c, i) => (
                            <TableCell key={i} sx={{ fontWeight: 700, fontFamily: MONO_STACK }}>
                              {c}
                            </TableCell>
                          ))}
                        </TableRow>
                      </TableHead>
                      <TableBody>
                        {(result.rows ?? []).map((r, ri) => (
                          <TableRow key={ri} hover>
                            {r.map((cell, ci) => (
                              <TableCell
                                key={ci}
                                sx={{
                                  fontFamily: MONO_STACK,
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
                ) : null}
              </Stack>
            )}

            {rightTab === "users" && (
              <Stack spacing={1.5} sx={{ mt: 1.5 }}>
                <Stack direction="row" spacing={1} alignItems="center">
                  <Button size="small" variant="contained" startIcon={<PersonAddIcon />} onClick={() => setCreateOpen(true)} sx={btnSx}>
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
                        <Typography variant="body2" sx={{ fontWeight: 600, fontFamily: MONO_STACK }}>
                          {u.user}@{u.host}
                        </Typography>
                        {u.user === "root" && <Chip size="small" label="平台管理员" />}
                        <Box sx={{ flex: 1 }} />
                        {u.user !== "root" && (
                          <UserPasswordButton
                            user={u.user}
                            host={u.host}
                            onDone={(m) => {
                              flash(m, "success");
                              setUsers(null);
                            }}
                            onError={(e) => flash(e, "error")}
                          />
                        )}
                        {u.user !== "root" && (
                          <UserGrantButton
                            user={u.user}
                            host={u.host}
                            databases={(ov?.databases ?? []).filter((d) => !sysDbs.has(d))}
                            onDone={(m) => {
                              flash(m, "success");
                              setUsers(null);
                            }}
                            onError={(e) => flash(e, "error")}
                          />
                        )}
                        {u.user !== "root" && (
                          <Button
                            size="small"
                            color="error"
                            onClick={() =>
                              setConfirm({
                                title: `删除用户 ${u.user}@${u.host}`,
                                content: `即将删除该用户并撤销其全部授权。`,
                                confirmText: u.user,
                                action: async () => {
                                  try {
                                    const r = await api.mysqlDropUser(u.user, u.host);
                                    if (r.ok) {
                                      flash(r.message ?? "用户已删除", "success");
                                      setUsers(null);
                                    } else flash(r.error ?? "删除失败", "error");
                                  } catch (e) {
                                    flash(`删除失败：${String(e)}`, "error");
                                  }
                                },
                              })
                            }
                            sx={btnSx}
                          >
                            删除
                          </Button>
                        )}
                      </Stack>
                      {u.grants.length > 0 && (
                        <Box sx={{ mt: 0.75 }}>
                          {u.grants.map((g, i) => (
                            <Tooltip key={i} title={g} placement="top-start">
                              <Typography
                                variant="caption"
                                sx={{
                                  display: "block",
                                  fontFamily: MONO_STACK,
                                  color: "text.secondary",
                                  whiteSpace: "nowrap",
                                  overflow: "hidden",
                                  textOverflow: "ellipsis",
                                }}
                              >
                                {g}
                              </Typography>
                            </Tooltip>
                          ))}
                        </Box>
                      )}
                    </Paper>
                  ))
                )}
              </Stack>
            )}

            {rightTab === "server" && (
              <Stack spacing={1.5} sx={{ mt: 1.5 }}>
                <Stack direction="row" spacing={1} alignItems="center">
                  <Typography variant="subtitle2" sx={{ fontWeight: 700 }}>
                    当前会话（进程列表）
                  </Typography>
                  <Box sx={{ flex: 1 }} />
                  <Button size="small" startIcon={<RefreshIcon />} onClick={() => void loadProcs()} disabled={procsLoading} sx={btnSx}>
                    刷新
                  </Button>
                </Stack>
                {procs?.columns.length ? (
                  <TableContainer sx={{ maxHeight: 420, border: "1px solid", borderColor: "divider", borderRadius: 1 }}>
                    <Table size="small" stickyHeader>
                      <TableHead>
                        <TableRow>
                          {procs.columns.map((c, i) => (
                            <TableCell key={i} sx={{ fontWeight: 700 }}>
                              {c}
                            </TableCell>
                          ))}
                          <TableCell>操作</TableCell>
                        </TableRow>
                      </TableHead>
                      <TableBody>
                        {procs.rows.map((r, ri) => {
                          const pid = idColIndex >= 0 ? Number(r[idColIndex]) : NaN;
                          return (
                            <TableRow key={ri} hover>
                              {r.map((cell, ci) => (
                                <TableCell
                                  key={ci}
                                  sx={{
                                    fontFamily: MONO_STACK,
                                    whiteSpace: "nowrap",
                                    maxWidth: 280,
                                    overflow: "hidden",
                                    textOverflow: "ellipsis",
                                  }}
                                >
                                  {cell}
                                </TableCell>
                              ))}
                              <TableCell>
                                <Button
                                  size="small"
                                  color="error"
                                  disabled={Number.isNaN(pid)}
                                  onClick={() =>
                                    setConfirm({
                                      title: `终止会话 ${pid}`,
                                      content: `即将 KILL 该会话。若它正在执行长事务/查询，操作将中断。`,
                                      confirmText: String(pid),
                                      action: async () => {
                                        try {
                                          const res = await api.mysqlKill(pid);
                                          if (res.ok) {
                                            flash(res.message ?? "会话已终止", "success");
                                            void loadProcs();
                                          } else flash(res.error ?? "终止失败", "error");
                                        } catch (e) {
                                          flash(`终止失败：${String(e)}`, "error");
                                        }
                                      },
                                    })
                                  }
                                  sx={btnSx}
                                >
                                  终止
                                </Button>
                              </TableCell>
                            </TableRow>
                          );
                        })}
                      </TableBody>
                    </Table>
                  </TableContainer>
                ) : (
                  <Typography variant="body2" color="text.secondary">
                    {procsLoading ? "加载中…" : "暂无会话数据"}
                  </Typography>
                )}
              </Stack>
            )}

            {rightTab === "server" && (
              <Divider sx={{ my: 2 }} />
            )}

            {rightTab === "server" && (
              <Stack spacing={1.5}>
                <Stack direction="row" spacing={1} alignItems="center">
                  <Typography variant="subtitle2" sx={{ fontWeight: 700, display: "flex", alignItems: "center", gap: 0.5 }}>
                    <TuneIcon fontSize="small" /> 常用参数（在线调整）
                  </Typography>
                  <Box sx={{ flex: 1 }} />
                  <Button size="small" startIcon={<RefreshIcon />} onClick={() => void loadVars()} disabled={varsLoading} sx={btnSx}>
                    刷新
                  </Button>
                </Stack>
                <Alert severity="info" variant="outlined">
                  修改后立即生效（SET GLOBAL）；重启服务后恢复为 my.ini 配置值，需要持久化请在下方「配置文件（my.ini）」中修改。
                </Alert>
                {varsLoading ? (
                  <Typography variant="body2" color="text.secondary">
                    加载中…
                  </Typography>
                ) : (vars?.length ?? 0) === 0 ? (
                  <Typography variant="body2" color="text.secondary">
                    暂无参数数据
                  </Typography>
                ) : (
                  <TableContainer sx={{ border: "1px solid", borderColor: "divider", borderRadius: 1 }}>
                    <Table size="small">
                      <TableHead>
                        <TableRow>
                          <TableCell sx={{ fontWeight: 700 }}>参数</TableCell>
                          <TableCell sx={{ fontWeight: 700 }}>当前值</TableCell>
                          <TableCell sx={{ fontWeight: 700 }}>调整</TableCell>
                        </TableRow>
                      </TableHead>
                      <TableBody>
                        {vars!.map((v) => (
                          <VariableRow
                            key={v.name}
                            item={v}
                            onSaved={(m) => flash(m, "success")}
                            onError={(e) => flash(e, "error")}
                          />
                        ))}
                      </TableBody>
                    </Table>
                  </TableContainer>
                )}
              </Stack>
            )}

            {rightTab === "server" && <Divider sx={{ my: 2 }} />}

            {rightTab === "server" && (
              <Stack spacing={1.5}>
                <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap" rowGap={1}>
                  <Typography variant="subtitle2" sx={{ fontWeight: 700, display: "flex", alignItems: "center", gap: 0.5 }}>
                    <BackupIcon fontSize="small" /> 备份与恢复
                  </Typography>
                  <Box sx={{ flex: 1 }} />
                  <FormControl size="small" sx={{ minWidth: 200 }}>
                    <InputLabel>备份范围</InputLabel>
                    <Select
                      size="small"
                      label="备份范围"
                      value={backupDb}
                      onChange={(e) => setBackupDb(e.target.value)}
                    >
                      <MenuItem value="*">全部业务库（跳过系统库）</MenuItem>
                      {(ov?.databases ?? [])
                        .filter((d) => !SYS_DBS.includes(d.toLowerCase()))
                        .map((d) => (
                          <MenuItem key={d} value={d}>
                            {d}
                          </MenuItem>
                        ))}
                    </Select>
                  </FormControl>
                  <Button
                    size="small"
                    variant="contained"
                    startIcon={<BackupIcon />}
                    disabled={backupBusy}
                    onClick={() => void doBackup()}
                    sx={btnSx}
                  >
                    {backupBusy ? "备份中…" : "立即备份"}
                  </Button>
                  <Button
                    size="small"
                    startIcon={<RefreshIcon />}
                    onClick={() => void loadBackups()}
                    disabled={backupsLoading}
                    sx={btnSx}
                  >
                    刷新
                  </Button>
                </Stack>
                <Alert severity="info" variant="outlined">
                  备份保存到工作目录的 backups 文件夹，包含表结构、数据和存储过程；恢复会覆盖同名表数据，需输入「恢复」强确认。
                </Alert>
                {backupsLoading ? (
                  <Typography variant="body2" color="text.secondary">
                    加载中…
                  </Typography>
                ) : backups.length === 0 ? (
                  <Typography variant="body2" color="text.secondary">
                    暂无备份文件
                  </Typography>
                ) : (
                  <TableContainer sx={{ border: "1px solid", borderColor: "divider", borderRadius: 1 }}>
                    <Table size="small">
                      <TableHead>
                        <TableRow>
                          <TableCell sx={{ fontWeight: 700 }}>备份文件</TableCell>
                          <TableCell sx={{ fontWeight: 700 }}>大小</TableCell>
                          <TableCell sx={{ fontWeight: 700 }}>时间</TableCell>
                          <TableCell sx={{ fontWeight: 700 }}>操作</TableCell>
                        </TableRow>
                      </TableHead>
                      <TableBody>
                        {backups.map((f) => (
                          <TableRow key={f.file} hover>
                            <TableCell sx={{ fontFamily: MONO_STACK }}>{f.file}</TableCell>
                            <TableCell>{f.sizeKb} KB</TableCell>
                            <TableCell>{new Date(f.modified * 1000).toLocaleString()}</TableCell>
                            <TableCell>
                              <Button
                                size="small"
                                startIcon={<RestoreIcon />}
                                onClick={() => restoreBackup(f)}
                                sx={btnSx}
                              >
                                恢复
                              </Button>
                            </TableCell>
                          </TableRow>
                        ))}
                      </TableBody>
                    </Table>
                  </TableContainer>
                )}
              </Stack>
            )}

            {rightTab === "server" && <Divider sx={{ my: 2 }} />}

            {rightTab === "server" && (
              <Stack spacing={1.5}>
                <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap" rowGap={1}>
                  <Typography variant="subtitle2" sx={{ fontWeight: 700, display: "flex", alignItems: "center", gap: 0.5 }}>
                    <DescriptionIcon fontSize="small" /> 配置文件（my.ini）
                  </Typography>
                  {iniPath && (
                    <Tooltip title={iniPath}>
                      <Typography
                        variant="caption"
                        sx={{ fontFamily: MONO_STACK, color: "text.secondary", maxWidth: 380, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
                      >
                        {iniPath}
                      </Typography>
                    </Tooltip>
                  )}
                  {iniPath && iniContent !== iniOriginal && (
                    <Chip size="small" color="warning" label="有未保存修改" />
                  )}
                  <Box sx={{ flex: 1 }} />
                  <Button
                    size="small"
                    startIcon={<RefreshIcon />}
                    onClick={() => {
                      if (
                        iniContent !== iniOriginal &&
                        !window.confirm("当前有未保存的修改，重新加载将丢弃这些修改。继续？")
                      )
                        return;
                      void loadIni();
                    }}
                    disabled={iniLoading}
                    sx={btnSx}
                  >
                    重新加载
                  </Button>
                  <Button
                    size="small"
                    variant="contained"
                    startIcon={<SaveOutlinedIcon />}
                    disabled={iniSaving || iniLoading || !iniPath || iniContent === iniOriginal}
                    onClick={() => void saveIni()}
                    sx={btnSx}
                  >
                    {iniSaving ? "保存中…" : "保存（自动备份）"}
                  </Button>
                </Stack>
                {iniError ? (
                  <Alert severity="warning" variant="outlined">
                    {iniError}
                  </Alert>
                ) : (
                  <>
                    <Alert severity="info" variant="outlined">
                      在线修改 MySQL 配置文件：保存前自动备份为 my.ini.bak-时间戳，重启 MySQL 服务后生效；格式错误会导致服务启动失败，可用备份文件还原。
                    </Alert>
                    <TextField
                      size="small"
                      multiline
                      minRows={8}
                      maxRows={16}
                      value={iniContent}
                      disabled={iniLoading || !iniPath}
                      onChange={(e) => setIniContent(e.target.value)}
                      placeholder={iniLoading ? "加载中…" : ""}
                      sx={{ "& .MuiInputBase-input": { fontFamily: MONO_STACK, fontSize: 13 } }}
                    />
                  </>
                )}
              </Stack>
            )}
          </Paper>
        </Stack>
      </Box>

      {/* 新建库 */}
      <Dialog open={newDbOpen} onClose={() => setNewDbOpen(false)} maxWidth="xs" fullWidth>
        <DialogTitle sx={{ fontWeight: 700 }}>新建数据库</DialogTitle>
        <DialogContent>
          <TextField
            autoFocus
            size="small"
            fullWidth
            sx={{ mt: 1 }}
            label="库名"
            value={newDbName}
            onChange={(e) => setNewDbName(e.target.value)}
            helperText="utf8mb4 字符集；仅字母、数字、下划线、$"
            onKeyDown={(e) => {
              if (e.key === "Enter") void createDb();
            }}
          />
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setNewDbOpen(false)} sx={btnSx}>
            取消
          </Button>
          <Button variant="contained" disabled={!newDbName.trim()} onClick={() => void createDb()} sx={btnSx}>
            创建
          </Button>
        </DialogActions>
      </Dialog>

      {/* ===== 导入 SQL 向导：预分析 → 关联目标库 → 执行 → 结果 ===== */}
      <Dialog
        open={importOpen}
        onClose={() => {
          if (!importing) setImportOpen(false);
        }}
        maxWidth="sm"
        fullWidth
      >
        <DialogTitle>导入 SQL 脚本</DialogTitle>
        <DialogContent dividers>
          <Stack spacing={2}>
            <Paper
              variant="outlined"
              sx={{
                p: 1.5,
                borderStyle: "dashed",
                borderColor: "rgba(13, 148, 136, 0.45)",
                display: "flex",
                alignItems: "center",
                gap: 1.5,
                minWidth: 0,
              }}
            >
              <Button
                size="small"
                variant="contained"
                startIcon={<UploadFileIcon />}
                onClick={() => void pickImportFile()}
                disabled={importing}
              >
                选择 SQL 文件
              </Button>
              <Typography
                variant="body2"
                title={importFile ?? undefined}
                sx={importFile ? { fontFamily: MONO_STACK, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" } : { color: "text.secondary" }}
              >
                {importFile
                  ? `${importFile.split(/[\\/]/).pop()}${
                      importAnalyze?.ok && importAnalyze.sizeBytes
                        ? `（${Math.max(1, Math.round(importAnalyze.sizeBytes / 1024))} KB）`
                        : ""
                    }`
                  : "支持含 / 不含建库语句的 .sql 脚本"}
              </Typography>
            </Paper>
            {importAnalyzing && <Alert severity="info">正在分析脚本结构…</Alert>}
            {importAnalyze && !importAnalyzing && !importAnalyze.ok && (
              <Alert severity="error">{importAnalyze.error ?? "无法读取该文件"}</Alert>
            )}
            {importAnalyze?.ok && !importAnalyzing && importAnalyze.hasCreateDb && (
              <Alert severity="success">
                脚本含建库语句{importAnalyze.createDbName ? `（${importAnalyze.createDbName}）` : ""}
                ，将按脚本执行
              </Alert>
            )}
            {importAnalyze?.ok && !importAnalyzing && !importAnalyze.hasCreateDb && importAnalyze.useDb && (
              <Alert severity="info">
                未检测到建库语句，脚本将使用库「{importAnalyze.useDb}」——也可在下方改选其他库
              </Alert>
            )}
            {importNeedDb && (
              <Alert severity="warning">
                未检测到建库 / USE 语句——请先选择目标数据库，导入时平台会自动切换到该库执行
              </Alert>
            )}
            {importAnalyze?.ok && !importAnalyze.hasCreateDb && !importAnalyzing && (
              <FormControl size="small" fullWidth disabled={importing || importChecking}>
                <InputLabel>{importNeedDb ? "目标数据库（必选）" : "目标数据库（可选）"}</InputLabel>
                <Select
                  label={importNeedDb ? "目标数据库（必选）" : "目标数据库（可选）"}
                  value={importDb}
                  onChange={(e) => void onImportDbChange(e.target.value)}
                >
                  {importAnalyze.useDb && <MenuItem value=""><em>跟随脚本（{importAnalyze.useDb}）</em></MenuItem>}
                  {(ov?.databases ?? [])
                    .filter((d) => !sysDbs.has(d))
                    .map((d) => (
                      <MenuItem key={d} value={d}>
                        {d}
                      </MenuItem>
                    ))}
                  <MenuItem value="__new__">
                    <em>＋ 新建数据库…</em>
                  </MenuItem>
                </Select>
              </FormControl>
            )}
            {importDb === "__new__" && (
              <TextField
                size="small"
                label="新库名称"
                value={importNewDbName}
                onChange={(e) => setImportNewDbName(e.target.value)}
                disabled={importing}
                helperText="仅允许中文、字母、数字、下划线（1-64 字符）；自动采用 utf8mb4 + unicode 排序（中文完整存储，兼容 MySQL 5.5+）"
              />
            )}
            {/* 冲突检测与处理：选库后自动查询；无冲突不打扰，有冲突让用户选策略 */}
            {importChecking && <Alert severity="info">正在检查目标库的表冲突…</Alert>}
            {importConflicts && !importChecking && importDb !== "__new__" && (
              hasConflicts ? (
                <>
                  <Alert severity="warning">
                    <Typography variant="body2" sx={{ fontWeight: 600, mb: 0.5 }}>
                      目标库已存在 {importConflicts.conflicts.length} 张同名表（重复导入），请选择处理方式：
                    </Typography>
                    <Typography variant="caption" sx={{ fontFamily: MONO_STACK, wordBreak: "break-all", display: "block" }}>
                      {importConflicts.conflicts.join("、")}
                    </Typography>
                    {importConflicts.scriptHasDrop && (
                      <Typography variant="caption" sx={{ display: "block", mt: 0.5, color: "text.secondary" }}>
                        脚本自带 DROP TABLE IF EXISTS，本身即可安全重复导入
                      </Typography>
                    )}
                  </Alert>
                  <FormControl size="small" fullWidth disabled={importing}>
                    <InputLabel>冲突处理方式</InputLabel>
                    <Select
                      label="冲突处理方式"
                      value={importStrategy === "direct" ? "rebuild" : importStrategy}
                      onChange={(e) => {
                        const v = e.target.value as "rebuild" | "append" | "recreate_db";
                        setImportStrategy(v);
                        if (v !== "recreate_db") setImportConfirmDanger(false);
                      }}
                    >
                      <MenuItem value="rebuild">
                        <Stack>
                          <Typography variant="body2">重建涉及的表（推荐）</Typography>
                          <Typography variant="caption" color="text.secondary">
                            先删除这 {importConflicts.conflicts.length} 张表再导入，库中其他表与数据不受影响
                          </Typography>
                        </Stack>
                      </MenuItem>
                      <MenuItem value="append">
                        <Stack>
                          <Typography variant="body2">追加 / 跳过冲突</Typography>
                          <Typography variant="caption" color="text.secondary">
                            保留现有表，跳过报错语句继续导入
                          </Typography>
                        </Stack>
                      </MenuItem>
                      <MenuItem value="recreate_db">
                        <Stack>
                          <Typography variant="body2" sx={{ color: "error.main", fontWeight: 600 }}>
                            整库重建（危险）
                          </Typography>
                          <Typography variant="caption" color="text.secondary">
                            删除整个数据库后重新导入，库中所有数据将丢失
                          </Typography>
                        </Stack>
                      </MenuItem>
                    </Select>
                  </FormControl>
                  {importStrategy === "recreate_db" && (
                    <FormControlLabel
                      control={
                        <Checkbox
                          size="small"
                          checked={importConfirmDanger}
                          onChange={(e) => setImportConfirmDanger(e.target.checked)}
                          sx={{ color: "error.main", "&.Mui-checked": { color: "error.main" } }}
                        />
                      }
                      label={
                        <Typography variant="body2" sx={{ color: "error.main" }}>
                          我已知晓：整个库将被删除并重建，库中所有现有数据都会丢失
                        </Typography>
                      }
                    />
                  )}
                </>
              ) : (
                <Alert severity="success">
                  {importConflicts.dbExists
                    ? "目标库已存在，与脚本无同名表冲突，可直接导入"
                    : "目标库当前不存在，导入时将自动创建"}
                </Alert>
              )
            )}
            {/* 无冲突场景下的通用遇错策略（有冲突时由冲突处理方式决定，避免重复） */}
            {!hasConflicts && (
              <FormControl size="small" fullWidth disabled={importing}>
                <InputLabel>遇错策略</InputLabel>
                <Select
                  label="遇错策略"
                  value={importOnError}
                  onChange={(e) => setImportOnError(e.target.value as "stop" | "continue")}
                >
                  <MenuItem value="stop">遇错停止（推荐）</MenuItem>
                  <MenuItem value="continue">跳过错误继续（结束后统计被跳过的错误）</MenuItem>
                </Select>
              </FormControl>
            )}
            {importing && importProgress && importProgress.phase !== "done" && (
              <Box>
                <Stack direction="row" justifyContent="space-between" alignItems="baseline" sx={{ mb: 0.5 }}>
                  <Typography variant="body2" sx={{ fontWeight: 600 }}>
                    {importProgress.phase === "preparing" && "正在连接 MySQL…"}
                    {importProgress.phase === "creating_db" && "正在创建数据库…"}
                    {importProgress.phase === "clearing_tables" && "正在清理同名表…"}
                    {importProgress.phase === "recreating_db" && "正在重建数据库…"}
                    {importProgress.phase === "importing" && "正在导入…请勿关闭窗口"}
                    {!["preparing", "creating_db", "clearing_tables", "recreating_db", "importing"].includes(importProgress.phase) && "正在处理…"}
                  </Typography>
                  <Typography variant="caption" color="text.secondary">
                    {importProgress.phase === "importing" && importProgress.total > 0 && (
                      <>
                        {(importProgress.bytes / 1048576).toFixed(1)} / {(importProgress.total / 1048576).toFixed(1)} MB ·{" "}
                        {((importProgress.bytes / importProgress.total) * 100).toFixed(0)}%
                      </>
                    )}
                    {importProgress.errors > 0 && (
                      <Typography component="span" variant="caption" sx={{ color: "warning.main" }}>
                        {" "}· 已捕获 {importProgress.errors} 条错误
                      </Typography>
                    )}
                  </Typography>
                </Stack>
                <LinearProgress
                  variant={
                    importProgress.phase === "importing" && importProgress.total > 0
                      ? "determinate"
                      : "indeterminate"
                  }
                  value={
                    importProgress.total > 0
                      ? (importProgress.bytes / importProgress.total) * 100
                      : undefined
                  }
                />
              </Box>
            )}
            {importing && !importProgress && (
              <Alert severity="info">正在导入，大文件可能需要一些时间，请勿关闭窗口…</Alert>
            )}
            {importVerifying && <Alert severity="info">正在验证目标库的表结构…</Alert>}
            {importResult && (
              <>
                <Alert
                  severity={
                    importResult.ok
                      ? importResult.skippedCount > 0
                        ? "warning"
                        : "success"
                      : "error"
                  }
                >
                  {importResult.message}
                </Alert>
                {importResult.errors.length > 0 && (
                  <Box sx={{ maxHeight: 180, overflow: "auto", borderRadius: 1.5, bgcolor: "action.hover", p: 1.25 }}>
                    {importResult.errors.map((e, i) => (
                      <Typography
                        key={i}
                        variant="caption"
                        sx={{ display: "block", fontFamily: MONO_STACK, color: "error.main", wordBreak: "break-all", py: 0.15 }}
                      >
                        {e}
                      </Typography>
                    ))}
                  </Box>
                )}
              </>
            )}
            {importVerify && !importVerifying && (
              <Box>
                <Typography variant="body2" sx={{ fontWeight: 600, mb: 0.5 }}>
                  {importVerify.tables.length > 0
                    ? `目标库现有 ${importVerify.tables.length} 张表（行数为估算值）`
                    : "目标库当前没有任何表"}
                </Typography>
                {importVerify.missing.length > 0 && (
                  <Alert severity="warning" sx={{ mb: 1 }}>
                    <Typography variant="body2" sx={{ fontWeight: 600 }}>
                      {importVerify.missing.length} 张脚本中的表未成功创建，导入可能不完整：
                    </Typography>
                    <Typography variant="caption" sx={{ fontFamily: MONO_STACK, wordBreak: "break-all" }}>
                      {importVerify.missing.join("、")}
                    </Typography>
                  </Alert>
                )}
                {importVerify.tables.length > 0 && (
                  <Box sx={{ maxHeight: 200, overflow: "auto", borderRadius: 1.5, bgcolor: "action.hover", p: 1.25 }}>
                    {importVerify.tables.map((t) => (
                      <Stack key={t.name} direction="row" justifyContent="space-between" sx={{ py: 0.2 }}>
                        <Typography variant="caption" sx={{ fontFamily: MONO_STACK, wordBreak: "break-all" }}>
                          {t.name}
                        </Typography>
                        <Typography variant="caption" color="text.secondary" sx={{ flexShrink: 0, pl: 1.5 }}>
                          {t.rows ? `${t.rows} 行` : "—"}
                        </Typography>
                      </Stack>
                    ))}
                  </Box>
                )}
              </Box>
            )}
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setImportOpen(false)} disabled={importing}>
            关闭
          </Button>
          <Button variant="contained" disabled={!importCanSubmit} onClick={() => void doImport()}>
            {importing ? "导入中…" : "开始导入"}
          </Button>
        </DialogActions>
      </Dialog>

      <ConfirmDialog target={confirm} onClose={() => setConfirm(null)} />

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

      <CredentialDialog
        open={credOpen}
        onClose={() => setCredOpen(false)}
        ov={ov}
        onDone={(msg) => {
          setCredOpen(false);
          flash(msg, "success");
          void refreshOverview();
        }}
        onError={(e) => flash(e, "error")}
      />

      <Snackbar
        open={notice !== null}
        autoHideDuration={6000}
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
          <TextField label="用户名" size="small" value={user} onChange={(e) => setUser(e.target.value)} fullWidth />
          <TextField
            label="允许连接的主机"
            size="small"
            value={host}
            onChange={(e) => setHost(e.target.value)}
            helperText="localhost 仅本机；% 允许任意主机"
            fullWidth
          />
          <TextField label="密码" size="small" value={password} onChange={(e) => setPassword(e.target.value)} fullWidth />
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
        <Button variant="contained" disabled={busy || !user || !password} onClick={() => void submit()} sx={btnSx}>
          {busy ? "创建中…" : "创建用户"}
        </Button>
      </DialogActions>
    </Dialog>
  );
}

// ===== 调整已有用户权限：撤销现有授权后按档位重授，密码不变 =====
function UserGrantButton({
  user,
  host,
  databases,
  onDone,
  onError,
}: {
  user: string;
  host: string;
  databases: string[];
  onDone: (msg: string) => void;
  onError: (msg: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [privilege, setPrivilege] = useState<"ro" | "rw" | "all">("rw");
  const [database, setDatabase] = useState("*");
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    setBusy(true);
    try {
      const r = await api.mysqlUserGrants(user, host, privilege, database);
      if (r.ok) {
        setOpen(false);
        onDone(r.message ?? "权限已调整");
      } else onError(r.error ?? "调整失败");
    } catch (e) {
      onError(`调整失败：${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <Button size="small" startIcon={<AdminPanelSettingsIcon />} onClick={() => setOpen(true)} sx={btnSx}>
        调整权限
      </Button>
      <Dialog open={open} onClose={() => setOpen(false)} maxWidth="xs" fullWidth>
        <DialogTitle sx={{ fontWeight: 700 }}>调整权限</DialogTitle>
        <DialogContent>
          <Stack spacing={2} sx={{ mt: 0.5 }}>
            <Typography variant="body2" color="text.secondary">
              目标账号 <Typography component="span" sx={{ fontFamily: MONO_STACK, fontWeight: 600 }}>{user}@{host}</Typography>
              。先撤销现有全部授权，再按下方选择重新授权，密码保持不变。
            </Typography>
            <FormControl size="small" fullWidth>
              <InputLabel>权限档位</InputLabel>
              <Select
                value={privilege}
                label="权限档位"
                onChange={(e) => setPrivilege(e.target.value as typeof privilege)}
              >
                <MenuItem value="ro">只读（SELECT）</MenuItem>
                <MenuItem value="rw">读写（增删改查）</MenuItem>
                <MenuItem value="all">全部权限</MenuItem>
              </Select>
            </FormControl>
            <FormControl size="small" fullWidth>
              <InputLabel>授权范围</InputLabel>
              <Select
                value={database}
                label="授权范围"
                onChange={(e) => setDatabase(e.target.value)}
              >
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
          <Button onClick={() => setOpen(false)} disabled={busy} sx={btnSx}>
            取消
          </Button>
          <Button variant="contained" disabled={busy} onClick={() => void submit()} sx={btnSx}>
            {busy ? "调整中…" : "确认调整"}
          </Button>
        </DialogActions>
      </Dialog>
    </>
  );
}

// ===== 业务用户改密码（root 执行，无需旧密码） =====
function UserPasswordButton({
  user,
  host,
  onDone,
  onError,
}: {
  user: string;
  host: string;
  onDone: (msg: string) => void;
  onError: (msg: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) {
      setPassword("");
      setConfirm("");
    }
  }, [open]);

  const submit = async () => {
    if (password !== confirm) {
      onError("两次输入的密码不一致");
      return;
    }
    setBusy(true);
    try {
      const r = await api.mysqlUserPassword(user, host, password);
      if (r.ok) {
        setOpen(false);
        onDone(r.message ?? "密码修改成功");
      } else onError(r.error ?? "修改失败");
    } catch (e) {
      onError(`修改失败：${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <Button size="small" startIcon={<VpnKeyIcon />} onClick={() => setOpen(true)} sx={btnSx}>
        改密码
      </Button>
      <Dialog open={open} onClose={() => setOpen(false)} maxWidth="xs" fullWidth>
        <DialogTitle sx={{ fontWeight: 700 }}>修改用户密码</DialogTitle>
        <DialogContent>
          <Stack spacing={2} sx={{ mt: 0.5 }}>
            <Typography variant="body2" color="text.secondary">
              目标账号 <Typography component="span" sx={{ fontFamily: MONO_STACK, fontWeight: 600 }}>{user}@{host}</Typography>
              ，由 root 执行改密，无需旧密码。使用该账号的应用需同步更新连接密码。
            </Typography>
            <TextField
              label="新密码（至少 6 位）"
              type="password"
              size="small"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
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
          <Button onClick={() => setOpen(false)} sx={btnSx}>
            取消
          </Button>
          <Button
            variant="contained"
            disabled={busy || password.length < 6 || !confirm}
            onClick={() => void submit()}
            sx={btnSx}
          >
            {busy ? "提交中…" : "确认修改"}
          </Button>
        </DialogActions>
      </Dialog>
    </>
  );
}

// ===== 服务器参数行：单参数在线调整（SET GLOBAL） =====
function VariableRow({
  item,
  onSaved,
  onError,
}: {
  item: MysqlVariableItem;
  onSaved: (msg: string) => void;
  onError: (msg: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(item.value);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!editing) setValue(item.value);
  }, [item.value, editing]);

  const cancel = () => {
    setValue(item.value);
    setEditing(false);
  };

  const save = async () => {
    const v = value.trim();
    if (!v || v === item.value) {
      cancel();
      return;
    }
    setBusy(true);
    try {
      const r = await api.mysqlSetVariable(item.name, v);
      if (r.ok) {
        setEditing(false);
        onSaved(r.message ?? `${item.name} 已调整`);
      } else onError(r.error ?? "调整失败");
    } catch (e) {
      onError(`调整失败：${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <TableRow hover>
      <TableCell sx={{ fontFamily: MONO_STACK, fontWeight: 600, whiteSpace: "nowrap" }}>{item.name}</TableCell>
      <TableCell sx={{ fontFamily: MONO_STACK, maxWidth: 280, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
        <Tooltip title={item.value} disableHoverListener={editing}>
          <span>{item.value}</span>
        </Tooltip>
      </TableCell>
      <TableCell>
        {editing ? (
          <Stack direction="row" spacing={1} alignItems="center">
            <TextField
              size="small"
              autoFocus
              value={value}
              onChange={(e) => setValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void save();
                if (e.key === "Escape") cancel();
              }}
              sx={{ width: 220 }}
            />
            <Button size="small" variant="contained" disabled={busy} onClick={() => void save()} sx={btnSx}>
              {busy ? "保存中…" : "保存"}
            </Button>
            <Button size="small" disabled={busy} onClick={cancel} sx={btnSx}>
              取消
            </Button>
          </Stack>
        ) : (
          <Button size="small" onClick={() => setEditing(true)} sx={btnSx}>
            调整
          </Button>
        )}
      </TableCell>
    </TableRow>
  );
}

// ===== 连接配置：平台托管凭据修正（平台外改过密码/端口后的自救入口） =====
function CredentialDialog({
  open,
  onClose,
  ov,
  onDone,
  onError,
}: {
  open: boolean;
  onClose: () => void;
  ov: MysqlOverview | null;
  onDone: (msg: string) => void;
  onError: (msg: string) => void;
}) {
  const [user, setUser] = useState("");
  const [port, setPort] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => {
    if (open) {
      setUser(ov?.user ?? "");
      setPort(ov?.port != null ? String(ov.port) : "");
      setPassword("");
      setTestResult(null);
    }
    // ov 只在打开瞬间取预填值，打开期间不随刷新重置输入
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const parsePort = (): number | null => {
    const n = Number(port.trim());
    return Number.isInteger(n) && n >= 1 && n <= 65535 ? n : null;
  };

  // 用表单当前值实测连接（不写托管文件）：保存前先验证
  const runTest = async () => {
    if (!user.trim()) {
      setTestResult({ ok: false, text: "用户名不能为空" });
      return;
    }
    const portNum = parsePort();
    if (portNum === null) {
      setTestResult({ ok: false, text: "端口必须是 1-65535 的整数" });
      return;
    }
    setTesting(true);
    setTestResult(null);
    try {
      const r = await api.mysqlTestConnection(user.trim(), password, portNum);
      setTestResult(
        r.ok
          ? { ok: true, text: `连接成功${r.version ? `，服务器版本 ${r.version}` : ""}` }
          : { ok: false, text: r.error ?? "连接失败" },
      );
    } catch (e) {
      setTestResult({ ok: false, text: `测试失败：${String(e)}` });
    } finally {
      setTesting(false);
    }
  };

  const submit = async () => {
    if (!user.trim()) {
      onError("用户名不能为空");
      return;
    }
    const portNum = parsePort();
    if (portNum === null) {
      onError("端口必须是 1-65535 的整数");
      return;
    }
    setBusy(true);
    try {
      const r = await api.mysqlUpdateCredential({
        user: user.trim(),
        port: portNum,
        ...(password ? { password } : {}),
      });
      if (r.ok) onDone(r.message ?? "连接配置已保存");
      else onError(r.error ?? "保存失败");
    } catch (e) {
      onError(`保存失败：${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onClose={onClose} maxWidth="xs" fullWidth>
      <DialogTitle sx={{ fontWeight: 700 }}>连接配置</DialogTitle>
      <DialogContent>
        <Stack spacing={2} sx={{ mt: 0.5 }}>
          <Alert severity="info">
            这是平台连接 MySQL 使用的配置。若你在平台外修改过密码或端口导致平台连不上数据库，在这里改成实际值即可恢复。
          </Alert>
          <TextField
            label="用户名"
            size="small"
            value={user}
            onChange={(e) => setUser(e.target.value)}
            fullWidth
          />
          <TextField
            label="密码"
            type="password"
            size="small"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            helperText="留空表示不修改密码；测试连接按此处输入实测"
            fullWidth
          />
          <TextField
            label="端口"
            size="small"
            value={port}
            onChange={(e) => setPort(e.target.value)}
            helperText="1-65535，默认 3306"
            fullWidth
          />
          {testResult && (
            <Alert severity={testResult.ok ? "success" : "error"} sx={{ whiteSpace: "pre-wrap", wordBreak: "break-all" }}>
              {testResult.text}
            </Alert>
          )}
          <Typography variant="caption" color="text.secondary">
            保存仅更新平台托管配置，不会修改 MySQL 本身；保存后点「刷新」验证连接。
          </Typography>
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button
          startIcon={<NetworkCheckIcon />}
          disabled={testing || busy}
          onClick={() => void runTest()}
          sx={btnSx}
        >
          {testing ? "测试中…" : "测试连接"}
        </Button>
        <Box sx={{ flex: 1 }} />
        <Button onClick={onClose} sx={btnSx}>
          取消
        </Button>
        <Button variant="contained" disabled={busy || testing} onClick={() => void submit()} sx={btnSx}>
          {busy ? "保存中…" : "保存"}
        </Button>
      </DialogActions>
    </Dialog>
  );
}
