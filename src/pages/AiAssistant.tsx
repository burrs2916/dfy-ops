import { useCallback, useEffect, useState } from "react";
import {
  Alert,
  Box,
  Button,
  Card,
  Chip,
  CircularProgress,
  Collapse,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Divider,
  FormControl,
  FormControlLabel,
  IconButton,
  InputAdornment,
  InputLabel,
  Menu,
  MenuItem,
  Select,
  Snackbar,
  Switch,
  TextField,
  Tooltip,
  Typography,
} from "@mui/material";
import {
  Add as AddIcon,
  Cable as CableIcon,
  CheckCircle as CheckCircleIcon,
  CloudOutlined as CloudIcon,
  DeleteOutline as DeleteIcon,
  EditOutlined as EditIcon,
  ExpandLess as ExpandLessIcon,
  ExpandMore as ExpandMoreIcon,
  PlayArrow as PlayIcon,
  SmartToyOutlined as ModelIcon,
  Star as StarIcon,
  StarBorder as StarBorderIcon,
  Visibility,
  VisibilityOff,
  WarningAmber as WarningIcon,
} from "@mui/icons-material";
import { api } from "../api";
import type {
  AiEndpointDto,
  AiModelConfigDto,
  AiModelDto,
  AiProviderDto,
} from "../types";

/**
 * AI 助手 —— 模型配置页。
 * 结构与 web-craft ModelConfigPage 1:1 同构：三层配置（供应商 → 接入点 → 模型），
 * 折叠卡片 + 快速添加预设 + 连接/模型测试 + 删除确认。
 * 诊断时后端按 model→endpoint→provider 动态解析归属链，绝不写死。
 */

const API_TYPE_OPTIONS: { value: string; label: string }[] = [
  { value: "openai-completions", label: "OpenAI 兼容 (Chat Completions)" },
  { value: "anthropic-messages", label: "Anthropic Messages" },
  { value: "ollama", label: "Ollama 本地" },
];

const PROVIDER_PRESETS = ["OpenAI", "Anthropic", "DeepSeek", "Google", "Ollama"];

/// 从字符串生成确定性颜色（相同名称永远得到相同颜色）
const PROVIDER_COLORS = [
  "#60A5FA", "#38BDF8", "#00E676", "#FBBF24", "#FF6B6B",
  "#26C6DA", "#AB47BC", "#FFA726", "#66BB6A", "#5C6BC0",
  "#EC407A", "#7E57C2", "#0EA5E9", "#9CCC65", "#8D6E63",
];

function hashString(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) {
    h = ((h << 5) - h + s.charCodeAt(i)) | 0;
  }
  return Math.abs(h);
}

function getProviderColor(name: string): string {
  return PROVIDER_COLORS[hashString(name || "?") % PROVIDER_COLORS.length];
}

/// 供应商颜色圆点：根据名称哈希自动分配颜色，用于视觉区分
function ProviderDot({ name, size = 10 }: { name: string; size?: number }) {
  return (
    <Box
      sx={{
        width: size,
        height: size,
        borderRadius: "50%",
        bgcolor: getProviderColor(name),
        flexShrink: 0,
      }}
    />
  );
}

// ===== web-craft 同款配色（浅色主题分支） =====
const PRIMARY_COLOR = "#1D4ED8";
const CARD_BORDER = "rgba(0,0,0,0.08)";
const CARD_BG = "rgba(255,255,255,0.8)";
const INNER_CARD_BG = "rgba(245,245,245,0.6)";
const HOVER_BG = "rgba(0,0,0,0.03)";
const HOVER_BG2 = "rgba(0,0,0,0.02)";
const ERROR_COLOR = "#DC2626";
const SUCCESS_COLOR = "#16A34A";
const DISABLED_COLOR = "#9E9E9E";

function getApiTypeLabel(apiType: string): string {
  return API_TYPE_OPTIONS.find((o) => o.value === apiType)?.label ?? apiType;
}

// ===== 对话框 =====

