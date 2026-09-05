import { Box, Chip, CircularProgress, IconButton, Tooltip, Typography } from "@mui/material";
import {
  CheckCircle as CheckCircleIcon,
  DeleteOutline as DeleteIcon,
  EditOutlined as EditIcon,
  PlayArrow as PlayIcon,
  SmartToyOutlined as ModelIcon,
  Star as StarIcon,
  StarBorder as StarBorderIcon,
  WarningAmber as WarningIcon,
} from "@mui/icons-material";
import type { AiModelDto } from "../../types";
import { CARD_BORDER, DISABLED_COLOR, ERROR_COLOR, PRIMARY_COLOR, SUCCESS_COLOR, type TestResult } from "./shared";

/**
 * 模型行：三层配置的最内层。原样抽出自 AiAssistant.tsx，行为 1:1。
 */
export function ModelRow({
  model,
  isActive,
  testResult,
  isTesting,
  onSetActive,
  onTest,
  onEdit,
  onDelete,
}: {
  model: AiModelDto;
  isActive: boolean;
  testResult?: TestResult;
  isTesting: boolean;
  onSetActive: (id: string) => void;
  onTest: (id: string) => void;
  onEdit: (model: AiModelDto) => void;
  onDelete: (model: AiModelDto, isActive: boolean) => void;
}) {
  return (
    <Box
      sx={{
        p: 0.75,
        border: 1,
        borderColor: CARD_BORDER,
        borderRadius: 1,
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        "&:hover": { borderColor: `${PRIMARY_COLOR}80` },
      }}
    >
      <Box sx={{ display: "flex", alignItems: "center", gap: 0.5, minWidth: 0, flex: 1 }}>
        <ModelIcon sx={{ fontSize: 12, color: model.enabled ? PRIMARY_COLOR : DISABLED_COLOR }} />
        <Box sx={{ minWidth: 0, flex: 1 }}>
          <Typography variant="caption" sx={{ fontWeight: 600, display: "block", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
            {model.name}
          </Typography>
          <Typography variant="caption" color="text.secondary" sx={{ fontSize: 8 }}>
            {model.refKey}
          </Typography>
        </Box>
      </Box>
      <Box sx={{ display: "flex", alignItems: "center", gap: 0.25, flexShrink: 0 }}>
        {isActive && (
          <Chip label="诊断模型" size="small" color="primary" sx={{ fontSize: 8, height: 14 }} />
        )}
        {testResult &&
          (testResult.success ? (
            <Tooltip title={testResult.message}>
              <CheckCircleIcon sx={{ fontSize: 10, color: SUCCESS_COLOR }} />
            </Tooltip>
          ) : (
            <Tooltip title={testResult.message}>
              <WarningIcon sx={{ fontSize: 10, color: ERROR_COLOR }} />
            </Tooltip>
          ))}
        {isTesting && <CircularProgress size={10} />}
        <Tooltip title={isActive ? "当前诊断模型" : "设为诊断模型"}>
          <IconButton size="small" sx={{ p: 0.15 }} onClick={(e) => { e.stopPropagation(); onSetActive(model.id); }}>
            {isActive ? (
              <StarIcon sx={{ fontSize: 12, color: PRIMARY_COLOR }} />
            ) : (
              <StarBorderIcon sx={{ fontSize: 12 }} />
            )}
          </IconButton>
        </Tooltip>
        <Tooltip title="测试模型">
          <IconButton size="small" sx={{ p: 0.15 }} onClick={(e) => { e.stopPropagation(); onTest(model.id); }}>
            <PlayIcon sx={{ fontSize: 10 }} />
          </IconButton>
        </Tooltip>
        <IconButton size="small" sx={{ p: 0.15 }} onClick={() => onEdit(model)}>
          <EditIcon sx={{ fontSize: 10 }} />
        </IconButton>
        <IconButton size="small" sx={{ p: 0.15 }} onClick={() => onDelete(model, isActive)}>
          <DeleteIcon sx={{ fontSize: 10, color: ERROR_COLOR }} />
        </IconButton>
      </Box>
    </Box>
  );
}
