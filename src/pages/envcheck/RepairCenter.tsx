import { useCallback, useEffect, useMemo, useState } from "react";
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
import {
  Bolt,
  Check,
  Cloud,
  ContentCopy,
  DeleteOutline,
  Inventory2,
  Pause,
  PlayArrow,
  Refresh,
  SettingsEthernet,
  SmartToy,
  UploadFile,
  Widgets,
} from "@mui/icons-material";
import { open } from "@tauri-apps/plugin-dialog";
import { useNavigate } from "react-router-dom";
import { api } from "../../api";
import { useAppSettingsStore } from "../../store";
import type { EnvCheckItem, RepairPlan, ResourceScan, UninstallTargetStatus } from "../../types";
import { MONO_STACK } from "../../theme";
import EmptyBlock from "./EmptyBlock";
import StatusDot from "./StatusDot";
import OpsEventTimeline from "../../components/OpsEventTimeline";
import {
  CONVENTION_DIR,
  FIXED_REMOTE,
  MANAGED_ENDPOINT_TARGETS,
  SERVICE_TARGETS,
  SOFTWARE_ORDER,
  STATUS_HEX,
  STATUS_LABEL,
  TARGET_DESC,
  TARGET_LABEL,
  configTarget,
  fmtSize,
  repairTarget,
} from "./shared";
import { TARGET_ICON } from "./icons";
import BackendInstallDialog, { type ExternalConfigPayload } from "./BackendInstallDialog";

