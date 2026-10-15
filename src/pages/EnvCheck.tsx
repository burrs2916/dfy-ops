import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Alert,
  Badge,
  Box,
  Button,
  Checkbox,
  Chip,
  CircularProgress,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Divider,
  FormControlLabel,
  IconButton,
  Paper,
  Snackbar,
  Stack,
  Switch,
  Tab,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Tabs,
  TextField,
  Typography,
} from "@mui/material";
import { Check, ContentCopy, DeleteOutline, ExpandMore, Pause, PlayArrow, Refresh, SmartToy } from "@mui/icons-material";
import { api } from "../api";
import { useAppSettingsStore, useOpsStore } from "../store";
import type {
  CheckStatus,
  EnvCheckItem,
  EnvRepairJob,
  RepairPlan,
  ResourceScan,
  ServiceEnvReport,
  UninstallTargetStatus,
} from "../types";

const STATUS_COLOR: Record<CheckStatus, "success" | "warning" | "error"> = {
  pass: "success",
  warn: "warning",
  fail: "error",
};

const STATUS_LABEL: Record<CheckStatus, string> = {
  pass: "满足",
  warn: "警告",
  fail: "缺失",
};

const STATUS_HEX: Record<CheckStatus, string> = {
  pass: "#3B6D11",
  warn: "#BA7517",
  fail: "#A32D2D",
};

const ACK_KEY = "dfy-ops:ack";
function loadAck(): Set<string> {
  try {
    const raw = localStorage.getItem(ACK_KEY);
    return new Set(raw ? (JSON.parse(raw) as string[]) : []);
  } catch {
    return new Set();
  }
}
function saveAck(set: Set<string>) {
  try {
    localStorage.setItem(ACK_KEY, JSON.stringify([...set]));
  } catch {
    /* ignore */
  }
}
function ackId(reportId: string, itemId: string) {
  return `${reportId}::${itemId}`;
}

/** 与后端 ops/resource.rs 的 repair_target 口径一致：dep- 与 service- 前缀折叠后匹配。 */
const REPAIR_TARGETS = new Set(["java", "mysql", "redis", "vernemq", "minio", "greptime"]);
function repairTarget(checkId: string): string | null {
  const c = checkId.replace(/^dep-/, "").replace(/^service-/, "");
  return REPAIR_TARGETS.has(c) ? c : null;
}

const TARGET_LABEL: Record<string, string> = {
  java: "Java 运行时",
  mysql: "MySQL",
  redis: "Redis",
  vernemq: "VerneMQ",
  minio: "MinIO",
  greptime: "GreptimeDB",
};

/** 安装与维护卡片的展示顺序：用户重点的数据库/运行时在前 */
const SOFTWARE_ORDER = ["mysql", "java", "redis", "vernemq", "minio", "greptime"];
/** 一句话说清每个软件是干嘛的（卡片副标题，消灭抽象概念） */
const TARGET_DESC: Record<string, string> = {
  mysql: "数据库",
  java: "Java 运行时",
  redis: "缓存数据库",
  vernemq: "MQTT 消息队列",
  minio: "对象存储",
  greptime: "时序数据库",
};
/** 约定子目录名（与后端 convention_target 对齐）：无包时引导用户放到哪 */
const CONVENTION_DIR: Record<string, string> = {
  java: "JDK",
  mysql: "MySQL",
  redis: "Redis",
  vernemq: "VerneMQ",
  minio: "MinIO",
  greptime: "GreptimeDB",
};

