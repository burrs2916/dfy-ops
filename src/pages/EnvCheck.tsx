import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { MONO_STACK } from "../theme";
import {
  Alert,
  Badge,
  Box,
  Button,
  CircularProgress,
  Divider,
  FormControlLabel,
  Paper,
  Stack,
  Switch,
  Tab,
  Tabs,
  Typography,
} from "@mui/material";
import { Check, ContentCopy, Pause, PlayArrow, Refresh } from "@mui/icons-material";
import { useSearchParams } from "react-router-dom";
import { api } from "../api";
import { useAppSettingsStore, useOpsStore } from "../store";
import type { EnvRepairJob } from "../types";
import { REPAIR_STAGE_LABEL, STATUS_HEX, TARGET_LABEL, ackId, loadAck, saveAck } from "./envcheck/shared";
import Stat from "./envcheck/Stat";
import ServiceEnvSection from "./envcheck/ServiceEnvSection";
import RepairCenter from "./envcheck/RepairCenter";

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
  const [searchParams, setSearchParams] = useSearchParams();
  // 「去修复」跳入时的目标服务：滚动定位 + 短暂高亮，让用户不用在长页里翻找
  const [highlightId, setHighlightId] = useState<string | null>(null);

  useEffect(() => {
    const target = searchParams.get("service");
    if (!target || Object.keys(envMap).length === 0) return;
    setSearchParams({}, { replace: true });
    const timer = window.setTimeout(() => {
      document
        .getElementById(`env-report-${target}`)
        ?.scrollIntoView({ behavior: "smooth", block: "start" });
      setHighlightId(target);
      window.setTimeout(() => setHighlightId(null), 2600);
    }, 150);
    return () => window.clearTimeout(timer);
  }, [searchParams, envMap, setSearchParams]);

  // ===== 环境修复（java 分级链路 + 通用资源包/在线链路，共用同一任务状态） =====
  const [repairJob, setRepairJob] = useState<EnvRepairJob | null>(null);
  const [repairNotice, setRepairNotice] = useState<string | null>(null);
  const [repairError, setRepairError] = useState<string | null>(null);
  const repairPollRef = useRef<number | null>(null);

  const reports = useMemo(() => Object.values(envMap), [envMap]);

  useEffect(() => {
    void bootstrap();
  }, [bootstrap]);

  // 切走再回来：轮询随组件卸载被清掉，必须接上后台仍在跑的任务，
  // 否则任务状态丢失、完成提示也收不到
  useEffect(() => {
    let alive = true;
    api
      .envRepairStatus()
      .then((job) => {
        if (!alive) return;
        setRepairJob(job);
        if (job.running) pollRepair();
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

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
            setRepairNotice(`${label} ${job.op === "uninstall" ? "卸载" : "安装"}完成，环境已自动重新检查`);
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

  /** 修复中心的「一键修复」：资源包优先 → java 在线兜底 → 无包提示。
   *  dfy-backend 走安装前配置确认弹窗，确认后带 externalConfig 落盘。 */
  const startGenericRepair = useCallback(
    async (target: string, externalConfig?: Record<string, string>) => {
      setRepairError(null);
      setRepairNotice(null);
      try {
        const res = await api.startGenericRepair(target, externalConfig);
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
                  fontFamily: MONO_STACK,
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
      {!repairJob?.running && repairNotice && (
        <Alert severity="success" onClose={() => setRepairNotice(null)}>
          {repairNotice}
        </Alert>
      )}
      {!repairJob?.running && repairError && (
        <Alert severity="error" onClose={() => setRepairError(null)}>
          {repairError}
        </Alert>
      )}

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
                  <Box
                    key={r.id}
                    id={`env-report-${r.id}`}
                    sx={{
                      borderRadius: 2,
                      transition: "box-shadow 0.4s, outline-color 0.4s",
                      outline: highlightId === r.id ? 2 : 0,
                      outlineColor: "primary.main",
                      outlineOffset: 2,
                    }}
                  >
                    <ServiceEnvSection
                      report={r}
                      acked={acked}
                      onToggleAck={toggleAck}
                      onlyProblems={onlyProblems}
                      hideAcked={hideAcked}
                      onRepair={startRepair}
                      repairDisabled={!!repairJob?.running}
                    />
                  </Box>
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