function ProviderDialog({
  open,
  onClose,
  onSave,
  initial,
}: {
  open: boolean;
  onClose: () => void;
  onSave: (p: AiProviderDto) => void;
  initial?: AiProviderDto | null;
}) {
  const [name, setName] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [showApiKey, setShowApiKey] = useState(false);
  const [enabled, setEnabled] = useState(true);

  useEffect(() => {
    if (initial) {
      setName(initial.name);
      setApiKey(initial.apiKey);
      setEnabled(initial.enabled);
    } else {
      setName("");
      setApiKey("");
      setEnabled(true);
    }
  }, [initial, open]);

  const handleSave = () => {
    if (!name.trim()) return;
    onSave({
      id: initial?.id || "",
      name: name.trim(),
      apiKey: apiKey.trim(),
      enabled,
    });
    onClose();
  };

  return (
    <Dialog open={open} onClose={onClose} maxWidth="sm" fullWidth>
      <DialogTitle>{initial ? "编辑供应商" : "新增供应商"}</DialogTitle>
      <DialogContent sx={{ display: "flex", flexDirection: "column", gap: 2, pt: "16px !important" }}>
        <TextField
          label="供应商名称"
          value={name}
          onChange={(e) => setName(e.target.value)}
          fullWidth
          size="small"
          placeholder="如 OpenAI、DeepSeek"
        />
        <TextField
          label="API Key"
          value={apiKey}
          onChange={(e) => setApiKey(e.target.value)}
          fullWidth
          size="small"
          type={showApiKey ? "text" : "password"}
          placeholder={initial?.keySet ? "已配置，留空则保留原 Key" : "sk-..."}
          InputProps={{
            endAdornment: (
              <InputAdornment position="end">
                <IconButton
                  aria-label="切换密码可见性"
                  onClick={() => setShowApiKey((v) => !v)}
                  edge="end"
                  size="small"
                >
                  {showApiKey ? <VisibilityOff fontSize="small" /> : <Visibility fontSize="small" />}
                </IconButton>
              </InputAdornment>
            ),
          }}
        />
        <FormControlLabel
          control={<Switch checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />}
          label="启用"
        />
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>取消</Button>
        <Button onClick={handleSave} variant="contained" disabled={!name.trim()}>
          保存
        </Button>
      </DialogActions>
    </Dialog>
  );
}

function EndpointDialog({
  open,
  onClose,
  onSave,
  initial,
  providerId,
}: {
  open: boolean;
  onClose: () => void;
  onSave: (e: AiEndpointDto) => void;
  initial?: AiEndpointDto | null;
  providerId: string;
}) {
  const [name, setName] = useState("");
  const [apiType, setApiType] = useState("openai-completions");
  const [baseUrl, setBaseUrl] = useState("");
  const [enabled, setEnabled] = useState(true);

  useEffect(() => {
    if (initial) {
      setName(initial.name);
      setApiType(initial.apiType);
      setBaseUrl(initial.baseUrl);
      setEnabled(initial.enabled);
    } else {
      setName("");
      setApiType("openai-completions");
      setBaseUrl("");
      setEnabled(true);
    }
  }, [initial, open]);

  // 新建时：按接口类型联动默认值（与 web-craft 行为一致）
  useEffect(() => {
    if (!initial && open) {
      if (apiType === "ollama") {
        setBaseUrl("http://localhost:11434/v1/");
      }
    }
  }, [apiType, initial, open]);

  const handleSave = () => {
    if (!name.trim()) return;
    onSave({
      id: initial?.id || "",
      providerId,
      name: name.trim(),
      apiType,
      baseUrl: baseUrl.trim(),
      enabled,
    });
    onClose();
  };

  return (
    <Dialog open={open} onClose={onClose} maxWidth="sm" fullWidth>
      <DialogTitle>{initial ? "编辑接入点" : "新增接入点"}</DialogTitle>
      <DialogContent sx={{ display: "flex", flexDirection: "column", gap: 2, pt: "16px !important" }}>
        <TextField
          label="接入点名称"
          value={name}
          onChange={(e) => setName(e.target.value)}
          fullWidth
          size="small"
          placeholder="如 官方 API、中转节点"
        />
        <FormControl size="small" fullWidth>
          <InputLabel>接口类型</InputLabel>
          <Select
            value={apiType}
            label="接口类型"
            onChange={(e) => setApiType(e.target.value)}
          >
            {API_TYPE_OPTIONS.map((o) => (
              <MenuItem key={o.value} value={o.value}>
                {o.label}
              </MenuItem>
            ))}
          </Select>
        </FormControl>
        <TextField
          label="Base URL"
          value={baseUrl}
          onChange={(e) => setBaseUrl(e.target.value)}
          fullWidth
          size="small"
          placeholder="https://api.openai.com/v1/"
        />
        <FormControlLabel
          control={<Switch checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />}
          label="启用"
        />
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>取消</Button>
        <Button onClick={handleSave} variant="contained" disabled={!name.trim()}>
          保存
        </Button>
      </DialogActions>
    </Dialog>
  );
}