/** 包体积 → 人读文案（KB/MB/GB，1 位小数）。 */
function fmtSize(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

function StatusDot({ status }: { status: CheckStatus }) {
  return (
    <Box
      sx={{
        width: 10,
        height: 10,
        borderRadius: "50%",
        bgcolor: STATUS_HEX[status],
        flexShrink: 0,
      }}
    />
  );
}

/** 顶部统计带的一个数字格。 */
function Stat({ value, label, color }: { value: number; label: string; color?: string }) {
  return (
    <Stack alignItems="center" sx={{ px: 3, py: 0.5 }}>
      <Typography variant="h5" sx={{ fontWeight: 700, color: color ?? "text.primary", lineHeight: 1.2 }}>
        {value}
      </Typography>
      <Typography variant="caption" color="text.secondary">
        {label}
      </Typography>
    </Stack>
  );
}

/** 修复任务阶段 → 用户可读文案。 */
const REPAIR_STAGE_LABEL: Record<string, string> = {
  download: "下载安装包…",
  install: "静默安装中（请在 UAC 弹窗确认管理员权限）…",
  extract: "解压安装包…",
  configure: "写配置并初始化…",
  backup: "备份 MySQL 数据（全库导出为 SQL 文件）…",
  uninstall: "卸载中（停止并注销服务）…",
  verify: "验证结果…",
  done: "已完成",
};

function ServiceEnvSection({
  report,
  acked,
  onToggleAck,
  onlyProblems,
  hideAcked,
  onRepair,
  repairDisabled,
}: {
  report: ServiceEnvReport;
  acked: Set<string>;
  onToggleAck: (id: string) => void;
  onlyProblems: boolean;
  hideAcked: boolean;
  /** 支持自动修复的检查项（java）回调；未提供则不显示按钮 */
  onRepair?: (serviceId: string, checkId: string) => void;
  repairDisabled?: boolean;
}) {
  const accent = STATUS_HEX[report.overall];

  const rows = useMemo(() => {
    return report.items.filter((it) => {
      const id = ackId(report.id, it.id);
      if (onlyProblems && it.status === "pass") return false;
      if (hideAcked && acked.has(id)) return false;
      return true;
    });
  }, [report.items, report.id, acked, onlyProblems, hideAcked]);

  return (
    <Paper variant="outlined" sx={{ borderLeft: `4px solid ${accent}`, overflow: "hidden" }}>
      {/* 分区头：服务名 + 总体结论 + 检测范围声明 */}
      <Box sx={{ px: 2, pt: 1.5, pb: 1 }}>
        <Stack direction="row" justifyContent="space-between" alignItems="center">
          <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
            {report.name}
          </Typography>
          <Chip
            size="small"
            label={STATUS_LABEL[report.overall]}
            color={STATUS_COLOR[report.overall]}
          />
        </Stack>
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.25 }}>
          检测范围（{report.items.length} 项）：{report.items.map((i) => i.name).join(" · ")}
        </Typography>
      </Box>
      <Divider />

      {/* 检测结果表：每行 = 一个检查项 */}
      <TableContainer>
        <Table size="small" sx={{ tableLayout: "fixed" }}>
          <TableHead>
            <TableRow sx={{ bgcolor: "rgba(0,0,0,0.03)" }}>
              <TableCell sx={{ width: 86 }}>状态</TableCell>
              <TableCell sx={{ width: 150 }}>检查项</TableCell>
              <TableCell>期望</TableCell>
              <TableCell>检测值</TableCell>
              <TableCell>修复建议</TableCell>
              <TableCell sx={{ width: 92 }}>操作</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {rows.length === 0 ? (
              <TableRow>
                <TableCell colSpan={6} align="center">
                  <Typography variant="caption" color="text.secondary">
                    无匹配项（可调整「仅看问题项 / 隐藏已知悉」过滤）
                  </Typography>
                </TableCell>
              </TableRow>
            ) : (
              rows.map((it) => {
                const id = ackId(report.id, it.id);
                const isAcked = acked.has(id);
                const hasHint = it.status !== "pass" && !!it.fixHint;
                return (
                  <TableRow
                    key={it.id}
                    sx={{ opacity: isAcked ? 0.55 : 1, "& td": { verticalAlign: "top", py: 1 } }}
                  >
                    {/* 状态 */}
                    <TableCell>
                      <Stack direction="row" spacing={0.75} alignItems="center">
                        <StatusDot status={it.status} />
                        <Typography variant="caption" sx={{ color: STATUS_HEX[it.status], fontWeight: 700 }}>
                          {STATUS_LABEL[it.status]}
                        </Typography>
                      </Stack>
                    </TableCell>
                    {/* 检查项 */}
                    <TableCell>
                      <Typography variant="body2" sx={{ fontWeight: 600 }}>
                        {it.name}
                      </Typography>
                      {it.status === "fail" && (
                        <Typography variant="caption" sx={{ color: "#A32D2D", fontWeight: 700, display: "block" }}>
                          阻断（需先修复）
                        </Typography>
                      )}
                    </TableCell>
                    {/* 期望 */}
                    <TableCell>
                      <Typography variant="body2" color="text.secondary">
                        {it.expected}
                      </Typography>
                    </TableCell>
                    {/* 检测值（用户最关心的"实际探到什么"） */}
                    <TableCell>
                      <Typography
                        variant="body2"
                        sx={{
                          fontFamily: "monospace",
                          color: STATUS_HEX[it.status],
                          wordBreak: "break-all",
                        }}
                      >
                        {it.actual}
                      </Typography>
                    </TableCell>
                    {/* 修复建议（可复制） */}
                    <TableCell>
                      {hasHint && !isAcked ? (
                        <Stack direction="row" spacing={0.5} alignItems="flex-start">
                          <Typography
                            variant="caption"
                            sx={{
                              fontFamily: "monospace",
                              color: "text.primary",
                              wordBreak: "break-all",
                              flex: 1,
                              minWidth: 0,
                            }}
                          >
                            {it.fixHint}
                          </Typography>
                          <IconButton
                            size="small"
                            title="复制修复命令"
                            onClick={() => navigator.clipboard?.writeText(it.fixHint)}
                          >
                            <ContentCopy fontSize="small" sx={{ color: "text.secondary" }} />
                          </IconButton>
                        </Stack>
                      ) : (
                        <Typography variant="caption" color="text.secondary">
                          {it.status === "pass" ? "—" : isAcked ? "已确认，无需处理" : "—"}
                        </Typography>
                      )}
                    </TableCell>
                    {/* 操作 */}
                    <TableCell>
                      {it.status !== "pass" && (
                        <Stack direction="row" spacing={0.5} alignItems="center">
                          {it.id === "java" && onRepair && (
                            <Button
                              size="small"
                              variant="contained"
                              color="warning"
                              disabled={repairDisabled}
                              onClick={() => onRepair(report.id, it.id)}
                              sx={{ textTransform: "none", px: 1 }}
                            >
                              一键修复
                            </Button>
                          )}
                          <Button
                            size="small"
                            color={isAcked ? "inherit" : "primary"}
                            onClick={() => onToggleAck(id)}
                            sx={{ textTransform: "none", px: 1 }}
                          >
                            {isAcked ? "撤销" : "已知悉"}
                          </Button>
                        </Stack>
                      )}
                    </TableCell>
                  </TableRow>
                );
              })
            )}
          </TableBody>
        </Table>
      </TableContainer>
    </Paper>
  );
}

