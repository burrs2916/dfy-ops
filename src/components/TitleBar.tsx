import { useEffect, useState } from "react";
import { Box, IconButton, Typography } from "@mui/material";
import { getCurrentWindow } from "@tauri-apps/api/window";

/** Windows 窗口三键的内联 SVG（贴原生风格细线图标，不引额外依赖） */
function IconMinimize() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
      <path d="M0 5h10" stroke="currentColor" strokeWidth="1" />
    </svg>
  );
}

function IconMaximize({ restored }: { restored: boolean }) {
  return restored ? (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
      <path
        d="M2.5.5h7v7"
        fill="none"
        stroke="currentColor"
        strokeWidth="1"
      />
      <rect
        x="0.5"
        y="2.5"
        width="7"
        height="7"
        fill="none"
        stroke="currentColor"
        strokeWidth="1"
      />
    </svg>
  ) : (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
      <rect
        x="0.5"
        y="0.5"
        width="9"
        height="9"
        fill="none"
        stroke="currentColor"
        strokeWidth="1"
      />
    </svg>
  );
}

function IconClose() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
      <path d="M0 0l10 10M10 0L0 10" stroke="currentColor" strokeWidth="1" />
    </svg>
  );
}

const winBtnSx = {
  width: 46,
  borderRadius: 0,
  color: "rgba(255,255,255,0.9)",
  "&:hover": { bgcolor: "rgba(255,255,255,0.14)" },
} as const;

/**
 * Windows 自绘标题栏（Rust 侧已对该平台关闭系统装饰）。
 *
 * 目的：去掉原生标题栏左上角的系统窗口小图标——界面 AppBar 已有品牌大
 * logo，小图标冗余且与主题风格割裂。本栏只放标题文字 + 窗口三键，
 * 不放任何图标；拖拽与双击最大化由 data-tauri-drag-region（Tauri 核心
 * 注入处理）承担。macOS 不渲染本组件，保持系统原生红绿灯按钮。
 */
export default function TitleBar() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    let disposed = false;
    const sync = async () => {
      try {
        setMaximized(await getCurrentWindow().isMaximized());
      } catch {
        // 非 Tauri 环境不会渲染本组件；此处仅为兜底
      }
    };
    void sync();
    const unlisten = getCurrentWindow().onResized(() => {
      if (!disposed) void sync();
    });
    return () => {
      disposed = true;
      void unlisten.then((f) => f());
    };
  }, []);

  return (
    <Box
      data-tauri-drag-region
      sx={{
        height: 36,
        flexShrink: 0,
        bgcolor: "primary.main",
        display: "flex",
        alignItems: "stretch",
        justifyContent: "space-between",
        userSelect: "none",
      }}
    >
      <Typography
        data-tauri-drag-region
        sx={{
          alignSelf: "center",
          ml: 1.5,
          fontSize: "12.5px",
          fontWeight: 500,
          color: "rgba(255,255,255,0.9)",
        }}
      >
        DFY运维平台
      </Typography>
      <Box sx={{ display: "flex" }}>
        <IconButton
          aria-label="最小化"
          disableRipple
          disableFocusRipple
          onClick={() => void getCurrentWindow().minimize()}
          sx={winBtnSx}
        >
          <IconMinimize />
        </IconButton>
        <IconButton
          aria-label={maximized ? "还原" : "最大化"}
          disableRipple
          disableFocusRipple
          onClick={() => void getCurrentWindow().toggleMaximize()}
          sx={winBtnSx}
        >
          <IconMaximize restored={maximized} />
        </IconButton>
        <IconButton
          aria-label="关闭"
          disableRipple
          disableFocusRipple
          onClick={() => void getCurrentWindow().close()}
          sx={{ ...winBtnSx, "&:hover": { bgcolor: "#e81123", color: "#fff" } }}
        >
          <IconClose />
        </IconButton>
      </Box>
    </Box>
  );
}
