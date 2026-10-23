import { useCallback, useEffect, useState } from "react";
import {
  Alert,
  Box,
  Button,
  Card,
  CircularProgress,
  Menu,
  MenuItem,
  Snackbar,
  Typography,
} from "@mui/material";
import {
  Add as AddIcon,
  CloudOutlined as CloudIcon,
} from "@mui/icons-material";
import { api } from "../api";
import type {
  AiEndpointDto,
  AiModelConfigDto,
  AiModelDto,
  AiProviderDto,
} from "../types";
import { ProviderCard } from "./ai/ProviderCard";
import { ConfirmDialog, EndpointDialog, ModelDialog, ProviderDialog } from "./ai/dialogs";
import { CARD_BG, PROVIDER_PRESETS, ProviderDot, type TestResult } from "./ai/shared";

/**
 * AI 助手 —— 模型配置页。
 * 结构与 web-craft ModelConfigPage 1:1 同构：三层配置（供应商 → 接入点 → 模型），
 * 折叠卡片 + 快速添加预设 + 连接/模型测试 + 删除确认。
 * 诊断时后端按 model→endpoint→provider 动态解析归属链，绝不写死。
 *
 * 组件拆分：卡片渲染在 ai/ProviderCard → EndpointCard → ModelRow，
 * 对话框在 ai/dialogs，共享常量在 ai/shared——本文件只保留状态与回调。
 */

export default function AiAssistant() {
  const [config, setConfig] = useState<AiModelConfigDto | null>(null);
  const [loading, setLoading] = useState(true);
  const [providerDialog, setProviderDialog] = useState<{ open: boolean; data: AiProviderDto | null }>({ open: false, data: null });
  const [endpointDialog, setEndpointDialog] = useState<{ open: boolean; data: AiEndpointDto | null; providerId: string }>({ open: false, data: null, providerId: "" });
  const [modelDialog, setModelDialog] = useState<{ open: boolean; data: AiModelDto | null; endpointId: string }>({ open: false, data: null, endpointId: "" });
  const [deleteConfirm, setDeleteConfirm] = useState<{ open: boolean; title: string; message: string; onConfirm: () => void }>({ open: false, title: "", message: "", onConfirm: () => {} });
  const [snackbar, setSnackbar] = useState<{ open: boolean; message: string; severity: "success" | "error" }>({ open: false, message: "", severity: "success" });
  const [testResults, setTestResults] = useState<Map<string, TestResult>>(new Map());
  const [testingId, setTestingId] = useState<string | null>(null);
  const [modelTestResults, setModelTestResults] = useState<Map<string, TestResult>>(new Map());
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

  // ===== 删除确认（消息统计级联影响，确认后调 api 并刷新 config） =====

  const confirmDeleteProvider = useCallback((provider: AiProviderDto) => {
    const providerEndpoints = endpoints.filter((e) => e.providerId === provider.id);
    const affectedModels = models.filter((m) => providerEndpoints.some((e) => e.id === m.endpointId));
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
  }, [endpoints, models]);

  const confirmDeleteEndpoint = useCallback((endpoint: AiEndpointDto) => {
    const endpointModels = models.filter((m) => m.endpointId === endpoint.id);
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
  }, [models]);

  const confirmDeleteModel = useCallback((model: AiModelDto, isActive: boolean) => {
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
  }, []);

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
              background: `linear-gradient(135deg, #1e3f66 0%, #4a6fa5 100%)`,
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
          {providers.map((provider) => (
            <ProviderCard
              key={provider.id}
              provider={provider}
              endpoints={endpoints.filter((e) => e.providerId === provider.id)}
              models={models}
              expanded={expandedProviders.has(provider.id)}
              onToggle={() => toggleProvider(provider.id)}
              expandedEndpoints={expandedEndpoints}
              onToggleEndpoint={toggleEndpoint}
              testResults={testResults}
              testingId={testingId}
              modelTestResults={modelTestResults}
              testingModelId={testingModelId}
              activeModelId={activeModelId}
              onEdit={() => setProviderDialog({ open: true, data: provider })}
              onDelete={() => confirmDeleteProvider(provider)}
              onAddEndpoint={(providerId) => setEndpointDialog({ open: true, data: null, providerId })}
              onEditEndpoint={(endpoint, providerId) => setEndpointDialog({ open: true, data: endpoint, providerId })}
              onDeleteEndpoint={confirmDeleteEndpoint}
              onTestConnection={handleTestConnection}
              onAddModel={(endpointId) => setModelDialog({ open: true, data: null, endpointId })}
              onEditModel={(model, endpointId) => setModelDialog({ open: true, data: model, endpointId })}
              onDeleteModel={confirmDeleteModel}
              onTestModel={handleTestModel}
              onSetActiveModel={(id) => setActiveModel(id)}
            />
          ))}
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