/** 修复中心：问题项汇总 + 一键修复（资源包→在线）+ AI 方案（代执行/复制 + 追问）+ 已安装组件卸载。 */
function RepairCenter({
  pending,
  repairDisabled,
  repairRunning,
  activeTarget,
  onRepair,
  onUninstall,
}: {
  pending: { reportId: string; reportName: string; item: EnvCheckItem }[];
  repairDisabled: boolean;
  /** 修复/卸载任务是否在跑：结束后自动刷新安装痕迹与资源目录 */
  repairRunning: boolean;
  /** 正在执行任务的软件（卡片据此高亮"正在安装"）；null = 空闲 */
  activeTarget: string | null;
  onRepair: (target: string) => void;
  onUninstall: (target: string, removeData: boolean) => void;
}) {
  const resourceDir = useAppSettingsStore((s) => s.settings.resourceDir);
  const [scan, setScan] = useState<ResourceScan | null>(null);
  const [installed, setInstalled] = useState<UninstallTargetStatus[]>([]);
  const [plan, setPlan] = useState<RepairPlan | null>(null);
  const [planItems, setPlanItems] = useState<EnvCheckItem[]>([]);
  const [planning, setPlanning] = useState(false);
  const [planError, setPlanError] = useState<string | null>(null);
  const [followUp, setFollowUp] = useState("");
  // 代执行确认弹窗：仅保存待确认命令，确认后才发请求
  const [execCommand, setExecCommand] = useState<string | null>(null);
  const [executing, setExecuting] = useState(false);
  // 卸载确认弹窗：target 待卸载组件；removeData 默认不勾（数据库数据不可恢复）
  const [uninstallTarget, setUninstallTarget] = useState<UninstallTargetStatus | null>(null);
  const [removeData, setRemoveData] = useState(false);
  // MySQL 删数据强确认：勾选后必须手动输入 DELETE 才能点确认（防误删，业界惯例）
  const [confirmText, setConfirmText] = useState("");
  const mysqlDeleteConfirmOk =
    !(uninstallTarget?.target === "mysql" && removeData) || confirmText.trim() === "DELETE";
  const [snack, setSnack] = useState<{ open: boolean; msg: string; severity: "success" | "error" }>({
    open: false,
    msg: "",
    severity: "success",
  });
  // AI 方案「复制全部」的瞬时反馈
  const [planCopied, setPlanCopied] = useState(false);
  // 安装痕迹首次探测中（Windows 上 sc/注册表扫描需 1-2 秒，先如实显示加载态）
  const [probing, setProbing] = useState(true);
  // 子导航：「安装与维护」= 装软件主流程；「环境问题与 AI 方案」= 检测问题跟进
  const [subTab, setSubTab] = useState<"install" | "issues">("install");

  // 进入修复中心 / 目录配置变化 / 任务结束时：扫资源目录 + 探测安装痕迹
  useEffect(() => {
    if (!resourceDir) {
      setScan(null);
    } else {
      let alive = true;
      api
        .resources()
        .then((s) => {
          if (alive) setScan(s);
        })
        .catch(() => {
          if (alive) setScan(null);
        });
      return () => {
        alive = false;
      };
    }
  }, [resourceDir]);

  useEffect(() => {
    if (repairRunning) return; // 任务进行中不探（结束时会再刷）
    let alive = true;
    setProbing(true);
    api
      .uninstallStatus()
      .then((s) => {
        if (alive) {
          setInstalled(s.targets);
          setProbing(false);
        }
      })
      .catch(() => {
        if (alive) {
          setInstalled([]);
          setProbing(false);
        }
      });
    return () => {
      alive = false;
    };
  }, [repairRunning]);

  /** AI 方案：首次用当前问题项；追问时携带上一轮方案文本。 */
  const generatePlan = useCallback(
    async (items: EnvCheckItem[], previous?: string, question?: string) => {
      setPlanning(true);
      setPlanError(null);
      try {
        const p = await api.repairPlan(items, previous, question);
        if (!p.ok) {
          setPlanError(p.error || "生成方案失败");
        } else {
          setPlan(p);
          setPlanItems(items);
          setFollowUp("");
        }
      } catch (e) {
        setPlanError(e instanceof Error ? e.message : String(e));
      } finally {
        setPlanning(false);
      }
    },
    []
  );

  /** 上一轮方案的可读文本（追问时作为模型上下文）。 */
  const planAsText = useCallback(
    (p: RepairPlan) =>
      [p.summary, ...p.steps.map((s, i) => `${i + 1}. ${s.title}${s.detail ? `：${s.detail}` : ""}${s.command ? `（命令：${s.command}）` : ""}`)].join("\n"),
    []
  );

  const submitFollowUp = useCallback(() => {
    const q = followUp.trim();
    if (!q || !plan || planning) return; // planning 防重：思考中不允许再次提交
    void generatePlan(planItems.length ? planItems : pending.map((e) => e.item), planAsText(plan), q);
  }, [followUp, plan, planning, planItems, pending, generatePlan, planAsText]);

  const doExec = useCallback(async () => {
    if (!execCommand) return;
    setExecuting(true);
    try {
      const r = await api.repairExec(execCommand);
      setExecCommand(null);
      if (r.ok) {
        setSnack({ open: true, msg: "命令执行成功，请稍后点「重新检查」验证环境", severity: "success" });
      } else {
        setSnack({ open: true, msg: r.error || "命令执行失败", severity: "error" });
      }
    } catch (e) {
      setExecCommand(null);
      setSnack({
        open: true,
        msg: e instanceof Error ? e.message : String(e),
        severity: "error",
      });
    } finally {
      setExecuting(false);
    }
  }, [execCommand]);

  /** 卸载确认：removeData=true 时连数据目录一起删（不可恢复，弹窗里已显式勾选）。 */
  const doUninstall = useCallback(() => {
    if (!uninstallTarget) return;
    const t = uninstallTarget;
    setUninstallTarget(null);
    onUninstall(t.target, removeData);
  }, [uninstallTarget, removeData, onUninstall]);

  const identified = scan?.files.filter((f) => f.target) ?? [];
  // 每个目标的最佳包（与后端 find_zip/find_msi 取包口径一致：约定目录内优先，再比大小）
  const bestPkg = useCallback(
    (t: string) => {
      const hits = identified.filter((f) => f.target === t);
      if (hits.length === 0) return null;
      return hits.reduce((a, b) =>
        b.inConventionDir && !a.inConventionDir
          ? b
          : b.inConventionDir === a.inConventionDir && b.sizeBytes > a.sizeBytes
            ? b
            : a,
      );
    },
    [identified],
  );
  const installedOf = useCallback((t: string) => installed.find((x) => x.target === t), [installed]);

  // 远程部署声明：本机不装的软件（如 Redis/MinIO 远程部署），卡片转为说明行，
  // 不再显示"未安装/未找到"噪音；设置热更新 + 持久化
  const { settings, apply } = useAppSettingsStore();
  const remoteSet = useMemo(() => new Set(settings.remoteDeployed), [settings.remoteDeployed]);
  const toggleRemote = useCallback(
    async (t: string, toRemote: boolean) => {
      const next = toRemote
        ? [...settings.remoteDeployed, t]
        : settings.remoteDeployed.filter((x) => x !== t);
      const nextSettings = { ...settings, remoteDeployed: next };
      apply(nextSettings); // 乐观更新：界面立即切换
      try {
        await api.saveAppSettings(nextSettings);
      } catch {
        apply(settings); // 保存失败回滚
      }
    },
    [settings, apply],
  );

  // 远程部署分区默认折叠（声明过远程的软件不在主视线里）
  const [remoteOpen, setRemoteOpen] = useState(false);

  /** 软件卡片：状态机（执行中 > 已安装 > 残留 > 有包 > 无包）。
   *  探测（probing）不阻塞渲染——已装状态未知时按待安装显示，结果回来自动归位，
   *  避免进入页面 6 张卡集体转圈的"慢"体感。 */
  const renderCard = (t: string) => {
    const st = installedOf(t);
    const pkg = bestPkg(t);
    const runningThis = repairRunning && activeTarget === t;
    const conventionHint = `${resourceDir || "<资源目录>"}\\${CONVENTION_DIR[t]}\\`;

    let statusLine: React.ReactNode;
    let actions: React.ReactNode = null;
    if (runningThis) {
      statusLine = (
        <Stack direction="row" spacing={1} alignItems="center">
          <CircularProgress size={16} />
          <Typography variant="body2" sx={{ fontWeight: 600 }}>
            正在执行操作，进度见上方提示…
          </Typography>
        </Stack>
      );
    } else if (st?.serviceInstalled) {
      statusLine = (
        <Stack direction="row" spacing={1} alignItems="center" sx={{ minWidth: 0 }}>
          <Chip size="small" color="success" label="已安装" />
          <Typography variant="caption" sx={{ fontFamily: "monospace", color: "text.secondary", wordBreak: "break-all" }}>
            {st.installDir || "由 Windows Installer 管理"}
          </Typography>
        </Stack>
      );
      actions = (
        <>
          <Button size="small" variant="outlined" disabled={repairDisabled} onClick={() => onRepair(t)} sx={{ textTransform: "none", whiteSpace: "nowrap" }}>
            重装
          </Button>
          <Button
            size="small"
            variant="outlined"
            color="error"
            startIcon={<DeleteOutline />}
            disabled={!st.canUninstall || repairDisabled}
            onClick={() => {
              setRemoveData(false);
              setConfirmText("");
              setUninstallTarget(st);
            }}
            sx={{ textTransform: "none", whiteSpace: "nowrap" }}
          >
            卸载
          </Button>
        </>
      );
    } else if (st?.dirExists) {
      statusLine = (
        <Stack direction="row" spacing={1} alignItems="center" sx={{ minWidth: 0 }}>
          <Chip size="small" color="warning" label="残留目录" />
          <Typography variant="caption" sx={{ fontFamily: "monospace", color: "text.secondary", wordBreak: "break-all" }}>
            {st.installDir}（未注册服务，可清理后重装）
          </Typography>
        </Stack>
      );
      actions = (
        <Button
          size="small"
          variant="outlined"
          color="error"
          startIcon={<DeleteOutline />}
          disabled={!st.canUninstall || repairDisabled}
          onClick={() => {
            setRemoveData(false);
            setConfirmText("");
            setUninstallTarget(st);
          }}
          sx={{ textTransform: "none", whiteSpace: "nowrap" }}
        >
          清理
        </Button>
      );
    } else if (pkg) {
      statusLine = (
        <Stack direction="row" spacing={1} alignItems="center" sx={{ minWidth: 0 }}>
          <Chip size="small" variant="outlined" label="未安装" />
          <Typography variant="body2" color="text.secondary" sx={{ wordBreak: "break-all" }}>
            已识别安装包 <b>{pkg.fileName}</b>（{fmtSize(pkg.sizeBytes)}{pkg.inConventionDir ? "，约定目录" : ""}）
          </Typography>
        </Stack>
      );
      actions = (
        <Button size="small" variant="contained" disabled={repairDisabled} onClick={() => onRepair(t)} sx={{ textTransform: "none", whiteSpace: "nowrap" }}>
          一键安装
        </Button>
      );
    } else {
      statusLine = (
        <Stack direction="row" spacing={1} alignItems="center" sx={{ minWidth: 0 }}>
          <Chip size="small" variant="outlined" label="未安装" />
          <Typography variant="body2" color="text.secondary" sx={{ wordBreak: "break-word" }}>
            {t === "java"
              ? "未识别到安装包：可在资源目录 JDK\\ 放入 zip（免安装），或点「一键安装」在线下载（需外网）"
              : `未识别到安装包：把安装包放入 ${conventionHint} 子目录后这里会自动出现`}
          </Typography>
        </Stack>
      );
      actions = t === "java" ? (
        <Button size="small" variant="contained" disabled={repairDisabled} onClick={() => onRepair(t)} sx={{ textTransform: "none", whiteSpace: "nowrap" }}>
          一键安装（在线）
        </Button>
      ) : null;
    }

    return (
      <Paper
        key={t}
        variant="outlined"
        sx={{
          px: 2,
          py: 1.5,
          ...(runningThis ? { borderColor: "primary.main", bgcolor: "action.hover" } : {}),
        }}
      >
        <Stack direction="row" spacing={2} alignItems="center" sx={{ flexWrap: "wrap" }}>
          <Box sx={{ width: 168, flexShrink: 0 }}>
            <Typography variant="subtitle1" sx={{ fontWeight: 700, lineHeight: 1.3 }}>
              {TARGET_LABEL[t] ?? t}
            </Typography>
            <Typography variant="caption" color="text.secondary">
              {TARGET_DESC[t] ?? ""}
            </Typography>
          </Box>
          <Box sx={{ flex: 1, minWidth: 260 }}>{statusLine}</Box>
          <Stack direction="row" spacing={1} alignItems="center" sx={{ flexShrink: 0 }}>
            {actions}
          </Stack>
        </Stack>
      </Paper>
    );
  };

  /** 远程部署卡片：灰色说明行 + 改回本地的入口。 */
  const renderRemoteCard = (t: string) => (
    <Paper key={t} variant="outlined" sx={{ px: 2, py: 1.25, bgcolor: "action.hover" }}>
      <Stack direction="row" spacing={2} alignItems="center" sx={{ flexWrap: "wrap" }}>
        <Box sx={{ width: 168, flexShrink: 0 }}>
          <Typography variant="subtitle1" sx={{ fontWeight: 700, lineHeight: 1.3, color: "text.secondary" }}>
            {TARGET_LABEL[t] ?? t}
          </Typography>
          <Typography variant="caption" color="text.secondary">
            {TARGET_DESC[t] ?? ""}
          </Typography>
        </Box>
        <Box sx={{ flex: 1, minWidth: 260 }}>
          <Typography variant="body2" color="text.secondary">
            本机无需安装（按部署规划远程部署）
          </Typography>
        </Box>
        <Stack direction="row" spacing={1} alignItems="center" sx={{ flexShrink: 0 }}>
          <Button size="small" variant="text" disabled={repairRunning} onClick={() => void toggleRemote(t, false)} sx={{ textTransform: "none", whiteSpace: "nowrap" }}>
            改回本地安装
          </Button>
        </Stack>
      </Stack>
    </Paper>
  );

  return (
    <Stack spacing={2}>
      <Tabs value={subTab} onChange={(_, v: "install" | "issues") => setSubTab(v)}>
        <Tab value="install" label="安装与维护" />
        <Tab
          value="issues"
          label={
            <Badge color="error" badgeContent={pending.length > 0 ? pending.length : undefined}>
              环境问题与 AI 方案
            </Badge>
          }
        />
      </Tabs>

      {/* ===== 安装与维护：待安装 / 已安装 / 远程部署 三分区，探测后台完成不阻塞 ===== */}
      {subTab === "install" && (
        <>
          {!resourceDir ? (
            <Alert severity="warning">
              先配置「本地资源目录」（设置 → 资源目录，可用「浏览…」选择）：把安装包放进
              <b>以软件名命名的子目录</b>（如 <b>MySQL\</b>、<b>JDK\</b>），这里就会识别并出现「一键安装」按钮。
            </Alert>
          ) : scan && !scan.exists ? (
            <Alert severity="error">资源目录（{resourceDir}）不存在或不可访问，请到「设置 → 资源目录」重新选择。</Alert>
          ) : null}

          {(() => {
            const toInstall = SOFTWARE_ORDER.filter(
              (t) => !remoteSet.has(t) && !installedOf(t)?.serviceInstalled,
            );
            const installedList = SOFTWARE_ORDER.filter(
              (t) => !remoteSet.has(t) && installedOf(t)?.serviceInstalled,
            );
            const remoteList = SOFTWARE_ORDER.filter((t) => remoteSet.has(t));

            return (
              <>
                {/* 待安装：一键安装的主战场 */}
                <Stack spacing={1}>
                  <Stack direction="row" spacing={1} alignItems="center">
                    <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
                      待安装
                    </Typography>
                    {probing && (
                      <>
                        <CircularProgress size={12} />
                        <Typography variant="caption" color="text.secondary">
                          正在检测安装状态…
                        </Typography>
                      </>
                    )}
                  </Stack>
                  {toInstall.length === 0 ? (
                    <Typography variant="body2" color="text.secondary">
                      没有待安装的软件
                    </Typography>
                  ) : (
                    toInstall.map((t) => renderCard(t))
                  )}
                </Stack>

                {/* 已安装：重装 / 卸载的维护区 */}
                <Stack spacing={1}>
                  <Stack direction="row" spacing={1} alignItems="center">
                    <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
                      已安装
                    </Typography>
                    {probing && (
                      <>
                        <CircularProgress size={12} />
                        <Typography variant="caption" color="text.secondary">
                          正在检测…
                        </Typography>
                      </>
                    )}
                  </Stack>
                  {installedList.length === 0 ? (
                    <Typography variant="body2" color="text.secondary">
                      暂无（安装完成后软件会出现在这里）
                    </Typography>
                  ) : (
                    installedList.map((t) => renderCard(t))
                  )}
                </Stack>

                {/* 远程部署：默认折叠，不占主视线 */}
                {remoteList.length > 0 && (
                  <Box>
                    <Stack
                      direction="row"
                      spacing={1}
                      alignItems="center"
                      onClick={() => setRemoteOpen((v) => !v)}
                      sx={{ cursor: "pointer", userSelect: "none", py: 0.5 }}
                    >
                      <ExpandMore
                        sx={{
                          fontSize: 20,
                          transition: "transform 0.2s",
                          transform: remoteOpen ? "rotate(0deg)" : "rotate(-90deg)",
                          color: "text.secondary",
                        }}
                      />
                      <Typography variant="subtitle1" sx={{ fontWeight: 700, color: "text.secondary" }}>
                        远程部署（{remoteList.length}）
                      </Typography>
                      <Typography variant="caption" color="text.secondary">
                        {remoteList.map((t) => TARGET_LABEL[t] ?? t).join("、")} · 本机无需安装
                      </Typography>
                    </Stack>
                    {remoteOpen && (
                      <Stack spacing={1} sx={{ mt: 1 }}>
                        {remoteList.map((t) => renderRemoteCard(t))}
                      </Stack>
                    )}
                  </Box>
                )}
              </>
            );
          })()}

          {scan?.misplaced && scan.misplaced.length > 0 && (
            <Alert severity="warning" icon={false}>
              {scan.misplaced.map((m) => (
                <Typography key={m.path} variant="body2" component="div">
                  「{m.fileName}」放在了 {m.foundIn}/ 目录，但它属于 {m.fileTarget.toUpperCase()} ——建议移到对应的约定子目录（按软件名命名）
                </Typography>
              ))}
            </Alert>
          )}
        </>
      )}

      {/* ===== 环境问题与 AI 方案 ===== */}
      {subTab === "issues" && (
        <>
          {planError && <Alert severity="error">{planError}</Alert>}

      {/* 问题项汇总 */}
      <Paper variant="outlined" sx={{ overflow: "hidden" }}>
        <Stack direction="row" justifyContent="space-between" alignItems="center" sx={{ px: 2, pt: 1.5, pb: 1 }}>
          <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
            待处理问题项（{pending.length}）
          </Typography>
          <Button
            size="small"
            variant="contained"
            startIcon={planning ? <CircularProgress size={14} color="inherit" /> : <SmartToy />}
            disabled={planning || pending.length === 0}
            onClick={() => void generatePlan(pending.map((e) => e.item))}
          >
            {planning ? "生成中…" : "AI 生成修复方案"}
          </Button>
        </Stack>
        <Divider />
        <TableContainer>
          <Table size="small" sx={{ tableLayout: "fixed" }}>
            <TableHead>
              <TableRow sx={{ bgcolor: "rgba(0,0,0,0.03)" }}>
                <TableCell sx={{ width: 86 }}>状态</TableCell>
                <TableCell sx={{ width: 210 }}>检查项</TableCell>
                <TableCell>检测值</TableCell>
                <TableCell sx={{ width: 110 }}>操作</TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {pending.length === 0 ? (
                <TableRow>
                  <TableCell colSpan={4} align="center">
                    <Typography variant="caption" color="text.secondary">
                      没有待处理的环境问题
                    </Typography>
                  </TableCell>
                </TableRow>
              ) : (
                pending.map(({ reportId, reportName, item }) => {
                  const target = repairTarget(item.id);
                  return (
                    <TableRow key={`${reportId}::${item.id}`} sx={{ "& td": { verticalAlign: "top", py: 1 } }}>
                      <TableCell>
                        <Stack direction="row" spacing={0.75} alignItems="center">
                          <StatusDot status={item.status} />
                          <Typography variant="caption" sx={{ color: STATUS_HEX[item.status], fontWeight: 700 }}>
                            {STATUS_LABEL[item.status]}
                          </Typography>
                        </Stack>
                      </TableCell>
                      <TableCell>
                        <Typography variant="body2" sx={{ fontWeight: 600 }}>
                          {item.name}
                        </Typography>
                        <Typography variant="caption" color="text.secondary">
                          {reportName}
                        </Typography>
                      </TableCell>
                      <TableCell>
                        <Typography variant="body2" sx={{ fontFamily: "monospace", wordBreak: "break-all", color: "text.secondary" }}>
                          {item.actual}
                        </Typography>
                      </TableCell>
                      <TableCell>
                        {target ? (
                          <Button
                            size="small"
                            variant="contained"
                            color="warning"
                            disabled={repairDisabled}
                            onClick={() => onRepair(target)}
                            sx={{ textTransform: "none", px: 1 }}
                          >
                            一键修复
                          </Button>
                        ) : (
                          <Typography variant="caption" color="text.secondary">
                            AI 方案 / 手动
                          </Typography>
                        )}
                      </TableCell>
                    </TableRow>
                  );
                })
              )}
            </TableBody>
          </Table>
        </TableContainer>
      </Paper>

      {/* AI 方案卡片 */}
      {plan && (
        <Paper variant="outlined" sx={{ overflow: "hidden" }}>
          <Stack direction="row" justifyContent="space-between" alignItems="center" sx={{ px: 2, pt: 1.5 }}>
            <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
              AI 修复方案{plan.mode === "text" ? "（原始建议）" : ""}
            </Typography>
            <Stack direction="row" spacing={1} alignItems="center">
              <Chip size="small" label={plan.mode === "ai" ? "结构化步骤" : "文本建议"} color="primary" variant="outlined" />
              {/* 全文复制：方案多步时避免逐条点复制 */}
              <Button
                size="small"
                startIcon={planCopied ? <Check /> : <ContentCopy />}
                onClick={() => {
                  navigator.clipboard?.writeText(planAsText(plan));
                  setPlanCopied(true);
                  setTimeout(() => setPlanCopied(false), 1500);
                }}
                sx={{ textTransform: "none" }}
              >
                {planCopied ? "已复制" : "复制全部"}
              </Button>
            </Stack>
          </Stack>
          <Typography variant="body2" sx={{ px: 2, pt: 0.5, whiteSpace: "pre-wrap", wordBreak: "break-word" }}>
            {plan.summary}
          </Typography>
          {plan.steps.map((s, i) => (
            <Box key={i} sx={{ px: 2, py: 1 }}>
              <Stack direction="row" spacing={1} alignItems="center">
                <Typography variant="body2" sx={{ fontWeight: 700, flex: 1, minWidth: 0 }}>
                  {i + 1}. {s.title}
                </Typography>
                {s.command &&
                  (s.isExecutable ? (
                    <Button
                      size="small"
                      variant="contained"
                      color="warning"
                      onClick={() => setExecCommand(s.command)}
                      sx={{ textTransform: "none" }}
                    >
                      代执行
                    </Button>
                  ) : (
                    <IconButton
                      size="small"
                      title="复制命令（白名单外仅支持手动执行）"
                      onClick={() => navigator.clipboard?.writeText(s.command!)}
                    >
                      <ContentCopy fontSize="small" sx={{ color: "text.secondary" }} />
                    </IconButton>
                  ))}
              </Stack>
              {s.detail && (
                <Typography variant="body2" color="text.secondary" sx={{ mt: 0.25, whiteSpace: "pre-wrap" }}>
                  {s.detail}
                </Typography>
              )}
              {s.command && (
                <Typography
                  variant="caption"
                  sx={{ display: "block", mt: 0.5, fontFamily: "monospace", wordBreak: "break-all", color: "text.primary" }}
                >
                  {s.command}
                </Typography>
              )}
            </Box>
          ))}
          <Divider />
          <Stack direction="row" spacing={1} sx={{ px: 2, py: 1.5 }} alignItems="center">
            <TextField
              size="small"
              fullWidth
              placeholder="对方案有疑问？输入追问，AI 将基于当前方案继续作答"
              value={followUp}
              onChange={(e) => setFollowUp(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && followUp.trim() && !planning) submitFollowUp();
              }}
            />
            <Button
              size="small"
              variant="outlined"
              disabled={planning || !followUp.trim()}
              onClick={submitFollowUp}
              sx={{ textTransform: "none", whiteSpace: "nowrap" }}
            >
              {planning ? "思考中…" : "追问"}
            </Button>
          </Stack>
        </Paper>
      )}
        </>
      )}

      {/* 代执行确认弹窗：白名单命令 + UAC 提权，执行前必须人工确认 */}
      <Dialog open={!!execCommand} onClose={() => !executing && setExecCommand(null)} maxWidth="sm" fullWidth>
        <DialogTitle>确认代执行命令</DialogTitle>
        <DialogContent>
          <Typography variant="body2" color="text.secondary">
            该命令将在本机以管理员权限执行（请在随后弹出的 UAC 窗口确认）：
          </Typography>
          <Paper variant="outlined" sx={{ p: 1.5, mt: 1, bgcolor: "action.hover" }}>
            <Typography variant="body2" sx={{ fontFamily: "monospace", wordBreak: "break-all" }}>
              {execCommand}
            </Typography>
          </Paper>
          <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 1 }}>
            仅白名单程序（msiexec / winget / choco / sc / net / netsh / robocopy / xcopy / curl）的单条命令可代执行；复杂操作请复制到终端手动执行。
          </Typography>
        </DialogContent>
        <DialogActions>
          <Button size="small" onClick={() => setExecCommand(null)} disabled={executing}>
            取消
          </Button>
          <Button size="small" variant="contained" onClick={doExec} disabled={executing}>
            {executing ? "执行中…" : "执行"}
          </Button>
        </DialogActions>
      </Dialog>

      {/* 卸载确认弹窗：删数据必须显式勾选（数据库数据不可恢复）；java 走 MSI 静默卸载，无数据选项 */}
      <Dialog open={!!uninstallTarget} onClose={() => setUninstallTarget(null)} maxWidth="sm" fullWidth>
        <DialogTitle>卸载 {uninstallTarget?.label}</DialogTitle>
        <DialogContent>
          {uninstallTarget?.target === "java" ? (
            <>
              <Typography variant="body2">
                将执行<b>完全清理</b>（一次管理员确认）：枚举本机以 <b>MSI 方式安装</b>的全部 Java/JDK 条目（Temurin/OpenJDK 等）静默卸载；
                删除平台免安装目录 <b>C:\services\jdk</b>；清除用户级 <b>JAVA_HOME</b> 环境变量。卸载后系统零残留。
              </Typography>
              <Typography variant="body2" sx={{ mt: 1, color: "text.secondary" }}>
                卸载完成后已开终端里的 JAVA_HOME/PATH 是旧值，重开终端或重新登录后生效。
              </Typography>
            </>
          ) : (
            <>
              <Typography variant="body2">
                将停止并注销 Windows 服务
                <b>{uninstallTarget ? `（${uninstallTarget.target === "mysql" ? "MySQL80" : "Redis"}）` : ""}</b>，
                安装目录：
              </Typography>
              <Paper variant="outlined" sx={{ p: 1.5, mt: 1, bgcolor: "action.hover" }}>
                <Typography variant="body2" sx={{ fontFamily: "monospace", wordBreak: "break-all" }}>
                  {uninstallTarget?.installDir}
                </Typography>
              </Paper>
              <FormControlLabel
                sx={{ mt: 1.5, alignItems: "flex-start" }}
                control={
                  <Checkbox
                    checked={removeData}
                    onChange={(e) => {
                      setRemoveData(e.target.checked);
                      if (!e.target.checked) setConfirmText("");
                    }}
                    sx={{ pt: 0.25 }}
                  />
                }
                label={
                  <Typography variant="body2" color={removeData ? "error" : "text.secondary"}>
                    同时删除程序与数据目录（<b>数据库内的所有数据将不可恢复！</b>
                    不勾选则仅注销服务并保留数据，重装可复用）
                  </Typography>
                }
              />
              {uninstallTarget?.target === "mysql" && removeData && (
                <>
                  <Alert severity="info" sx={{ mt: 1 }}>
                    删除前将自动把<b>全部数据库（表结构 + 数据 + 例程）</b>备份为单个 SQL 文件，
                    保存到平台工作目录 <b>backups\</b> 下；<b>备份失败会中止删除</b>，数据分毫不动。
                  </Alert>
                  <TextField
                    size="small"
                    fullWidth
                    sx={{ mt: 1.5 }}
                    label='输入 DELETE 以确认删除数据'
                    placeholder="DELETE"
                    value={confirmText}
                    onChange={(e) => setConfirmText(e.target.value)}
                    error={confirmText.trim().length > 0 && confirmText.trim() !== "DELETE"}
                    helperText={
                      confirmText.trim().length > 0 && confirmText.trim() !== "DELETE"
                        ? "输入不匹配：请输入大写 DELETE"
                        : "这一步确认后，删除才真正开始执行"
                    }
                  />
                </>
              )}
            </>
          )}
        </DialogContent>
        <DialogActions>
          <Button size="small" onClick={() => setUninstallTarget(null)}>
            取消
          </Button>
          <Button size="small" variant="contained" color="error" onClick={doUninstall} disabled={!mysqlDeleteConfirmOk}>
            确认卸载
          </Button>
        </DialogActions>
      </Dialog>

      <Snackbar
        open={snack.open}
        autoHideDuration={4000}
        onClose={() => setSnack((s) => ({ ...s, open: false }))}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
      >
        <Alert severity={snack.severity} variant="filled" onClose={() => setSnack((s) => ({ ...s, open: false }))}>
          {snack.msg}
        </Alert>
      </Snackbar>
    </Stack>
  );
}

