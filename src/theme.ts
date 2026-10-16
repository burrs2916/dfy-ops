import { createTheme } from "@mui/material/styles";

// 品牌青蓝主色（logo 渐变起点 #0D9488 → #0EA5E9）。success 也用品牌色：
// 全项目不允许出现绿色系（用户明确要求），「成功」语义统一青蓝呈现。
export const theme = createTheme({
  palette: {
    mode: "light",
    primary: { main: "#0D9488" },
    secondary: { main: "#0EA5E9" },
    success: { main: "#0D9488" },
    error: { main: "#A32D2D" },
    warning: { main: "#BA7517" },
  },
  typography: {
    fontFamily:
      '"Segoe UI", "PingFang SC", "Microsoft YaHei", system-ui, sans-serif',
  },
});
