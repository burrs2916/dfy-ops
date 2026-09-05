import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Alert,
  Box,
  Button,
  Chip,
  CircularProgress,
  Divider,
  FormControlLabel,
  IconButton,
  Paper,
  Stack,
  Switch,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Typography,
} from "@mui/material";
import { Check, ContentCopy, Pause, PlayArrow, Refresh } from "@mui/icons-material";
import { api } from "../api";
import { useAppSettingsStore } from "../store";
import type { CheckStatus, ServiceEnvReport } from "../types";

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

function ServiceEnvSection({
  report,
  acked,
  onToggleAck,
  onlyProblems,
  hideAcked,
}: {
  report: ServiceEnvReport;
  acked: Set<string>;
  onToggleAck: (id: string) => void;
  onlyProblems: boolean;
  hideAcked: boolean;
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
                        <Button
                          size="small"
                          color={isAcked ? "inherit" : "primary"}
                          onClick={() => onToggleAck(id)}
                          sx={{ textTransform: "none", px: 1 }}
                        >
                          {isAcked ? "撤销" : "已知悉"}
                        </Button>
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

export default function EnvCheck() {
  const [reports, setReports] = useState<ServiceEnvReport[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copiedAll, setCopiedAll] = useState(false);
  const [acked, setAcked] = useState<Set<string>>(() => loadAck());
  const [onlyProblems, setOnlyProblems] = useState(false);
  const [hideAcked, setHideAcked] = useState(false);
  const [live, setLive] = useState(true);
  const [lastUpdate, setLastUpdate] = useState<Date | null>(null);

  const load = useCallback(async (silent = false) => {
    if (!silent) setLoading(true);
    setError(null);
    try {
      const r = await api.envCheckAll();
      setReports(r);
      setLastUpdate(new Date());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (!silent) setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  // 实时刷新：按「设置 → 通用」配置的间隔静默复探，使修复后状态自动反映。
  const envRefreshMs = useAppSettingsStore((s) => s.settings.envRefreshSecs) * 1000;
  useEffect(() => {
    if (!live) return;
    const id = window.setInterval(() => {
      load(true);
    }, envRefreshMs);
    return () => window.clearInterval(id);
  }, [live, load, envRefreshMs]);

  const toggleAck = useCallback((id: string) => {
    setAcked((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      saveAck(next);
      return next;
    });
  }, []);

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
            运维中心 · 运行环境检测
          </Typography>
          <Typography variant="body2" color="text.secondary" sx={{ mt: 0.25 }}>
            仅检测运行时依赖（不含构建工具）· 部署产物已预置，此处验证各服务"能否跑起来"
          </Typography>
        </Box>
        <Stack direction="row" spacing={1} alignItems="center">
          <Button
            variant="outlined"
            color={live ? "success" : "inherit"}
            startIcon={live ? <Pause /> : <PlayArrow />}
            onClick={() => setLive((v) => !v)}
          >
            {live ? "实时中" : "已暂停"}
          </Button>
          <Button
            variant="contained"
            startIcon={loading ? <CircularProgress size={16} color="inherit" /> : <Refresh />}
            onClick={() => load(false)}
            disabled={loading}
          >
            重新检查
          </Button>
        </Stack>
      </Stack>
      <Typography variant="caption" color="text.secondary">
        实时刷新{live ? "中" : "已暂停"} · 上次更新{" "}
        {lastUpdate ? lastUpdate.toLocaleTimeString("zh-CN") : "—"}
      </Typography>

      {error && (
        <Alert severity="error">{error}（请确认运维后端已在 127.0.0.1:18721 运行）</Alert>
      )}

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
          请在运维中心按「修复建议」自行执行，本平台仅做探测与提醒，不直接安装依赖。
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
              />
            ))}
          </Stack>
        </>
      )}

      {loading && reports.length === 0 ? (
        <Box sx={{ display: "flex", justifyContent: "center", mt: 8 }}>
          <CircularProgress />
        </Box>
      ) : null}
    </Stack>
  );
}