export default function EnvCheck() {
  // 数据来自全局 store：App 启动时 bootstrap() 已检测一次（store 级去重），
  // 切换页面返回本页直接渲染缓存，不再触发全量重探（此前每次 mount 都全量重探，
  // Windows 上一次要十几秒）
  const { envMap, loading, error, lastUpdate, bootstrap, refreshEnvAll } =
    useOpsStore();
  const [tab, setTab] = useState<"check" | "repair">("check");
  const [refreshing, setRefreshing] = useState(false);
  const [copiedAll, setCopiedAll] = useState(false);
  const [acked, setAcked] = useState<Set<string>>(() => loadAck());
  const [onlyProblems, setOnlyProblems] = useState(false);
  const [hideAcked, setHideAcked] = useState(false);
  // 轮询开关默认关闭：默认策略「启动检测一次 + 按需手动复检」，与总览一致
  const [live, setLive] = useState(false);

  // ===== 环境修复（java 分级链路 + 通用资源包/在线链路，共用同一任务状态） =====
  const [repairJob, setRepairJob] = useState<EnvRepairJob | null>(null);
  const [repairNotice, setRepairNotice] = useState<string | null>(null);
  const [repairError, setRepairError] = useState<string | null>(null);
  const repairPollRef = useRef<number | null>(null);

  const reports = useMemo(() => Object.values(envMap), [envMap]);

  useEffect(() => {
    void bootstrap();
  }, [bootstrap]);

  // 实时刷新（默认关闭）：按「设置 → 通用」配置的间隔强制复探（force 跳过
  // 后端 60s 缓存），使修复后状态自动反映。间隔 0 时不轮询。
  const envRefreshMs = useAppSettingsStore((s) => s.settings.envRefreshSecs) * 1000;
  useEffect(() => {
    if (!live || envRefreshMs <= 0) return;
    const id = window.setInterval(() => {
      void refreshEnvAll();
    }, envRefreshMs);
    return () => window.clearInterval(id);
  }, [live, refreshEnvAll, envRefreshMs]);
  // 「实时中」只在轮询真正生效时显示：间隔为 0（默认关闭）时如实显示已暂停
  const pollingActive = live && envRefreshMs > 0;

  // 手动「重新检查」：强制重探——用户刚修完环境点复检，必须拿到新结论
  const [refreshError, setRefreshError] = useState<string | null>(null);
  const manualRefresh = useCallback(async () => {
    setRefreshing(true);
    setRefreshError(null);
    const ok = await refreshEnvAll();
    if (!ok) {
      setRefreshError("环境检测请求失败");
    }
    setRefreshing(false);
  }, [refreshEnvAll]);

  const toggleAck = useCallback((id: string) => {
    setAcked((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      saveAck(next);
      return next;
    });
  }, []);

  // 卸载时停止修复轮询
  useEffect(() => {
    return () => {
      if (repairPollRef.current) window.clearInterval(repairPollRef.current);
    };
  }, []);

  /** 修复任务轮询（java 与通用修复共用）：任务结束后强制复探让新环境生效。 */
  const pollRepair = useCallback(() => {
    if (repairPollRef.current) window.clearInterval(repairPollRef.current);
    repairPollRef.current = window.setInterval(async () => {
      try {
        const job = await api.envRepairStatus();
        setRepairJob(job);
        if (!job.running) {
          window.clearInterval(repairPollRef.current!);
          repairPollRef.current = null;
          if (job.ok) {
            const label = job.target ? TARGET_LABEL[job.target] ?? job.target : "环境";
            // 任务结束已自动 refreshEnvAll()，如实告知用户结果已刷新
            setRepairNotice(`${label} ${job.stage === "uninstall" ? "卸载" : "安装"}完成，环境已自动重新检查`);
          } else {
            setRepairError(job.error || "修复未成功，请查看日志或按修复建议手动处理");
          }
          void refreshEnvAll();
        }
      } catch {
        /* 单次轮询失败忽略，下一轮重试 */
      }
    }, 2000);
  }, [refreshEnvAll]);

  /** 检测表里的「一键修复」（java）：分级响应——无需修复/配置指引/自动安装。 */
  const startRepair = useCallback(
    async (serviceId: string, checkId: string) => {
      setRepairError(null);
      setRepairNotice(null);
      try {
        const res = await api.startEnvRepair(serviceId, checkId);
        if (!res.ok) {
          setRepairError(res.error || "修复启动失败");
          return;
        }
        setRepairNotice(res.message || null);
        if (res.action === "install") pollRepair();
      } catch (e) {
        setRepairError(e instanceof Error ? e.message : String(e));
      }
    },
    [pollRepair]
  );

  /** 修复中心的「一键修复」：资源包优先 → java 在线兜底 → 无包提示。 */
  const startGenericRepair = useCallback(
    async (target: string) => {
      setRepairError(null);
      setRepairNotice(null);
      try {
        const res = await api.startGenericRepair(target);
        if (!res.ok) {
          setRepairError(res.error || "修复启动失败");
          return;
        }
        setRepairNotice(res.message || null);
        pollRepair();
      } catch (e) {
        setRepairError(e instanceof Error ? e.message : String(e));
      }
    },
    [pollRepair]
  );

  /** 修复中心「卸载」：停服务/注销服务（可选删数据目录），任务状态与安装共用。 */
  const startUninstall = useCallback(
    async (target: string, removeData: boolean) => {
      setRepairError(null);
      setRepairNotice(null);
      try {
        const res = await api.startUninstall(target, removeData);
        if (!res.ok) {
          setRepairError(res.error || "卸载启动失败");
          return;
        }
        setRepairNotice(res.message || null);
        pollRepair();
      } catch (e) {
        setRepairError(e instanceof Error ? e.message : String(e));
      }
    },
    [pollRepair]
  );

  // 待处理项 = 未通过且未被「已知悉」的项目；此为运维真正要跟进的清单。
  const pending = useMemo(
    () =>
      reports.flatMap((r) =>
        r.items.filter(
          (it) => it.status !== "pass" && !acked.has(ackId(r.id, it.id))
        )
      ),
    [reports, acked]
  );
  // 修复中心的行数据（带服务名）
  const pendingEntries = useMemo(
    () =>
      reports.flatMap((r) =>
        r.items
          .filter((it) => it.status !== "pass" && !acked.has(ackId(r.id, it.id)))
          .map((item) => ({ reportId: r.id, reportName: r.name, item }))
      ),
    [reports, acked]
  );
  const fail = pending.filter((it) => it.status === "fail").length;
  const warn = pending.filter((it) => it.status === "warn").length;
  const allItems = reports.flatMap((r) => r.items);
  const totalCount = allItems.length;
  const passCount = allItems.filter((it) => it.status === "pass").length;
  const ackedCount = reports
    .flatMap((r) => r.items.map((it) => ({ r, it })))
    .filter(({ r, it }) => it.status !== "pass" && acked.has(ackId(r.id, it.id))).length;

  // 聚合所有待处理缺失项的 Windows 修复命令，供用户在运维中心一次性复制执行。
  const allFixes = pending
    .filter((it) => it.fixHint)
    .map((it) => {
      const r = reports.find((rr) => rr.items.includes(it))!;
      return `# [${r.name}] ${it.name}\n${it.fixHint}`;
    })
    .join("\n\n");

  return (
    <Stack spacing={2}>
      {/* 头部 */}
      <Stack direction="row" justifyContent="space-between" alignItems="flex-start">
        <Box>
          <Typography variant="h5" sx={{ fontWeight: 700 }}>
            运维中心 · 运行环境
          </Typography>
          <Typography variant="body2" color="text.secondary" sx={{ mt: 0.25 }}>
            仅检测运行时依赖（不含构建工具）· 部署产物已预置，此处验证各服务"能否跑起来"
          </Typography>
        </Box>
        <Stack direction="row" spacing={1} alignItems="center">
          <Button
            variant="outlined"
            color={pollingActive ? "success" : "inherit"}
            startIcon={live ? <Pause /> : <PlayArrow />}
            onClick={() => setLive((v) => !v)}
          >
            {pollingActive ? "实时中" : "已暂停"}
          </Button>
          <Button
            variant="contained"
            startIcon={refreshing ? <CircularProgress size={16} color="inherit" /> : <Refresh />}
            onClick={manualRefresh}
            disabled={refreshing || loading}
          >
            重新检查
          </Button>
        </Stack>
      </Stack>
      <Typography variant="caption" color="text.secondary">
        实时刷新{pollingActive ? "中" : "已暂停"} · 上次更新{" "}
        {lastUpdate ? lastUpdate.toLocaleTimeString("zh-CN") : "—"}
      </Typography>

      {(error || refreshError) && (
        <Alert severity="error">
          {error || refreshError}（请确认运维后端已在 127.0.0.1:18721 运行）
        </Alert>
      )}

      {/* 修复任务进行中：阶段 + 最近日志（两个 Tab 共享，随时可见） */}
      {repairJob?.running && (
        <Alert
          severity="info"
          icon={<CircularProgress size={18} color="inherit" />}
          sx={{ "& .MuiAlert-message": { width: "100%" } }}
        >
          <Typography variant="body2" sx={{ fontWeight: 600 }}>
            {repairJob.target ? `正在修复 ${TARGET_LABEL[repairJob.target] ?? repairJob.target}：` : ""}
            {REPAIR_STAGE_LABEL[repairJob.stage] ?? repairJob.stage}
          </Typography>
          {/* 最近 3 条：过程可追溯（解压 → 配置 → 注册 → 验证各阶段的关键动作） */}
          {repairJob.logs.length > 0 &&
            repairJob.logs.slice(-3).map((log, i, arr) => (
              <Typography
                key={`${repairJob.logs.length}-${i}`}
                variant="caption"
                sx={{
                  fontFamily: "monospace",
                  display: "block",
                  mt: i === 0 && arr.length === 3 ? 0.5 : 0.25,
                  wordBreak: "break-all",
                  opacity: i === arr.length - 1 ? 1 : 0.65,
                }}
              >
                {log}
              </Typography>
            ))}
        </Alert>
      )}

      {/* 修复结果提示（无需修复 / 配置指引 / 安装完成） */}
      {!repairJob?.running && repairNotice && <Alert severity="success">{repairNotice}</Alert>}
      {!repairJob?.running && repairError && <Alert severity="error">{repairError}</Alert>}

      <Tabs value={tab} onChange={(_, v: "check" | "repair") => setTab(v)}>
        <Tab value="check" label="检测结果" />
        <Tab
          value="repair"
          label={
            <Badge color="error" badgeContent={fail + warn > 0 ? fail + warn : undefined}>
              修复中心
            </Badge>
          }
        />
      </Tabs>

      {tab === "check" && (
        <>
          {!loading && reports.length > 0 && (fail > 0 || warn > 0) && (
            <Alert
              severity={fail > 0 ? "error" : "warning"}
              action={
                <Button
                  color="inherit"
                  size="small"
                  disabled={!allFixes}
                  startIcon={copiedAll ? <Check /> : <ContentCopy />}
                  onClick={() => {
                    navigator.clipboard?.writeText(allFixes);
                    setCopiedAll(true);
                    setTimeout(() => setCopiedAll(false), 1500);
                  }}
                >
                  {copiedAll ? "已复制" : "复制全部修复命令"}
                </Button>
              }
            >
              待处理环境缺失 <b>{fail + warn}</b> 项（致命 {fail} / 警告 {warn}）。
              可到「修复中心」一键修复或生成 AI 方案。
            </Alert>
          )}

          {!loading && reports.length > 0 && (
            <>
              {/* 统计带 */}
              <Paper variant="outlined" sx={{ px: 2, py: 1.5 }}>
                <Stack direction="row" alignItems="center" flexWrap="wrap" useFlexGap>
                  <Stat value={totalCount} label="检测项总数" />
                  <Divider orientation="vertical" flexItem sx={{ mx: 0.5 }} />
                  <Stat value={passCount} label="满足" color={STATUS_HEX.pass} />
                  <Divider orientation="vertical" flexItem sx={{ mx: 0.5 }} />
                  <Stat value={warn} label="警告" color={STATUS_HEX.warn} />
                  <Divider orientation="vertical" flexItem sx={{ mx: 0.5 }} />
                  <Stat value={fail} label="缺失（阻断）" color={STATUS_HEX.fail} />
                  <Divider orientation="vertical" flexItem sx={{ mx: 0.5 }} />
                  <Stat value={ackedCount} label="已知悉" />
                  <Box sx={{ flexGrow: 1 }} />
                  <FormControlLabel
                    control={
                      <Switch size="small" checked={onlyProblems} onChange={(e) => setOnlyProblems(e.target.checked)} />
                    }
                    label="仅看问题项"
                  />
                  <FormControlLabel
                    control={
                      <Switch size="small" checked={hideAcked} onChange={(e) => setHideAcked(e.target.checked)} />
                    }
                    label="隐藏已知悉"
                  />
                </Stack>
              </Paper>

              {/* 每个服务一个分区（全宽表格） */}
              <Stack spacing={2}>
                {reports.map((r) => (
                  <ServiceEnvSection
                    key={r.id}
                    report={r}
                    acked={acked}
                    onToggleAck={toggleAck}
                    onlyProblems={onlyProblems}
                    hideAcked={hideAcked}
                    onRepair={startRepair}
                    repairDisabled={!!repairJob?.running}
                  />
                ))}
              </Stack>
            </>
          )}
        </>
      )}

      {/* 修复中心：CSS 隐藏挂载（不条件卸载）——切换 Tab 不丢 AI 方案 /
          资源扫描 / 安装痕迹等本地状态，返回时零重探 */}
      <Box sx={{ display: tab === "repair" ? "block" : "none" }}>
        <RepairCenter
          pending={pendingEntries}
          repairDisabled={!!repairJob?.running}
          repairRunning={!!repairJob?.running}
          activeTarget={repairJob?.target ?? null}
          onRepair={startGenericRepair}
          onUninstall={startUninstall}
        />
      </Box>

      {(loading || refreshing) && reports.length === 0 ? (
        <Box sx={{ display: "flex", justifyContent: "center", mt: 8 }}>
          <CircularProgress />
        </Box>
      ) : null}
    </Stack>
  );
}