function ModelDialog({
  open,
  onClose,
  onSave,
  initial,
  endpointId,
}: {
  open: boolean;
  onClose: () => void;
  onSave: (m: AiModelDto) => void;
  initial?: AiModelDto | null;
  endpointId: string;
}) {
  const [name, setName] = useState("");
  const [refKey, setRefKey] = useState("");
  const [enabled, setEnabled] = useState(true);

  useEffect(() => {
    if (initial) {
      setName(initial.name);
      setRefKey(initial.refKey);
      setEnabled(initial.enabled);
    } else {
      setName("");
      setRefKey("");
      setEnabled(true);
    }
  }, [initial, open]);

  const handleSave = () => {
    if (!name.trim() || !refKey.trim()) return;
    onSave({
      id: initial?.id || "",
      endpointId,
      name: name.trim(),
      refKey: refKey.trim(),
      enabled,
    });
    onClose();
  };

  return (
    <Dialog open={open} onClose={onClose} maxWidth="sm" fullWidth>
      <DialogTitle>{initial ? "编辑模型" : "新增模型"}</DialogTitle>
      <DialogContent sx={{ display: "flex", flexDirection: "column", gap: 2, pt: "16px !important" }}>
        <TextField
          label="模型名称"
          value={name}
          onChange={(e) => setName(e.target.value)}
          fullWidth
          size="small"
          placeholder="如 GPT-4o mini"
        />
        <TextField
          label="模型标识 (refKey)"
          value={refKey}
          onChange={(e) => setRefKey(e.target.value)}
          fullWidth
          size="small"
          placeholder="如 gpt-4o-mini"
          helperText="调用 API 时实际传给服务商的模型标识"
        />
        <FormControlLabel
          control={<Switch checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />}
          label="启用"
        />
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>取消</Button>
        <Button onClick={handleSave} variant="contained" disabled={!name.trim() || !refKey.trim()}>
          保存
        </Button>
      </DialogActions>
    </Dialog>
  );
}

function ConfirmDialog({
  open,
  title,
  message,
  onConfirm,
  onCancel,
}: {
  open: boolean;
  title: string;
  message: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <Dialog open={open} onClose={onCancel} maxWidth="xs" fullWidth>
      <DialogTitle>{title}</DialogTitle>
      <DialogContent>
        <Typography variant="body2" sx={{ whiteSpace: "pre-wrap" }}>
          {message}
        </Typography>
      </DialogContent>
      <DialogActions>
        <Button onClick={onCancel}>取消</Button>
        <Button onClick={onConfirm} variant="contained" color="error">
          删除
        </Button>
      </DialogActions>
    </Dialog>
  );
}

// ===== 主页面 =====

