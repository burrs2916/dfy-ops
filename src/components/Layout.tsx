import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  AppBar,
  Box,
  CssBaseline,
  Toolbar,
  Typography,
  Button,
} from "@mui/material";
import { Settings as SettingsIcon, AccountTree, SmartToy } from "@mui/icons-material";
import { NavLink, Outlet } from "react-router-dom";
import TitleBar from "./TitleBar";

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
      <AppBar position="static" elevation={0}>
        <Toolbar>
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
          <Typography variant="h6" sx={{ fontWeight: 600, mr: 3 }}>
            DFY运维平台
          </Typography>
          <Box sx={{ display: "flex", gap: 1 }}>
            <Button
              component={NavLink}
              to="/"
              color="inherit"
              sx={{
                textTransform: "none",
                "&.active": { bgcolor: "rgba(255,255,255,0.16)" },
              }}
            >
              服务总览
            </Button>
            <Button
              component={NavLink}
              to="/env"
              color="inherit"
              sx={{
                textTransform: "none",
                "&.active": { bgcolor: "rgba(255,255,255,0.16)" },
              }}
            >
              环境检查
            </Button>
            <Button
              component={NavLink}
              to="/topology"
              color="inherit"
              startIcon={<AccountTree />}
              sx={{
                textTransform: "none",
                "&.active": { bgcolor: "rgba(255,255,255,0.16)" },
              }}
            >
              依赖拓扑
            </Button>
            <Button
              component={NavLink}
              to="/ai"
              color="inherit"
              startIcon={<SmartToy />}
              sx={{
                textTransform: "none",
                "&.active": { bgcolor: "rgba(255,255,255,0.16)" },
              }}
            >
              AI助手
            </Button>
            <Button
              component={NavLink}
              to="/settings"
              color="inherit"
              startIcon={<SettingsIcon />}
              sx={{
                textTransform: "none",
                "&.active": { bgcolor: "rgba(255,255,255,0.16)" },
              }}
            >
              设置
            </Button>
          </Box>
        </Toolbar>
      </AppBar>
      <Box
        component="main"
        sx={{ flex: 1, overflow: "auto", p: 3, bgcolor: "#f5f6f8" }}
      >
        <Outlet />
      </Box>
    </Box>
  );
}
