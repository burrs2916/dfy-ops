import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AppBar, Box, CssBaseline, Toolbar, Typography, Button } from "@mui/material";
import {
  Settings as SettingsIcon,
  AccountTree,
  SmartToy,
  DashboardRounded,
  BuildCircleRounded,
  TuneRounded,
} from "@mui/icons-material";
import { NavLink, Outlet } from "react-router-dom";
import TitleBar from "./TitleBar";
import { BRAND_GRADIENT } from "../theme";

/** 顶部导航按钮统一样式：胶囊激活态 + 平滑过渡 */
const navBtn = {
  textTransform: "none" as const,
  fontWeight: 600 as const,
  borderRadius: 2,
  px: 1.75,
  py: 0.6,
  transition: "background-color .15s ease",
  "&:hover": { bgcolor: "rgba(255,255,255,0.12)" },
  "&.active": {
    bgcolor: "rgba(255,255,255,0.18)",
    boxShadow: "inset 0 0 0 1px rgba(255,255,255,0.25)",
  },
};

const NAV_ITEMS = [
  { to: "/", label: "服务总览", icon: <DashboardRounded /> },
  { to: "/env", label: "修复中心", icon: <BuildCircleRounded /> },
  { to: "/ops", label: "运维中心", icon: <TuneRounded /> },
  { to: "/topology", label: "依赖拓扑", icon: <AccountTree /> },
  { to: "/ai", label: "AI助手", icon: <SmartToy /> },
  { to: "/settings", label: "设置", icon: <SettingsIcon /> },
];

export default function Layout() {
  // 仅 Windows 渲染自绘标题栏（Rust 侧该平台已关闭系统装饰）；
  // macOS 保持系统原生标题栏。非 Tauri 环境调用失败，维持 false。
  const [customTitlebar, setCustomTitlebar] = useState(false);
  useEffect(() => {
    invoke<boolean>("uses_custom_titlebar")
      .then(setCustomTitlebar)
      .catch(() => {});
  }, []);

  return (
    <Box sx={{ display: "flex", flexDirection: "column", height: "100vh" }}>
      <CssBaseline />
      {customTitlebar && <TitleBar />}
      <AppBar
        position="static"
        elevation={0}
        sx={{
          background: BRAND_GRADIENT,
          borderRadius: 0,
        }}
      >
        <Toolbar sx={{ minHeight: { xs: 56 } }}>
          <Box
            component="img"
            src="/logo.png"
            alt="DFY运维平台"
            sx={{
              fontSize: "1.25rem",
              height: "2.1em",
              width: "2.1em",
              borderRadius: "50%",
              backgroundColor: "#fff",
              p: "2px",
              mr: 1.5,
              objectFit: "contain",
              boxShadow: 1,
            }}
          />
          <Typography variant="h6" sx={{ fontWeight: 700, mr: 3, letterSpacing: 0.3 }}>
            DFY运维平台
          </Typography>
          <Box sx={{ display: "flex", gap: 0.5 }}>
            {NAV_ITEMS.map((item) => (
              <Button key={item.to} component={NavLink} to={item.to} color="inherit" startIcon={item.icon} sx={navBtn}>
                {item.label}
              </Button>
            ))}
          </Box>
        </Toolbar>
      </AppBar>
      <Box
        component="main"
        sx={{
          flex: 1,
          overflow: "auto",
          p: 3,
          // 与 gateway-console 一致的浅灰渐变背景
          background: "linear-gradient(135deg, #f5f7fa 0%, #e9ecef 100%)",
        }}
      >
        <Outlet />
      </Box>
    </Box>
  );
}
