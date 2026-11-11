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
          检测范围（{report.items.length} 项）：{[...new Set(report.items.map((i) => i.name))].join(" · ")}
        </Typography>
      </Box>
      <Divider />

      {/* 检测结果表：状态 / 检查项（含期望）/ 检测值（含修复建议）/ 操作。
          期望与建议不再各自独占一列——六列时三列均分剩余宽度，中文逐字竖排；
          收敛为四列后检测值独占大半行宽，实际探测结果一眼读完。 */}
      <TableContainer>
        <Table size="small" sx={{ tableLayout: "fixed" }}>
          <TableHead>
            <TableRow sx={{ bgcolor: "rgba(0,0,0,0.03)" }}>
              <TableCell sx={{ width: 96 }}>状态</TableCell>
              <TableCell sx={{ width: "24%" }}>检查项</TableCell>
              <TableCell>检测值</TableCell>
              <TableCell sx={{ width: 104 }}>操作</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {rows.length === 0 ? (
              <TableRow>
                <TableCell colSpan={4} align="center">
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
                    sx={{
                      opacity: isAcked ? 0.55 : 1,
                      // 满足行降噪：结论无需多看，压缩行高让问题行更突出
                      "& td": {
                        verticalAlign: "top",
                        py: it.status === "pass" ? 0.5 : 1,
                      },
                    }}
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
                    {/* 检查项（期望并入第二行，作为判定依据随项走） */}
                    <TableCell>
                      <Typography variant="body2" sx={{ fontWeight: 600 }}>
                        {it.name}
                      </Typography>
                      {it.status === "fail" && (
                        <Typography variant="caption" sx={{ color: "#A32D2D", fontWeight: 700, display: "block" }}>
                          阻断（需先修复）
                        </Typography>
                      )}
                      <Typography variant="caption" color="text.secondary" sx={{ display: "block" }}>
                        期望：{it.expected}
                      </Typography>
                    </TableCell>
                    {/* 检测值（用户最关心的"实际探到什么"）；非 pass 时修复建议紧跟其下 */}
                    <TableCell>
                      <Typography
                        variant="body2"
                        sx={{
                          fontFamily: MONO_STACK,
                          color: STATUS_HEX[it.status],
                          wordBreak: "break-word",
                        }}
                      >
                        {it.actual}
                      </Typography>
                      {hasHint && !isAcked ? (
                        <Stack direction="row" spacing={0.5} alignItems="flex-start" sx={{ mt: 0.5 }}>
                          <Typography
                            variant="caption"
                            sx={{
                              fontFamily: MONO_STACK,
                              color: "text.primary",
                              wordBreak: "break-word",
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
                        isAcked && (
                          <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.25 }}>
                            已人工确认
                          </Typography>
                        )
                      )}
                    </TableCell>
                    {/* 操作 */}
                    <TableCell>
                      {it.status !== "pass" && (
                        <Stack direction="row" spacing={0.5} alignItems="center" flexWrap="wrap" useFlexGap>
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
