import { Box } from "@mui/material";

/**
 * AI 助手共享常量与小组件（从 AiAssistant.tsx 原样抽出，行为 1:1）。
 */

/// 连接/模型测试结果（endpoint 测试与 model 测试同构）
export type TestResult = { success: boolean; message: string };

export const API_TYPE_OPTIONS: { value: string; label: string }[] = [
  { value: "openai-completions", label: "OpenAI 兼容 (Chat Completions)" },
  { value: "anthropic-messages", label: "Anthropic Messages" },
  { value: "ollama", label: "Ollama 本地" },
];

export const PROVIDER_PRESETS = ["OpenAI", "Anthropic", "DeepSeek", "Google", "Ollama"];

/// 从字符串生成确定性颜色（相同名称永远得到相同颜色）
// 色板不含绿色系（用户要求全项目无绿色），绿色位以品牌青蓝替代
export const PROVIDER_COLORS = [
  "#60A5FA", "#38BDF8", "#26C6DA", "#FBBF24", "#FF6B6B",
  "#22D3EE", "#AB47BC", "#FFA726", "#0EA5E9", "#5C6BC0",
  "#EC407A", "#7E57C2", "#0D9488", "#F59E0B", "#8D6E63",
];

export function hashString(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) {
    h = ((h << 5) - h + s.charCodeAt(i)) | 0;
  }
  return Math.abs(h);
}

export function getProviderColor(name: string): string {
  return PROVIDER_COLORS[hashString(name || "?") % PROVIDER_COLORS.length];
}

/// 供应商颜色圆点：根据名称哈希自动分配颜色，用于视觉区分
export function ProviderDot({ name, size = 10 }: { name: string; size?: number }) {
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
export const PRIMARY_COLOR = "#1D4ED8";
export const CARD_BORDER = "rgba(0,0,0,0.08)";
export const CARD_BG = "rgba(255,255,255,0.8)";
export const INNER_CARD_BG = "rgba(245,245,245,0.6)";
export const HOVER_BG = "rgba(0,0,0,0.03)";
export const HOVER_BG2 = "rgba(0,0,0,0.02)";
export const ERROR_COLOR = "#DC2626";
export const SUCCESS_COLOR = "#0D9488";
export const DISABLED_COLOR = "#9E9E9E";

export function getApiTypeLabel(apiType: string): string {
  return API_TYPE_OPTIONS.find((o) => o.value === apiType)?.label ?? apiType;
}
