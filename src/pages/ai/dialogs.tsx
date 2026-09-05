import { useEffect, useState } from "react";
import {
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  FormControl,
  FormControlLabel,
  IconButton,
  InputAdornment,
  InputLabel,
  MenuItem,
  Select,
  Switch,
  TextField,
  Typography,
} from "@mui/material";
import { Visibility, VisibilityOff } from "@mui/icons-material";
import type { AiEndpointDto, AiModelDto, AiProviderDto } from "../../types";
import { API_TYPE_OPTIONS } from "./shared";

/**
 * AI 助手四个对话框（供应商/接入点/模型/删除确认），从 AiAssistant.tsx 原样抽出，行为 1:1。
 */

export function ProviderDialog({
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

export function EndpointDialog({
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

export function ModelDialog({
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

export function ConfirmDialog({
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
