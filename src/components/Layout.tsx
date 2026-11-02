import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Box, ButtonBase, CssBaseline, Divider, Stack, Typography } from "@mui/material";
import {
  AccountTree,
  BoltOutlined,
  BuildCircleRounded,
  DashboardRounded,
  DnsOutlined,
  Settings as SettingsIcon,
  SettingsOutlined,
  SmartToy,
  StorageOutlined,
} from "@mui/icons-material";
import { NavLink, Outlet, useLocation } from "react-router-dom";
import TitleBar from "./TitleBar";
import pkg from "../../package.json";

/**
 * 侧边栏分组导航（v0.2.46 信息架构重构）：
 * 顶部 6 页签放不下「MySQL/Redis 独立入口 + 配置中心 + 远程部署」，
 * 改左侧固定栏按运维心智分四组——服务运维 / 部署修复 / 数据管理 / 系统工具。
 * 服务详情（/service/:id）不高亮任何条目，返回入口在页内。
 */
const NAV_GROUPS: { title: string; items: { to: string; label: string; icon: React.ReactNode; end?: boolean }[] }[] = [
  {
    title: "服务运维",
    items: [
      { to: "/", label: "服务总览", icon: <DashboardRounded fontSize="small" />, end: true },
      { to: "/topology", label: "依赖拓扑", icon: <AccountTree fontSize="small" /> },
    ],
  },
  {
    title: "部署修复",
    items: [{ to: "/env", label: "修复中心", icon: <BuildCircleRounded fontSize="small" /> }],
  },
  {
    title: "数据管理",
    items: [
      { to: "/mysql", label: "MySQL 管理", icon: <StorageOutlined fontSize="small" /> },
      { to: "/redis", label: "Redis 管理", icon: <BoltOutlined fontSize="small" /> },
    ],
  },
  {
    title: "系统工具",
    items: [
      { to: "/config", label: "配置中心", icon: <SettingsOutlined fontSize="small" /> },
      { to: "/deploy", label: "远程部署", icon: <DnsOutlined fontSize="small" /> },
      { to: "/ai", label: "AI 助手", icon: <SmartToy fontSize="small" /> },
      { to: "/settings", label: "设置", icon: <SettingsIcon fontSize="small" /> },
    ],
  },
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
  const location = useLocation();
  // 服务详情归入「服务总览」的语境：选中态跟随来源入口，避免详情页左侧空白无归属
  const isServiceDetail = location.pathname.startsWith("/service/");

  return (
    <Box sx={{ display: "flex", flexDirection: "column", height: "100vh" }}>
      <CssBaseline />
      {customTitlebar && <TitleBar />}
      <Box sx={{ display: "flex", flex: 1, minHeight: 0 }}>
        {/* ===== 侧边导航栏 ===== */}
        <Box
          component="nav"
          sx={{
            width: 224,
            flexShrink: 0,
            display: "flex",
            flexDirection: "column",
            bgcolor: "background.paper",
            borderRight: 1,
            borderColor: "divider",
          }}
        >
          {/* 品牌区 */}
          <Stack direction="row" alignItems="center" spacing={1.25} sx={{ px: 2, py: 2 }}>
            <Box
              component="img"
              src="/logo.png"
              alt="DFY运维平台"
              sx={{
                height: 34,
                width: 34,
                borderRadius: "50%",
                backgroundColor: "#fff",
                p: "2px",
                objectFit: "contain",
                boxShadow: 1,
                border: 1,
                borderColor: "divider",
              }}
            />
            <Box>
              <Typography variant="subtitle1" sx={{ fontWeight: 700, lineHeight: 1.2, letterSpacing: 0.3 }}>
                DFY 运维平台
              </Typography>
              <Typography variant="caption" color="text.secondary">
                v{pkg.version}
              </Typography>
            </Box>
          </Stack>
          <Divider />
          <Box sx={{ flex: 1, overflowY: "auto", py: 1 }}>
            {NAV_GROUPS.map((group) => (
              <Box key={group.title} sx={{ mb: 0.5 }}>
                <Typography
                  variant="overline"
                  sx={{ px: 2, pt: 1.25, pb: 0.5, display: "block", color: "text.disabled", fontWeight: 600 }}
                >
                  {group.title}
                </Typography>
                {group.items.map((item) => (
                  <NavLink
                    key={item.to}
                    to={item.to}
                    end={item.end}
                    style={{ textDecoration: "none" }}
                  >
                    {({ isActive }) => {
                      const active = isActive || (item.to === "/" && isServiceDetail);
                      return (
                        <ButtonBase
                          sx={{
                            width: 1,
                            justifyContent: "flex-start",
                            gap: 1.25,
                            px: 2,
                            py: 1,
                            borderRadius: 0,
                            textTransform: "none",
                            fontSize: "0.875rem",
                            fontWeight: active ? 600 : 500,
                            color: active ? "primary.main" : "text.primary",
                            bgcolor: active ? "rgba(22,58,94,0.08)" : "transparent",
                            borderLeft: "3px solid",
                            borderColor: active ? "primary.main" : "transparent",
                            transition: "background-color .12s ease, color .12s ease",
                            "&:hover": { bgcolor: "action.hover" },
                          }}
                        >
                          {item.icon}
                          {item.label}
                        </ButtonBase>
                      );
                    }}
                  </NavLink>
                ))}
              </Box>
            ))}
          </Box>
        </Box>

        {/* ===== 主内容区 ===== */}
        <Box
          component="main"
          sx={{
            flex: 1,
            minHeight: 0,
            overflow: "auto",
            p: 4,
            // 与 gateway-console 一致的浅灰渐变背景
            background: "linear-gradient(135deg, #f5f7fa 0%, #e9ecef 100%)",
          }}
        >
          <Outlet />
        </Box>
      </Box>
    </Box>
  );
}
