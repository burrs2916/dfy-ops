import { useMemo } from "react";
import {
  Box,
  Button,
  Chip,
  Divider,
  IconButton,
  Paper,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Typography,
} from "@mui/material";
import { ContentCopy } from "@mui/icons-material";
import { MONO_STACK } from "../../theme";
import type { ServiceEnvReport } from "../../types";
import { STATUS_COLOR, STATUS_HEX, STATUS_LABEL, ackId } from "./shared";
import StatusDot from "./StatusDot";

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
              <TableCell sx={{ width: 108 }}>状态</TableCell>
              <TableCell sx={{ width: 190 }}>检查项</TableCell>
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
                          fontFamily: MONO_STACK,
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
                              fontFamily: MONO_STACK,
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
                          {it.status === "pass"
                            ? "符合要求"
                            : isAcked
                              ? "已人工确认"
                              : "人工处理，或使用 AI 方案"}
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

export default ServiceEnvSection;