/** 修复中心：问题项汇总 + 一键修复（资源包→在线）+ AI 方案（代执行/复制 + 追问）+ 已安装组件卸载。 */
export default function RepairCenter({
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
  onRepair: (target: string, externalConfig?: Record<string, string>) => void;
  onUninstall: (target: string, removeData: boolean) => void;
}) {
  const resourceDir = useAppSettingsStore((s) => s.settings.resourceDir);
  // dfy-backend 安装前配置确认弹窗（一键安装/重装先弹配置，确认后才真正安装）
  const [backendCfgOpen, setBackendCfgOpen] = useState(false);
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
  // 启停事件时间轴刷新键：onServiceAction / 安装完成后递增
  const [eventRefresh, setEventRefresh] = useState(0);
  // 安装痕迹首次探测中（Windows 上 sc/注册表扫描需 1-2 秒，先如实显示加载态）
  const [probing, setProbing] = useState(true);
  // 探测请求本身失败（后端未启动等）：与「未安装」区分开，避免误导一键安装
  const [probeFailed, setProbeFailed] = useState(false);
  // 子导航：「安装与维护」= 装软件主流程；「环境问题与 AI 方案」= 检测问题跟进。
  // MySQL 管理 / Redis 管理已独立为顶级「运维中心」（/ops），本页专注环境安装与修复。
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

  /** 安装痕迹探测（可手动重入：服务启停 / 导入后即时刷新状态） */
  const probeStatus = useCallback(() => {
    setProbing(true);
    api
      .uninstallStatus()
      .then((s) => {
        // 探测明细进 console：target/serviceInstalled/installDir 一目了然，
        // probeVersion 用于核对探测数据出自哪个构建（排查「改了没生效」）
        console.info(
          "[RepairCenter] 探测结果 probeVersion=%s",
          s.probeVersion ?? "?",
          s.targets.map((x: UninstallTargetStatus) => ({
            t: x.target,
            installed: x.serviceInstalled,
            dir: x.installDir,
          })),
        );
        setInstalled(s.targets);
        setProbeFailed(false);
      })
      .catch(() => {
        setInstalled([]);
        setProbeFailed(true);
      })
      .finally(() => setProbing(false));
  }, []);

  useEffect(() => {
    if (repairRunning) return; // 任务进行中不探（结束时会再刷）
    probeStatus();
    // 安装/卸载等任务结束 → 事件时间轴同步刷新（自动启动等 installer 事件已留痕）
    setEventRefresh((k) => k + 1);
  }, [repairRunning, probeStatus]);

  /** 服务启停 / SQL 导入的进行中标记（按钮防抖 + loading） */
  const [busyOps, setBusyOps] = useState<string | null>(null);

  /** 一键安装 / 重装统一入口：dfy-backend 先弹配置确认（外部配置落盘），
   *  其余目标直接安装。 */
  const launchInstall = useCallback(
    (t: string) => {
      if (t === "dfy-backend") setBackendCfgOpen(true);
      else onRepair(t);
    },
    [onRepair],
  );

  /** 启动 / 停止平台服务（MySQL80、Redis；net start/stop + UAC 确认），完成后即时重探；
   *  同时递增 eventRefresh 让「启停事件」时间轴重取（操作已留痕） */
  const onServiceAction = useCallback(
    async (target: string, action: "start" | "stop") => {
      setBusyOps(`${target}:${action}`);
      try {
        const r = await api.serviceAction(target, action);
        if (r.ok) {
          setSnack({ open: true, msg: r.message || "操作成功", severity: "success" });
        } else {
          setSnack({ open: true, msg: r.error || "操作失败", severity: "error" });
        }
      } catch (e) {
        setSnack({ open: true, msg: e instanceof Error ? e.message : String(e), severity: "error" });
      } finally {
        setBusyOps(null);
        probeStatus();
        setEventRefresh((k) => k + 1);
      }
    },
    [probeStatus],
  );

  /** 导入 SQL 到 MySQL：原生文件选择器选 .sql → stdin 直灌执行 */
  const onImportSql = useCallback(async () => {
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
    setBusyOps("mysql:import");
    try {
      const r = await api.mysqlImport(picked);
      if (r.ok) {
        setSnack({ open: true, msg: "SQL 导入完成", severity: "success" });
      } else {
        setSnack({ open: true, msg: r.error || "导入失败", severity: "error" });
      }
    } catch (e) {
      setSnack({ open: true, msg: e instanceof Error ? e.message : String(e), severity: "error" });
    } finally {
      setBusyOps(null);
    }
  }, []);

  /** JDK 环境变量一键修复（JAVA_HOME + PATH，幂等）：装完 cmd 仍不识别 java 时用 */
  const onJavaEnvFix = useCallback(async () => {
    setBusyOps("java:env");
    try {
      const r = await api.javaEnvFix();
      if (r.ok) {
        setSnack({ open: true, msg: r.message || "环境变量已修复", severity: "success" });
      } else {
        setSnack({ open: true, msg: r.error || "修复失败", severity: "error" });
      }
    } catch (e) {
      setSnack({ open: true, msg: e instanceof Error ? e.message : String(e), severity: "error" });
    } finally {
      setBusyOps(null);
      probeStatus();
    }
  }, [probeStatus]);

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

  // 安装与维护的子页签（待安装/已安装/远程部署）
  const [instTab, setInstTab] = useState<"toInstall" | "installed" | "remote">("toInstall");
  /** 已安装页签内当前查看的服务（二级 Tab）；切换探测结果后自动校正为列表首项 */
  const [instSvcTab, setInstSvcTab] = useState<string>("");

  // 固定远程服务的托管端点（远程部署卡片直接展示各自的检测目标）：
  // 与「远程部署」配置面板同一可信源；未配置时值为 null（引导先去配置）。
  const [remoteEndpoints, setRemoteEndpoints] = useState<
    Record<string, { host: string; port: number } | null>
  >({});
  useEffect(() => {
    let alive = true;
    void Promise.all(
      [...MANAGED_ENDPOINT_TARGETS].map((t) =>
        api
          .serviceRemote(t)
          .then((r) =>
            [t, r.host != null && r.port != null ? { host: r.host, port: r.port } : null] as const,
          )
          .catch(() => [t, null] as const),
      ),
    ).then((entries) => {
      if (alive) setRemoteEndpoints(Object.fromEntries(entries));
    });
    return () => {
      alive = false;
    };
  }, []);

  /** 远程服务问题项「去远程部署」→ 跳转运维中心的远程部署配置页（地址/密码都在那里改） */
  const navigate = useNavigate();
  const gotoRemoteDeploy = useCallback(() => {
    navigate("/ops?tab=deploy");
  }, [navigate]);

  /** 软件卡片：状态机（执行中 > 检测中/探测不可用 > 已安装 > 残留 > 有包 > 无包）。
   *  探测（probing）不阻塞渲染——已装状态未知时如实显示「检测中」，结果回来自动归位，
   *  避免进入页面 6 张卡集体转圈的"慢"体感，也不冒充「未安装」误导一键安装。 */
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
    } else if (!st && probing) {
      // 探测中状态未知：不冒充「未安装」，避免误导用户去一键安装
      statusLine = (
        <Stack direction="row" spacing={1} alignItems="center">
          <CircularProgress size={14} />
          <Typography variant="body2" color="text.secondary">
            正在检测安装状态…
          </Typography>
        </Stack>
      );
    } else if (!st && probeFailed) {
      statusLine = (
        <Stack direction="row" spacing={1} alignItems="center" sx={{ minWidth: 0 }}>
          <Chip size="small" color="warning" label="探测不可用" />
          <Typography variant="body2" color="text.secondary">
            安装状态获取失败（运维后端未响应），可重试
          </Typography>
        </Stack>
      );
      actions = (
        <Button size="small" variant="outlined" startIcon={<Refresh />} onClick={() => probeStatus()} sx={{ textTransform: "none", whiteSpace: "nowrap" }}>
          重试
        </Button>
      );
    } else if (st?.serviceInstalled) {
      const isSvc = SERVICE_TARGETS.has(t);
      const starting = busyOps === `${t}:start`;
      const stopping = busyOps === `${t}:stop`;
      const svcBusy = starting || stopping;
      statusLine = (
        <Stack direction="row" spacing={1} alignItems="center" sx={{ minWidth: 0, flexWrap: "wrap", rowGap: 0.25 }}>
          {isSvc ? (
            <Chip
              size="small"
              color={st.serviceRunning ? "success" : "default"}
              label={starting ? "启动中…" : stopping ? "停止中…" : st.serviceRunning ? "运行中" : "已停止"}
            />
          ) : (
            <Chip size="small" color="success" label="已安装" />
          )}
          <Typography variant="body2" sx={{ fontWeight: 600, wordBreak: "break-all" }}>
            {t === "java"
              ? `JDK ${st.version ?? "版本未知"} · ${
                  st.javaHomeConfigured && st.javaPathConfigured
                    ? "环境变量已配置"
                    : "环境变量待修复"
                }`
              : st.installDir || "由 Windows Installer 管理"}
          </Typography>
          {t === "java" && !st.javaHomeConfigured && (
            <Chip size="small" color="warning" label="JAVA_HOME 缺失" />
          )}
          {t === "java" && st.javaHomeConfigured && !st.javaPathConfigured && (
            <Chip size="small" color="warning" label="PATH 缺失" />
          )}
          {t === "java" && st.installDir && (
            <Typography variant="caption" sx={{ fontFamily: MONO_STACK, color: "text.secondary", wordBreak: "break-all", width: "100%" }}>
              {st.installDir}
            </Typography>
          )}
        </Stack>
      );
      actions = (
        <>
          {isSvc && !st.serviceRunning && (
            <Tooltip
              title={
                t === "nginx"
                  ? "Nginx 是常驻服务：安装完成时已自动启动；此按钮用于因端口占用等原因启动失败后的重试。"
                  : "启动平台托管的服务"
              }
            >
              <Button
                size="small"
                variant="outlined"
                startIcon={starting ? <CircularProgress size={14} color="inherit" /> : <PlayArrow />}
                disabled={repairDisabled || svcBusy}
                onClick={() => void onServiceAction(t, "start")}
                sx={{ textTransform: "none", whiteSpace: "nowrap" }}
              >
                启动
              </Button>
            </Tooltip>
          )}
          {isSvc && st.serviceRunning && (
            <Tooltip
              title={
                t === "nginx"
                  ? "停止后前端页面与 /admin-api/ 接口将全部不可访问，请确认没有用户正在使用系统。"
                  : "停止平台托管的服务"
              }
            >
              <Button
                size="small"
                variant="outlined"
                startIcon={stopping ? <CircularProgress size={14} color="inherit" /> : <Pause />}
                disabled={repairDisabled || svcBusy}
                onClick={() => void onServiceAction(t, "stop")}
                sx={{ textTransform: "none", whiteSpace: "nowrap" }}
              >
                停止
              </Button>
            </Tooltip>
          )}
          {t === "mysql" && (
            <Tooltip title={st.serviceRunning ? "从本机 .sql 文件导入数据库" : "MySQL 未运行，请先启动服务"}>
              <span>
                <Button
                  size="small"
                  variant="outlined"
                  startIcon={busyOps === "mysql:import" ? <CircularProgress size={14} color="inherit" /> : <UploadFile />}
                  disabled={repairDisabled || svcBusy || busyOps === "mysql:import" || !st.serviceRunning}
                  onClick={() => void onImportSql()}
                  sx={{ textTransform: "none", whiteSpace: "nowrap" }}
                >
                  导入 SQL
                </Button>
              </span>
            </Tooltip>
          )}
          {t === "java" && (!st.javaHomeConfigured || !st.javaPathConfigured) && (
            <Tooltip title="写入 JAVA_HOME 并把 JDK bin 加入用户 PATH（重开命令行窗口生效）">
              <span>
                <Button
                  size="small"
                  variant="contained"
                  color="warning"
                  startIcon={busyOps === "java:env" ? <CircularProgress size={14} color="inherit" /> : <Bolt />}
                  disabled={repairDisabled || busyOps === "java:env"}
                  onClick={() => void onJavaEnvFix()}
                  sx={{ textTransform: "none", whiteSpace: "nowrap" }}
                >
                  修复环境变量
                </Button>
              </span>
            </Tooltip>
          )}
          <Button size="small" variant="outlined" disabled={repairDisabled || svcBusy} onClick={() => launchInstall(t)} sx={{ textTransform: "none", whiteSpace: "nowrap" }}>
            重装
          </Button>
          {/* 卸载：前置条件——服务/进程运行中时禁止卸载（反向代理硬断请求会丢数据） */}
          {(() => {
            const runningBlockUninstall =
              (t === "mysql" || t === "nginx" || t === "dfy-backend") && st.serviceRunning;
            return (
              <Tooltip
                title={
                  runningBlockUninstall
                    ? `${TARGET_LABEL[t]} 正在运行，请先在「服务运维」面板停止后再卸载`
                    : st.canUninstall
                      ? "卸载后需重新安装才能使用"
                      : "平台暂不支持该组件卸载"
                }
              >
                <span>
                  <Button
                    size="small"
                    variant="outlined"
                    color="error"
                    startIcon={<DeleteOutline />}
                    disabled={!st.canUninstall || repairDisabled || svcBusy || runningBlockUninstall}
                    onClick={() => {
                      setRemoveData(false);
                      setConfirmText("");
                      setUninstallTarget(st);
                    }}
                    sx={{ textTransform: "none", whiteSpace: "nowrap" }}
                  >
                    卸载
                  </Button>
                </span>
              </Tooltip>
            );
          })()}
        </>
      );
    } else if (st?.dirExists) {
      statusLine = (
        <Stack direction="row" spacing={1} alignItems="center" sx={{ minWidth: 0 }}>
          <Chip size="small" color="warning" label="残留目录" />
          <Typography variant="caption" sx={{ fontFamily: MONO_STACK, color: "text.secondary", wordBreak: "break-all" }}>
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
        <Button size="small" variant="contained" disabled={repairDisabled} onClick={() => launchInstall(t)} sx={{ textTransform: "none", whiteSpace: "nowrap" }}>
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
          <Box
            sx={{
              width: 42,
              height: 42,
              borderRadius: 2,
              display: "grid",
              placeItems: "center",
              flexShrink: 0,
              color: "#fff",
              bgcolor: runningThis ? "primary.main" : st?.serviceInstalled ? "success.main" : "primary.main",
              boxShadow: "0 2px 8px rgba(30,63,102,0.25)",
              "& svg": { fontSize: 22 },
            }}
          >
            {TARGET_ICON[t] ?? <Widgets />}
          </Box>
          <Box sx={{ width: 150, flexShrink: 0 }}>
            <Typography variant="subtitle2" sx={{ fontWeight: 700, lineHeight: 1.4, fontSize: "0.95rem" }}>
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

  /** 远程部署卡片：灰色说明行（本机一律不安装）。
   *  托管端点服务（Redis/VerneMQ/MinIO/GreptimeDB）= 各自的检测目标 + 去配置入口；
   *  用户声明的远程 = 说明 + 改回本地的入口。 */
  const renderRemoteCard = (t: string) => {
    const managed = MANAGED_ENDPOINT_TARGETS.has(t);
    const ep = managed ? remoteEndpoints[t] : undefined;
    return (
      <Paper key={t} variant="outlined" sx={{ px: 2, py: 1.25, bgcolor: "action.hover" }}>
        <Stack direction="row" spacing={2} alignItems="center" sx={{ flexWrap: "wrap" }}>
          <Box
            sx={{
              width: 42,
              height: 42,
              borderRadius: 2,
              display: "grid",
              placeItems: "center",
              flexShrink: 0,
              color: "text.secondary",
              bgcolor: "grey.300",
              "& svg": { fontSize: 22 },
            }}
          >
            {TARGET_ICON[t] ?? <Widgets />}
          </Box>
          <Box sx={{ width: 150, flexShrink: 0 }}>
            <Typography variant="subtitle2" sx={{ fontWeight: 700, lineHeight: 1.4, fontSize: "0.95rem", color: "text.secondary" }}>
              {TARGET_LABEL[t] ?? t}
            </Typography>
            <Typography variant="caption" color="text.secondary">
              {TARGET_DESC[t] ?? ""}
            </Typography>
          </Box>
          <Box sx={{ flex: 1, minWidth: 260 }}>
            {managed ? (
              <Typography variant="body2" color="text.secondary">
                远程部署（本机不安装）：检测目标为托管端点
                <Typography
                  component="span"
                  sx={{ fontFamily: MONO_STACK, fontWeight: 700, color: "text.primary", mx: 0.5 }}
                >
                  {ep ? `${ep.host}:${ep.port}` : ep === null ? "未配置" : "…"}
                </Typography>
                ；不可达时请在「运维中心 → 远程部署」检查地址与密码
              </Typography>
            ) : (
              <Typography variant="body2" color="text.secondary">
                本机无需安装（按部署规划远程部署）
              </Typography>
            )}
          </Box>
          <Stack direction="row" spacing={1} alignItems="center" sx={{ flexShrink: 0 }}>
            {managed ? (
              <Button
                size="small"
                variant="outlined"
                startIcon={<SettingsEthernet />}
                onClick={gotoRemoteDeploy}
                sx={{ textTransform: "none", whiteSpace: "nowrap" }}
              >
                去远程部署
              </Button>
            ) : (
              <Button size="small" variant="text" disabled={repairRunning} onClick={() => void toggleRemote(t, false)} sx={{ textTransform: "none", whiteSpace: "nowrap" }}>
                改回本地安装
              </Button>
            )}
          </Stack>
        </Stack>
      </Paper>
    );
  };

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

      {/* ===== 安装与维护：页头 + 三个并列子 Tab（待安装/已安装/远程部署） =====
          CSS 隐藏挂载（display 切换）：切到「环境问题」页不卸载本面板，
          探测结果与卡片状态原样保留，切回即现 */}
      <Box sx={{ display: subTab === "install" ? undefined : "none" }}>
          <Box sx={{ mb: -0.5 }}>
            <Typography variant="h6" sx={{ fontWeight: 700, letterSpacing: 0.2 }}>
              环境修复
            </Typography>
            <Typography variant="body2" color="text.secondary">
              识别资源目录中的离线安装包，一键安装 / 卸载 MySQL、JDK 等环境依赖；
              安装后的配置文件统一在「运维中心 → 配置中心」查看与编辑
            </Typography>
          </Box>

          {(() => {
            // 纯远程软件（FIXED_REMOTE）不进待安装/已安装——本机不安装、无安装包可识别；
            // 用户声明的远程部署（remoteSet）仍可与固定远程共存展示。
            const local = (t: string) => !remoteSet.has(t) && !FIXED_REMOTE.has(t);
            const toInstall = SOFTWARE_ORDER.filter(
              (t) => local(t) && !installedOf(t)?.serviceInstalled,
            );
            const installedList = SOFTWARE_ORDER.filter(
              (t) => local(t) && installedOf(t)?.serviceInstalled,
            );
            const remoteList = SOFTWARE_ORDER.filter(
              (t) => remoteSet.has(t) || FIXED_REMOTE.has(t),
            );
            const TabLabel = ({ text, count }: { text: string; count: number }) => (
              <Stack direction="row" spacing={0.75} alignItems="center">
                <span>{text}</span>
                {count > 0 && (
                  <Box
                    sx={{
                      minWidth: 18,
                      height: 18,
                      px: 0.5,
                      borderRadius: 9,
                      display: "grid",
                      placeItems: "center",
                      fontSize: 11,
                      fontWeight: 700,
                      bgcolor: "primary.main",
                      color: "#fff",
                    }}
                  >
                    {count}
                  </Box>
                )}
              </Stack>
            );

            return (
              <>
                <Tabs
                  value={instTab}
                  onChange={(_, v: "toInstall" | "installed" | "remote") => setInstTab(v)}
                  sx={{ "& .MuiTab-root": { textTransform: "none", minHeight: 44 } }}
                >
                  <Tab value="toInstall" label={<TabLabel text="待安装" count={toInstall.length} />} />
                  <Tab value="installed" label={<TabLabel text="已安装" count={installedList.length} />} />
                  <Tab value="remote" label={<TabLabel text="远程部署" count={remoteList.length} />} />
                </Tabs>

                {instTab === "toInstall" && (
                  <Stack spacing={1.25}>
                    {!resourceDir ? (
                      <Alert severity="warning">
                        先配置「本地资源目录」（设置 → 资源目录，可用「浏览…」选择）：把安装包放进
                        <b>以软件名命名的子目录</b>（如 <b>MySQL\</b>、<b>JDK\</b>），这里就会识别并出现「一键安装」按钮。
                      </Alert>
                    ) : scan && !scan.exists ? (
                      <Alert severity="error">资源目录（{resourceDir}）不存在或不可访问，请到「设置 → 资源目录」重新选择。</Alert>
                    ) : null}
                    {probing && (
                      <Stack direction="row" spacing={1} alignItems="center">
                        <CircularProgress size={12} />
                        <Typography variant="caption" color="text.secondary">
                          正在检测本机安装状态…
                        </Typography>
                      </Stack>
                    )}
                    {toInstall.length === 0 ? (
                      <EmptyBlock
                        icon={<Check sx={{ fontSize: 36, color: "success.main" }} />}
                        title={probing ? "正在检测安装状态…" : "没有待安装的软件"}
                        hint={probing ? undefined : "已安装的软件在「已安装」页；缺安装包的软件放入资源目录后会出现在这里"}
                      />
                    ) : (
                      toInstall.map((t) => renderCard(t))
                    )}
                    {scan?.misplaced && scan.misplaced.filter((m) => !FIXED_REMOTE.has(m.fileTarget)).length > 0 && (
                      <Alert severity="warning" icon={false}>
                        {scan.misplaced
                          .filter((m) => !FIXED_REMOTE.has(m.fileTarget))
                          .map((m) => (
                            <Typography key={m.path} variant="body2" component="div">
                              「{m.fileName}」放在了 {m.foundIn}/ 目录，但它属于 {m.fileTarget.toUpperCase()} ——建议移到对应的约定子目录（按软件名命名）
                            </Typography>
                          ))}
                      </Alert>
                    )}
                  </Stack>
                )}

                {instTab === "installed" && (
                  <Stack spacing={1.25}>
                    {probing && (
                      <Stack direction="row" spacing={1} alignItems="center">
                        <CircularProgress size={12} />
                        <Typography variant="caption" color="text.secondary">
                          正在检测…
                        </Typography>
                      </Stack>
                    )}
                    {installedList.length === 0 ? (
                      <EmptyBlock
                        icon={<Inventory2 sx={{ fontSize: 36 }} />}
                        title={probing ? "正在检测…" : "暂无已安装的软件"}
                        hint="在「待安装」页一键安装后，软件会出现在这里（可重装 / 卸载）"
                      />
                    ) : (
                      <>
                        {/* 每个服务一个页签：独占整页宽度，启停事件跟随服务展示，
                            不再所有卡片竖排 + 全局事件堆底（分不清是谁的事件） */}
                        <Tabs
                          value={installedList.includes(instSvcTab) ? instSvcTab : installedList[0]}
                          onChange={(_, v: string) => setInstSvcTab(v)}
                          sx={{ "& .MuiTab-root": { textTransform: "none", minHeight: 44 } }}
                        >
                          {installedList.map((t) => (
                            <Tab key={t} value={t} label={TARGET_LABEL[t] ?? t.toUpperCase()} />
                          ))}
                        </Tabs>
                        {renderCard(
                          installedList.includes(instSvcTab) ? instSvcTab : installedList[0],
                        )}
                        <OpsEventTimeline
                          refreshKey={eventRefresh}
                          target={installedList.includes(instSvcTab) ? instSvcTab : installedList[0]}
                        />
                      </>
                    )}
                  </Stack>
                )}

                {instTab === "remote" && (
                  <Stack spacing={1.25}>
                    {remoteList.length === 0 ? (
                      <EmptyBlock
                        icon={<Cloud sx={{ fontSize: 36 }} />}
                        title="没有声明远程部署的软件"
                        hint="Redis、VerneMQ 等走远程部署的软件可在这里标记（本机不安装）"
                      />
                    ) : (
                      remoteList.map((t) => renderRemoteCard(t))
                    )}
                  </Stack>
                )}
              </>
            );
          })()}
      </Box>

      {/* ===== 环境问题与 AI 方案（CSS 隐藏挂载：切页不丢 AI 方案卡与追问输入） ===== */}
      <Box sx={{ display: subTab === "issues" ? undefined : "none" }}>
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
                  const cfg = configTarget(item.id);
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
                        <Typography variant="body2" sx={{ fontFamily: MONO_STACK, wordBreak: "break-all", color: "text.secondary" }}>
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
                            onClick={() => launchInstall(target)}
                            sx={{ textTransform: "none", px: 1 }}
                          >
                            一键修复
                          </Button>
                        ) : cfg ? (
                          <Button
                            size="small"
                            variant="outlined"
                            onClick={gotoRemoteDeploy}
                            sx={{ textTransform: "none", px: 1, whiteSpace: "nowrap" }}
                          >
                            去远程部署
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
                  sx={{ display: "block", mt: 0.5, fontFamily: MONO_STACK, wordBreak: "break-all", color: "text.primary" }}
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
      </Box>

      {/* 代执行确认弹窗：白名单命令 + UAC 提权，执行前必须人工确认 */}
      <Dialog open={!!execCommand} onClose={() => !executing && setExecCommand(null)} maxWidth="sm" fullWidth>
        <DialogTitle>确认代执行命令</DialogTitle>
        <DialogContent>
          <Typography variant="body2" color="text.secondary">
            该命令将在本机以管理员权限执行（请在随后弹出的 UAC 窗口确认）：
          </Typography>
          <Paper variant="outlined" sx={{ p: 1.5, mt: 1, bgcolor: "action.hover" }}>
            <Typography variant="body2" sx={{ fontFamily: MONO_STACK, wordBreak: "break-all" }}>
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
                <Typography variant="body2" sx={{ fontFamily: MONO_STACK, wordBreak: "break-all" }}>
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

      {/* DFY 后端安装前配置确认：预填生产默认值，确认后才真正安装 */}
      <BackendInstallDialog
        open={backendCfgOpen}
        onClose={() => setBackendCfgOpen(false)}
        onConfirm={(cfg: ExternalConfigPayload) => {
          setBackendCfgOpen(false);
          onRepair("dfy-backend", cfg as unknown as Record<string, string>);
        }}
      />

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
