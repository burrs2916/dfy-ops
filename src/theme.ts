import { createTheme, alpha } from "@mui/material/styles";

// 字体栈（逐级回退，跨平台安全，不依赖单一系统专属字体）：
// - 界面：Windows 命中 Segoe UI/微软雅黑，macOS 命中苹方，Linux 命中思源黑体
// - 数据/代码：等宽栈（Cascadia Code Win / SF Mono mac / Consolas / Menlo），
//   配合 tabular-nums 让数字列对齐，数据类界面观感立刻专业
const FONT_STACK =
  '"Segoe UI", "PingFang SC", "Microsoft YaHei", "Noto Sans SC", "Helvetica Neue", Arial, sans-serif';
const MONO_STACK =
  '"Cascadia Code", "JetBrains Mono", Consolas, "SF Mono", Menlo, "Courier New", monospace';

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
    text: {
      primary: "#1e2530",
      secondary: "#5b6572",
      disabled: "#9aa3af",
    },
    divider: "rgba(30, 37, 48, 0.12)",
  },
  shape: { borderRadius: 8 },
  typography: {
    fontFamily: FONT_STACK,
    // 中文界面适度字距（0.1~0.3px）提升精致感；标题 700 加粗、说明文字 13px
    h6: { fontSize: "1.125rem", fontWeight: 700, letterSpacing: 0.3 },
    subtitle1: { fontSize: "0.95rem", fontWeight: 700, lineHeight: 1.4, letterSpacing: 0.2 },
    subtitle2: { fontSize: "0.85rem", fontWeight: 600, letterSpacing: 0.1 },
    body1: { fontSize: "0.875rem", lineHeight: 1.55, letterSpacing: 0.1 },
    body2: { fontSize: "0.8125rem", lineHeight: 1.55, letterSpacing: 0.1 },
    caption: { fontSize: "0.75rem", letterSpacing: 0.2 },
    // 中文分组标题：overline 默认全大写+大字距对中文无意义，收敛为加粗小标
    overline: {
      fontSize: "0.78rem",
      fontWeight: 700,
      letterSpacing: 1,
      textTransform: "none",
      lineHeight: 1.6,
    },
    button: { textTransform: "none", fontWeight: 600, fontSize: "0.8125rem", letterSpacing: 0.2 },
  },
  components: {
    // ===== 按钮：统一圆角/内边距/字重；contained 悬停带品牌投影，outlined 统一淡边框 =====
    MuiButton: {
      defaultProps: { disableElevation: true },
      styleOverrides: {
        root: { borderRadius: 8, paddingInline: 14 },
        sizeSmall: { paddingInline: 10, fontSize: "0.78rem" },
        containedPrimary: {
          boxShadow: "none",
          "&:hover": { boxShadow: `0 2px 8px ${alpha("#0D9488", 0.35)}` },
        },
        outlined: {
          borderColor: "rgba(30, 37, 48, 0.18)",
          "&:hover": {
            borderColor: alpha("#0D9488", 0.55),
            backgroundColor: alpha("#0D9488", 0.04),
          },
        },
        text: {
          "&:hover": { backgroundColor: alpha("#0D9488", 0.06) },
        },
      },
    },
    // ===== 图标按钮：统一尺寸与圆角 =====
    MuiIconButton: {
      styleOverrides: { root: { borderRadius: 8 } },
    },
    // ===== 输入框：细边框 + 聚焦品牌色描边 + 平滑过渡 =====
    MuiOutlinedInput: {
      styleOverrides: {
        root: {
          backgroundColor: "#fff",
          transition: "box-shadow .15s ease",
          "& .MuiOutlinedInput-notchedOutline": { borderColor: "rgba(30, 37, 48, 0.16)" },
          "&:hover": { "& .MuiOutlinedInput-notchedOutline": { borderColor: alpha("#0D9488", 0.55) } },
          "&.Mui-focused": {
            "& .MuiOutlinedInput-notchedOutline": { borderColor: "#0D9488", borderWidth: 1.5 },
            boxShadow: `0 0 0 3px ${alpha("#0D9488", 0.12)}`,
          },
        },
        input: { fontSize: "0.8125rem" },
      },
    },
    MuiInputLabel: {
      styleOverrides: { root: { fontSize: "0.82rem" } },
    },
    MuiSelect: {
      styleOverrides: { select: { fontSize: "0.8125rem" } },
    },
    MuiMenuItem: {
      styleOverrides: { root: { fontSize: "0.8125rem", minHeight: 34 } },
    },
    // ===== Chip：小圆角紧凑标签，描边柔和 =====
    MuiChip: {
      styleOverrides: {
        root: { borderRadius: 6, fontWeight: 600, fontSize: "0.72rem" },
        sizeSmall: { height: 22 },
        outlined: {
          borderColor: "rgba(30, 37, 48, 0.18)",
          color: "#5b6572",
        },
      },
    },
    // ===== 表格：表头浅品牌底 + 小号加粗，正文 13px 数据感 =====
    MuiTableCell: {
      styleOverrides: {
        head: {
          fontWeight: 700,
          fontSize: "0.72rem",
          letterSpacing: 0.3,
          color: "#5b6572",
          backgroundColor: alpha("#0D9488", 0.045),
          whiteSpace: "nowrap",
        },
        root: { fontSize: "0.8125rem", padding: "7px 14px" },
      },
    },
    MuiTableRow: {
      styleOverrides: {
        root: { transition: "background-color .12s ease", "&:hover": { backgroundColor: alpha("#0D9488", 0.03) } },
      },
    },
    // ===== 页签：小写化 + 加粗 13px =====
    MuiTab: {
      styleOverrides: {
        root: { textTransform: "none", fontWeight: 600, fontSize: "0.85rem", minHeight: 42 },
      },
    },
    // ===== 卡片：去 MUI 灰纹理，柔和双层小阴影（克制、不压内容）=====
    MuiPaper: {
      styleOverrides: {
        root: {
          backgroundImage: "none",
          borderRadius: 10,
        },
        elevation1: {
          boxShadow: "0 1px 2px rgba(16, 24, 40, 0.04), 0 1px 3px rgba(16, 24, 40, 0.06)",
        },
      },
    },
    // ===== 页签：指示器圆角短条，更轻盈 =====
    MuiTabs: {
      styleOverrides: {
        indicator: { borderRadius: "3px 3px 0 0", height: 3 },
      },
    },
    // ===== 弹窗/提示 =====
    MuiDialog: {
      styleOverrides: { paper: { borderRadius: 12 } },
    },
    MuiDialogTitle: {
      styleOverrides: { root: { fontSize: "1rem", fontWeight: 700 } },
    },
    MuiAlert: {
      styleOverrides: {
        root: { borderRadius: 8, fontSize: "0.8125rem", alignItems: "center" },
      },
    },
    MuiTooltip: {
      defaultProps: { arrow: true },
      styleOverrides: {
        tooltip: { fontSize: "0.72rem", backgroundColor: "#263140", borderRadius: 6, padding: "5px 9px" },
        arrow: { color: "#263140" },
      },
    },
    MuiLinearProgress: {
      styleOverrides: { root: { borderRadius: 2, backgroundColor: alpha("#0D9488", 0.12) } },
    },
    MuiSnackbarContent: {
      styleOverrides: { root: { borderRadius: 8 } },
    },
  },
});

export { FONT_STACK, MONO_STACK };