export default function AiAssistant() {
  const [config, setConfig] = useState<AiModelConfigDto | null>(null);
  const [loading, setLoading] = useState(true);
  const [providerDialog, setProviderDialog] = useState<{ open: boolean; data: AiProviderDto | null }>({ open: false, data: null });
  const [endpointDialog, setEndpointDialog] = useState<{ open: boolean; data: AiEndpointDto | null; providerId: string }>({ open: false, data: null, providerId: "" });
  const [modelDialog, setModelDialog] = useState<{ open: boolean; data: AiModelDto | null; endpointId: string }>({ open: false, data: null, endpointId: "" });
  const [deleteConfirm, setDeleteConfirm] = useState<{ open: boolean; title: string; message: string; onConfirm: () => void }>({ open: false, title: "", message: "", onConfirm: () => {} });
  const [snackbar, setSnackbar] = useState<{ open: boolean; message: string; severity: "success" | "error" }>({ open: false, message: "", severity: "success" });
  const [testResults, setTestResults] = useState<Map<string, { success: boolean; message: string }>>(new Map());
  const [testingId, setTestingId] = useState<string | null>(null);
  const [modelTestResults, setModelTestResults] = useState<Map<string, { success: boolean; message: string }>>(new Map());
  const [testingModelId, setTestingModelId] = useState<string | null>(null);
  const [expandedProviders, setExpandedProviders] = useState<Set<string>>(new Set());
  const [expandedEndpoints, setExpandedEndpoints] = useState<Set<string>>(new Set());
  const [presetAnchor, setPresetAnchor] = useState<HTMLElement | null>(null);

  const providers = config?.providers ?? [];
  const endpoints = config?.endpoints ?? [];
  const models = config?.models ?? [];
  const activeModelId = config?.activeModelId ?? null;

  useEffect(() => {
    (async () => {
      try {
        const cfg = await api.aiConfig();
        setConfig(cfg);
        setExpandedProviders(new Set(cfg.providers.map((p) => p.id)));
      } catch (e) {
        setSnackbar({ open: true, message: e instanceof Error ? e.message : "加载失败", severity: "error" });
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  const toggleProvider = useCallback((id: string) => {
    setExpandedProviders((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const toggleEndpoint = useCallback((id: string) => {
    setExpandedEndpoints((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const handleTestConnection = useCallback(async (endpointId: string) => {
    setTestingId(endpointId);
    try {
      const result = await api.testAiEndpoint(endpointId);
      const message = result.ok ? result.message || "连接成功" : result.error || "连接失败";
      setTestResults((prev) => new Map(prev).set(endpointId, { success: result.ok, message }));
      setSnackbar({ open: true, message, severity: result.ok ? "success" : "error" });
    } catch (e) {
      const message = e instanceof Error ? e.message : "连接失败";
      setTestResults((prev) => new Map(prev).set(endpointId, { success: false, message }));
      setSnackbar({ open: true, message, severity: "error" });
    } finally {
      setTestingId(null);
    }
  }, []);

  const handleTestModel = useCallback(async (modelId: string) => {
    setTestingModelId(modelId);
    try {
      const result = await api.testAiModel(modelId);
      const message = result.ok ? result.message || "测试通过" : result.error || "测试失败";
      setModelTestResults((prev) => new Map(prev).set(modelId, { success: result.ok, message }));
      setSnackbar({ open: true, message, severity: result.ok ? "success" : "error" });
    } catch (e) {
      const message = e instanceof Error ? e.message : "测试失败";
      setModelTestResults((prev) => new Map(prev).set(modelId, { success: false, message }));
      setSnackbar({ open: true, message, severity: "error" });
    } finally {
      setTestingModelId(null);
    }
  }, []);

  const handleAddPreset = useCallback(async (presetName: string) => {
    try {
      let cfg = await api.saveAiProvider({ id: "", name: presetName, apiKey: "", enabled: true });
      // Ollama 预设：自动创建默认接入点，方便直接检测本地模型（与 web-craft 一致）
      if (presetName === "Ollama") {
        const provider = cfg.providers.find((p) => p.name === presetName);
        if (provider) {
          cfg = await api.saveAiEndpoint({
            id: "",
            providerId: provider.id,
            name: "Default",
            apiType: "ollama",
            baseUrl: "http://localhost:11434/v1/",
            enabled: true,
          });
        }
      }
      setConfig(cfg);
      setExpandedProviders((prev) => {
        const next = new Set(prev);
        cfg.providers.filter((p) => p.name === presetName).forEach((p) => next.add(p.id));
        return next;
      });
      setSnackbar({ open: true, message: `已添加供应商：${presetName}`, severity: "success" });
    } catch (e) {
      setSnackbar({ open: true, message: e instanceof Error ? e.message : "添加失败", severity: "error" });
    }
    setPresetAnchor(null);
  }, []);

  const saveProvider = useCallback(async (p: AiProviderDto) => {
    try {
      setConfig(await api.saveAiProvider(p));
      setSnackbar({ open: true, message: "供应商已保存", severity: "success" });
    } catch (e) {
      setSnackbar({ open: true, message: e instanceof Error ? e.message : "保存失败", severity: "error" });
    }
  }, []);

  const saveEndpoint = useCallback(async (ep: AiEndpointDto) => {
    try {
      setConfig(await api.saveAiEndpoint(ep));
      setSnackbar({ open: true, message: "接入点已保存", severity: "success" });
    } catch (e) {
      setSnackbar({ open: true, message: e instanceof Error ? e.message : "保存失败", severity: "error" });
    }
  }, []);

  const saveModel = useCallback(async (m: AiModelDto) => {
    try {
      setConfig(await api.saveAiModel(m));
      setSnackbar({ open: true, message: "模型已保存", severity: "success" });
    } catch (e) {
      setSnackbar({ open: true, message: e instanceof Error ? e.message : "保存失败", severity: "error" });
    }
  }, []);

  const setActiveModel = useCallback(async (modelId: string) => {
    try {
      setConfig(await api.setActiveAiModel(modelId));
    } catch (e) {
      setSnackbar({ open: true, message: e instanceof Error ? e.message : "设置失败", severity: "error" });
    }
  }, []);

  const getEndpointsForProvider = useCallback(
    (providerId: string) => endpoints.filter((e) => e.providerId === providerId),
    [endpoints],
  );

  const getModelsForEndpoint = useCallback(
    (endpointId: string) => models.filter((m) => m.endpointId === endpointId),
    [models],
  );

  if (loading) {
    return (
      <Box
        sx={{
          mx: -3,
          my: -3,
          height: "calc(100vh - 64px)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <CircularProgress size={28} />
      </Box>
    );
  }

  return (
    <Box sx={{ mx: -3, my: -3, p: 2, height: "calc(100vh - 64px)", overflow: "auto" }}>
      <Box sx={{ mb: 2, display: "flex", alignItems: "center", justifyContent: "space-between" }}>
        <Box>
          <Typography variant="subtitle2" sx={{ fontWeight: 700, fontSize: 14 }}>
            模型配置
          </Typography>
          <Typography variant="caption" color="text.secondary">
            配置 AI 诊断使用的供应商、接入点与模型（三层结构，动态解析归属）
          </Typography>
        </Box>
        <Box sx={{ display: "flex", gap: 1 }}>
          <Button
            size="small"
            variant="outlined"
            startIcon={<CloudIcon sx={{ fontSize: 14 }} />}
            onClick={(e) => setPresetAnchor(e.currentTarget)}
            sx={{ borderRadius: 2, fontSize: 11 }}
          >
            快速添加
          </Button>
          <Button
            size="small"
            variant="contained"
            startIcon={<AddIcon sx={{ fontSize: 14 }} />}
            onClick={() => setProviderDialog({ open: true, data: null })}
            sx={{
              borderRadius: 2,
              fontSize: 11,
              background: `linear-gradient(135deg, ${PRIMARY_COLOR} 0%, #3B82F6 100%)`,
            }}
          >
            新增供应商
          </Button>
        </Box>
      </Box>

      <Menu open={Boolean(presetAnchor)} anchorEl={presetAnchor} onClose={() => setPresetAnchor(null)}>
        {PROVIDER_PRESETS.map((preset) => (
          <MenuItem key={preset} onClick={() => handleAddPreset(preset)}>
            <ProviderDot name={preset} size={10} />
            <Box sx={{ ml: 1.5 }} />
            {preset}
          </MenuItem>
        ))}
      </Menu>

      {providers.length === 0 ? (
        <Card sx={{ border: 1, borderColor: "divider", borderRadius: 2, textAlign: "center", py: 6, bgcolor: CARD_BG }}>
          <CloudIcon sx={{ fontSize: 48, color: "#555", mb: 1 }} />
          <Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>
            还没有配置任何供应商
          </Typography>
          <Typography variant="caption" color="text.disabled" sx={{ mb: 2, display: "block" }}>
            通过「快速添加」一键接入常用供应商，或手动新增
          </Typography>
          <Button
            variant="outlined"
            size="small"
            startIcon={<AddIcon sx={{ fontSize: 14 }} />}
            onClick={() => setProviderDialog({ open: true, data: null })}
            sx={{ borderRadius: 2 }}
          >
            新增供应商
          </Button>
        </Card>
      ) : (
        <Box sx={{ display: "flex", flexDirection: "column", gap: 1.5 }}>
          {providers.map((provider) => {
            const providerEndpoints = getEndpointsForProvider(provider.id);
            const isExpanded = expandedProviders.has(provider.id);

            return (
              <Card key={provider.id} sx={{ border: 1, borderColor: CARD_BORDER, borderRadius: 2, bgcolor: CARD_BG, overflow: "visible" }}>
                <Box
                  sx={{
                    p: 1.5,
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    cursor: "pointer",
                    "&:hover": { bgcolor: HOVER_BG },
                  }}
                  onClick={() => toggleProvider(provider.id)}
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
                      <IconButton size="small" onClick={() => setProviderDialog({ open: true, data: provider })}>
                        <EditIcon sx={{ fontSize: 14 }} />
                      </IconButton>
                    </Tooltip>
                    <Tooltip title="删除供应商">
                      <IconButton
                        size="small"
                        onClick={() => {
                          const affectedModels = models.filter((m) =>
                            endpoints.some((e) => e.providerId === provider.id && e.id === m.endpointId),
                          );
                          let msg = `确定删除供应商「${provider.name}」？`;
                          if (affectedModels.length > 0) {
                            msg += `\n\n其下 ${providerEndpoints.length} 个接入点与 ${affectedModels.length} 个模型将一并删除。`;
                          }
                          setDeleteConfirm({
                            open: true,
                            title: "删除供应商",
                            message: msg,
                            onConfirm: async () => {
                              try {
                                setConfig(await api.deleteAiProvider(provider.id));
                              } catch (e) {
                                setSnackbar({ open: true, message: e instanceof Error ? e.message : "删除失败", severity: "error" });
                              }
                              setDeleteConfirm({ open: false, title: "", message: "", onConfirm: () => {} });
                            },
                          });
                        }}
                      >
                        <DeleteIcon sx={{ fontSize: 14, color: ERROR_COLOR }} />
                      </IconButton>
                    </Tooltip>
                    <IconButton size="small" onClick={() => toggleProvider(provider.id)}>
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
                        onClick={() => setEndpointDialog({ open: true, data: null, providerId: provider.id })}
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
                          const endpointModels = getModelsForEndpoint(endpoint.id);
                          const isEndpointExpanded = expandedEndpoints.has(endpoint.id);
                          const testResult = testResults.get(endpoint.id);
                          const isTesting = testingId === endpoint.id;

                          return (
                            <Card key={endpoint.id} variant="outlined" sx={{ borderRadius: 1.5, borderColor: CARD_BORDER, bgcolor: INNER_CARD_BG }}>
                              <Box
                                sx={{
                                  p: 1,
                                  display: "flex",
                                  alignItems: "center",
                                  justifyContent: "space-between",
                                  cursor: "pointer",
                                  "&:hover": { bgcolor: HOVER_BG2 },
                                }}
                                onClick={() => toggleEndpoint(endpoint.id)}
                              >
                                <Box sx={{ display: "flex", alignItems: "center", gap: 0.75 }}>
                                  <CableIcon sx={{ fontSize: 14, color: "#FBBF24" }} />
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
                                    <IconButton size="small" sx={{ p: 0.25 }} onClick={() => handleTestConnection(endpoint.id)}>
                                      <PlayIcon sx={{ fontSize: 12 }} />
                                    </IconButton>
                                  </Tooltip>
                                  <Tooltip title="编辑接入点">
                                    <IconButton size="small" sx={{ p: 0.25 }} onClick={() => setEndpointDialog({ open: true, data: endpoint, providerId: provider.id })}>
                                      <EditIcon sx={{ fontSize: 12 }} />
                                    </IconButton>
                                  </Tooltip>
                                  <Tooltip title="删除接入点">
                                    <IconButton
                                      size="small"
                                      sx={{ p: 0.25 }}
                                      onClick={() => {
                                        let msg = `确定删除接入点「${endpoint.name}」？`;
                                        if (endpointModels.length > 0) {
                                          msg += `\n\n其下 ${endpointModels.length} 个模型将一并删除。`;
                                        }
                                        setDeleteConfirm({
                                          open: true,
                                          title: "删除接入点",
                                          message: msg,
                                          onConfirm: async () => {
                                            try {
                                              setConfig(await api.deleteAiEndpoint(endpoint.id));
                                            } catch (e) {
                                              setSnackbar({ open: true, message: e instanceof Error ? e.message : "删除失败", severity: "error" });
                                            }
                                            setDeleteConfirm({ open: false, title: "", message: "", onConfirm: () => {} });
                                          },
                                        });
                                      }}
                                    >
                                      <DeleteIcon sx={{ fontSize: 12, color: ERROR_COLOR }} />
                                    </IconButton>
                                  </Tooltip>
                                  <IconButton size="small" sx={{ p: 0.25 }} onClick={() => toggleEndpoint(endpoint.id)}>
                                    {isEndpointExpanded ? <ExpandLessIcon sx={{ fontSize: 12 }} /> : <ExpandMoreIcon sx={{ fontSize: 12 }} />}
                                  </IconButton>
                                </Box>
                              </Box>

                              <Collapse in={isEndpointExpanded}>
                                <Divider sx={{ borderColor: CARD_BORDER }} />
                                <Box sx={{ p: 1 }}>
                                  <Box sx={{ display: "flex", justifyContent: "space-between", alignItems: "center", mb: 0.75 }}>
                                    <Typography variant="caption" color="text.secondary">
                                      模型 ({endpointModels.length})
                                    </Typography>
                                    <Button
                                      size="small"
                                      startIcon={<AddIcon sx={{ fontSize: 10 }} />}
                                      onClick={() => setModelDialog({ open: true, data: null, endpointId: endpoint.id })}
                                      sx={{ fontSize: 9, borderRadius: 1 }}
                                    >
                                      新增模型
                                    </Button>
                                  </Box>

                                  {endpointModels.length === 0 ? (
                                    <Typography variant="caption" color="text.disabled" sx={{ display: "block", textAlign: "center", py: 0.75 }}>
                                      暂无模型
                                    </Typography>
                                  ) : (
                                    <Box sx={{ display: "flex", flexDirection: "column", gap: 0.5 }}>
                                      {endpointModels.map((model) => {
                                        const modelTestResult = modelTestResults.get(model.id);
                                        const isModelTesting = testingModelId === model.id;
                                        const isActive = activeModelId === model.id;

                                        return (
                                          <Box
                                            key={model.id}
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
                                              {modelTestResult &&
                                                (modelTestResult.success ? (
                                                  <Tooltip title={modelTestResult.message}>
                                                    <CheckCircleIcon sx={{ fontSize: 10, color: SUCCESS_COLOR }} />
                                                  </Tooltip>
                                                ) : (
                                                  <Tooltip title={modelTestResult.message}>
                                                    <WarningIcon sx={{ fontSize: 10, color: ERROR_COLOR }} />
                                                  </Tooltip>
                                                ))}
                                              {isModelTesting && <CircularProgress size={10} />}
                                              <Tooltip title={isActive ? "当前诊断模型" : "设为诊断模型"}>
                                                <IconButton size="small" sx={{ p: 0.15 }} onClick={(e) => { e.stopPropagation(); setActiveModel(model.id); }}>
                                                  {isActive ? (
                                                    <StarIcon sx={{ fontSize: 12, color: PRIMARY_COLOR }} />
                                                  ) : (
                                                    <StarBorderIcon sx={{ fontSize: 12 }} />
                                                  )}
                                                </IconButton>
                                              </Tooltip>
                                              <Tooltip title="测试模型">
                                                <IconButton size="small" sx={{ p: 0.15 }} onClick={(e) => { e.stopPropagation(); handleTestModel(model.id); }}>
                                                  <PlayIcon sx={{ fontSize: 10 }} />
                                                </IconButton>
                                              </Tooltip>
                                              <IconButton size="small" sx={{ p: 0.15 }} onClick={() => setModelDialog({ open: true, data: model, endpointId: endpoint.id })}>
                                                <EditIcon sx={{ fontSize: 10 }} />
                                              </IconButton>
                                              <IconButton
                                                size="small"
                                                sx={{ p: 0.15 }}
                                                onClick={() => {
                                                  setDeleteConfirm({
                                                    open: true,
                                                    title: "删除模型",
                                                    message: `确定删除模型「${model.name}」？${isActive ? "\n\n该模型当前为诊断模型，删除后诊断增强将不可用。" : ""}`,
                                                    onConfirm: async () => {
                                                      try {
                                                        setConfig(await api.deleteAiModel(model.id));
                                                      } catch (e) {
                                                        setSnackbar({ open: true, message: e instanceof Error ? e.message : "删除失败", severity: "error" });
                                                      }
                                                      setDeleteConfirm({ open: false, title: "", message: "", onConfirm: () => {} });
                                                    },
                                                  });
                                                }}
                                              >
                                                <DeleteIcon sx={{ fontSize: 10, color: ERROR_COLOR }} />
                                              </IconButton>
                                            </Box>
                                          </Box>
                                        );
                                      })}
                                    </Box>
                                  )}
                                </Box>
                              </Collapse>
                            </Card>
                          );
                        })}
                      </Box>
                    )}
                  </Box>
                </Collapse>
              </Card>
            );
          })}
        </Box>
      )}

      <ProviderDialog
        open={providerDialog.open}
        onClose={() => setProviderDialog({ open: false, data: null })}
        onSave={saveProvider}
        initial={providerDialog.data}
      />

      <EndpointDialog
        open={endpointDialog.open}
        onClose={() => setEndpointDialog({ open: false, data: null, providerId: "" })}
        onSave={saveEndpoint}
        initial={endpointDialog.data}
        providerId={endpointDialog.providerId}
      />

      <ModelDialog
        open={modelDialog.open}
        onClose={() => setModelDialog({ open: false, data: null, endpointId: "" })}
        onSave={saveModel}
        initial={modelDialog.data}
        endpointId={modelDialog.endpointId}
      />

      <ConfirmDialog
        open={deleteConfirm.open}
        title={deleteConfirm.title}
        message={deleteConfirm.message}
        onConfirm={deleteConfirm.onConfirm}
        onCancel={() => setDeleteConfirm({ open: false, title: "", message: "", onConfirm: () => {} })}
      />

      <Snackbar
        open={snackbar.open}
        autoHideDuration={3000}
        onClose={() => setSnackbar({ ...snackbar, open: false })}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
      >
        <Alert severity={snackbar.severity} onClose={() => setSnackbar({ ...snackbar, open: false })} variant="filled" sx={{ fontSize: 12 }}>
          {snackbar.message}
        </Alert>
      </Snackbar>
    </Box>
  );
}
