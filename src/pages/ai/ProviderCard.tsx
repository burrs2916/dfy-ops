import {
  Box,
  Button,
  Card,
  Chip,
  Collapse,
  Divider,
  IconButton,
  Tooltip,
  Typography,
} from "@mui/material";
import {
  Add as AddIcon,
  DeleteOutline as DeleteIcon,
  EditOutlined as EditIcon,
  ExpandLess as ExpandLessIcon,
  ExpandMore as ExpandMoreIcon,
} from "@mui/icons-material";
import type { AiEndpointDto, AiModelDto, AiProviderDto } from "../../types";
import { EndpointCard } from "./EndpointCard";
import { ProviderDot, CARD_BG, CARD_BORDER, ERROR_COLOR, HOVER_BG, type TestResult } from "./shared";

/**
 * 供应商卡片：三层配置的最外层（展开后列出接入点卡片）。原样抽出自 AiAssistant.tsx，行为 1:1。
 */
export function ProviderCard({
  provider,
  endpoints,
  models,
  expanded,
  onToggle,
  expandedEndpoints,
  onToggleEndpoint,
  testResults,
  testingId,
  modelTestResults,
  testingModelId,
  activeModelId,
  onEdit,
  onDelete,
  onAddEndpoint,
  onEditEndpoint,
  onDeleteEndpoint,
  onTestConnection,
  onAddModel,
  onEditModel,
  onDeleteModel,
  onTestModel,
  onSetActiveModel,
}: {
  provider: AiProviderDto;
  /** 该供应商名下的接入点（父层已按 providerId 过滤） */
  endpoints: AiEndpointDto[];
  /** 全部模型（用于删除提示统计与按接入点过滤） */
  models: AiModelDto[];
  expanded: boolean;
  onToggle: () => void;
  expandedEndpoints: Set<string>;
  onToggleEndpoint: (id: string) => void;
  testResults: Map<string, TestResult>;
  testingId: string | null;
  modelTestResults: Map<string, TestResult>;
  testingModelId: string | null;
  activeModelId: string | null;
  onEdit: () => void;
  onDelete: () => void;
  onAddEndpoint: (providerId: string) => void;
  onEditEndpoint: (endpoint: AiEndpointDto, providerId: string) => void;
  onDeleteEndpoint: (endpoint: AiEndpointDto) => void;
  onTestConnection: (endpointId: string) => void;
  onAddModel: (endpointId: string) => void;
  onEditModel: (model: AiModelDto, endpointId: string) => void;
  onDeleteModel: (model: AiModelDto, isActive: boolean) => void;
  onTestModel: (id: string) => void;
  onSetActiveModel: (id: string) => void;
}) {
  const providerEndpoints = endpoints;
  const isExpanded = expanded;

  return (
    <Card sx={{ border: 1, borderColor: CARD_BORDER, borderRadius: 2, bgcolor: CARD_BG, overflow: "visible" }}>
      <Box
        sx={{
          p: 1.5,
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          cursor: "pointer",
          "&:hover": { bgcolor: HOVER_BG },
        }}
        onClick={onToggle}
      >
        <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
          <ProviderDot name={provider.name} size={10} />
          <Box>
            <Typography variant="body2" sx={{ fontWeight: 600 }}>
              {provider.name}
            </Typography>
            <Box sx={{ display: "flex", gap: 0.5, alignItems: "center" }}>
              <Chip label={`${providerEndpoints.length} 接入点`} size="small" variant="outlined" sx={{ fontSize: 9, height: 18 }} />
              <Chip
                label={provider.enabled ? "已启用" : "已禁用"}
                size="small"
                color={provider.enabled ? "success" : "default"}
                sx={{ fontSize: 9, height: 18 }}
              />
            </Box>
          </Box>
        </Box>
        <Box sx={{ display: "flex", alignItems: "center", gap: 0.25 }} onClick={(e) => e.stopPropagation()}>
          <Tooltip title="编辑供应商">
            <IconButton size="small" onClick={onEdit}>
              <EditIcon sx={{ fontSize: 14 }} />
            </IconButton>
          </Tooltip>
          <Tooltip title="删除供应商">
            <IconButton size="small" onClick={onDelete}>
              <DeleteIcon sx={{ fontSize: 14, color: ERROR_COLOR }} />
            </IconButton>
          </Tooltip>
          <IconButton size="small" onClick={onToggle}>
            {isExpanded ? <ExpandLessIcon sx={{ fontSize: 14 }} /> : <ExpandMoreIcon sx={{ fontSize: 14 }} />}
          </IconButton>
        </Box>
      </Box>

      <Collapse in={isExpanded}>
        <Divider sx={{ borderColor: CARD_BORDER }} />
        <Box sx={{ p: 1.5 }}>
          <Box sx={{ display: "flex", justifyContent: "space-between", alignItems: "center", mb: 1 }}>
            <Typography variant="caption" color="text.secondary" sx={{ fontWeight: 600 }}>
              接入点
            </Typography>
            <Button
              size="small"
              startIcon={<AddIcon sx={{ fontSize: 12 }} />}
              onClick={() => onAddEndpoint(provider.id)}
              sx={{ fontSize: 10, borderRadius: 1.5 }}
            >
              新增接入点
            </Button>
          </Box>

          {providerEndpoints.length === 0 ? (
            <Typography variant="caption" color="text.disabled" sx={{ display: "block", textAlign: "center", py: 1.5 }}>
              暂无接入点
            </Typography>
          ) : (
            <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
              {providerEndpoints.map((endpoint) => {
                const endpointModels = models.filter((m) => m.endpointId === endpoint.id);

                return (
                  <EndpointCard
                    key={endpoint.id}
                    endpoint={endpoint}
                    models={endpointModels}
                    expanded={expandedEndpoints.has(endpoint.id)}
                    onToggle={() => onToggleEndpoint(endpoint.id)}
                    testResult={testResults.get(endpoint.id)}
                    isTesting={testingId === endpoint.id}
                    modelTestResults={modelTestResults}
                    testingModelId={testingModelId}
                    activeModelId={activeModelId}
                    onTest={() => onTestConnection(endpoint.id)}
                    onEdit={() => onEditEndpoint(endpoint, provider.id)}
                    onDelete={() => onDeleteEndpoint(endpoint)}
                    onAddModel={() => onAddModel(endpoint.id)}
                    onEditModel={(m) => onEditModel(m, endpoint.id)}
                    onDeleteModel={onDeleteModel}
                    onTestModel={onTestModel}
                    onSetActiveModel={onSetActiveModel}
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
