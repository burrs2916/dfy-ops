import {
  Box,
  Button,
  Card,
  Collapse,
  Divider,
  IconButton,
  Tooltip,
  Typography,
  CircularProgress,
} from "@mui/material";
import {
  Add as AddIcon,
  Cable as CableIcon,
  CheckCircle as CheckCircleIcon,
  DeleteOutline as DeleteIcon,
  EditOutlined as EditIcon,
  ExpandLess as ExpandLessIcon,
  ExpandMore as ExpandMoreIcon,
  PlayArrow as PlayIcon,
  WarningAmber as WarningIcon,
} from "@mui/icons-material";
import type { AiEndpointDto, AiModelDto } from "../../types";
import { ModelRow } from "./ModelRow";
import {
  CARD_BORDER,
  ERROR_COLOR,
  HOVER_BG2,
  INNER_CARD_BG,
  SUCCESS_COLOR,
  getApiTypeLabel,
  type TestResult,
} from "./shared";

/**
 * 接入点卡片：三层配置的中间层（展开后列出模型行）。原样抽出自 AiAssistant.tsx，行为 1:1。
 */
export function EndpointCard({
  endpoint,
  models,
  expanded,
  onToggle,
  testResult,
  isTesting,
  modelTestResults,
  testingModelId,
  activeModelId,
  onTest,
  onEdit,
  onDelete,
  onAddModel,
  onEditModel,
  onDeleteModel,
  onTestModel,
  onSetActiveModel,
}: {
  endpoint: AiEndpointDto;
  models: AiModelDto[];
  expanded: boolean;
  onToggle: () => void;
  testResult?: TestResult;
  isTesting: boolean;
  modelTestResults: Map<string, TestResult>;
  testingModelId: string | null;
  activeModelId: string | null;
  onTest: () => void;
  onEdit: () => void;
  onDelete: () => void;
  onAddModel: () => void;
  onEditModel: (model: AiModelDto) => void;
  onDeleteModel: (model: AiModelDto, isActive: boolean) => void;
  onTestModel: (id: string) => void;
  onSetActiveModel: (id: string) => void;
}) {
  return (
    <Card variant="outlined" sx={{ borderRadius: 1.5, borderColor: CARD_BORDER, bgcolor: INNER_CARD_BG }}>
      <Box
        sx={{
          p: 1,
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          cursor: "pointer",
          "&:hover": { bgcolor: HOVER_BG2 },
        }}
        onClick={onToggle}
      >
        <Box sx={{ display: "flex", alignItems: "center", gap: 0.75 }}>
          <CableIcon sx={{ fontSize: 14, color: "#BA7517" }} />
          <Box>
            <Typography variant="caption" sx={{ fontWeight: 600 }}>
              {endpoint.name}
            </Typography>
            <Typography variant="caption" color="text.secondary" sx={{ display: "block", fontSize: 9 }}>
              {getApiTypeLabel(endpoint.apiType)} · {endpoint.baseUrl || "未配置地址"}
            </Typography>
          </Box>
        </Box>
        <Box sx={{ display: "flex", alignItems: "center", gap: 0.25 }} onClick={(e) => e.stopPropagation()}>
          {testResult &&
            (testResult.success ? (
              <Tooltip title={testResult.message}>
                <CheckCircleIcon sx={{ fontSize: 12, color: SUCCESS_COLOR }} />
              </Tooltip>
            ) : (
              <Tooltip title={testResult.message}>
                <WarningIcon sx={{ fontSize: 12, color: ERROR_COLOR }} />
              </Tooltip>
            ))}
          {isTesting && <CircularProgress size={12} />}
          <Tooltip title="测试连接">
            <IconButton size="small" sx={{ p: 0.25 }} onClick={onTest}>
              <PlayIcon sx={{ fontSize: 12 }} />
            </IconButton>
          </Tooltip>
          <Tooltip title="编辑接入点">
            <IconButton size="small" sx={{ p: 0.25 }} onClick={onEdit}>
              <EditIcon sx={{ fontSize: 12 }} />
            </IconButton>
          </Tooltip>
          <Tooltip title="删除接入点">
            <IconButton size="small" sx={{ p: 0.25 }} onClick={onDelete}>
              <DeleteIcon sx={{ fontSize: 12, color: ERROR_COLOR }} />
            </IconButton>
          </Tooltip>
          <IconButton size="small" sx={{ p: 0.25 }} onClick={onToggle}>
            {expanded ? <ExpandLessIcon sx={{ fontSize: 12 }} /> : <ExpandMoreIcon sx={{ fontSize: 12 }} />}
          </IconButton>
        </Box>
      </Box>

      <Collapse in={expanded}>
        <Divider sx={{ borderColor: CARD_BORDER }} />
        <Box sx={{ p: 1 }}>
          <Box sx={{ display: "flex", justifyContent: "space-between", alignItems: "center", mb: 0.75 }}>
            <Typography variant="caption" color="text.secondary">
              模型 ({models.length})
            </Typography>
            <Button
              size="small"
              startIcon={<AddIcon sx={{ fontSize: 10 }} />}
              onClick={onAddModel}
              sx={{ fontSize: 9, borderRadius: 1 }}
            >
              新增模型
            </Button>
          </Box>

          {models.length === 0 ? (
            <Typography variant="caption" color="text.disabled" sx={{ display: "block", textAlign: "center", py: 0.75 }}>
              暂无模型
            </Typography>
          ) : (
            <Box sx={{ display: "flex", flexDirection: "column", gap: 0.5 }}>
              {models.map((model) => {
                const modelTestResult = modelTestResults.get(model.id);
                const isModelTesting = testingModelId === model.id;
                const isActive = activeModelId === model.id;

                return (
                  <ModelRow
                    key={model.id}
                    model={model}
                    isActive={isActive}
                    testResult={modelTestResult}
                    isTesting={isModelTesting}
                    onSetActive={onSetActiveModel}
                    onTest={onTestModel}
                    onEdit={onEditModel}
                    onDelete={onDeleteModel}
                  />
                );
              })}
            </Box>
          )}
        </Box>
      </Collapse>
    </Card>
  );
}
