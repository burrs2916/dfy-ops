import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Box, ButtonBase, CssBaseline, Stack, Typography } from "@mui/material";
import { alpha } from "@mui/material/styles";
import {
  AccountTree,
  BoltOutlined,
  BuildCircleRounded,
  DashboardRounded,
  DnsOutlined,
  MonitorHeart,
  Settings as SettingsIcon,
  SettingsOutlined,
  SmartToy,
  StorageOutlined,
} from "@mui/icons-material";
import { NavLink, Outlet, useLocation } from "react-router-dom";
import TitleBar from "./TitleBar";
import { api } from "../api";
import pkg from "../../package.json";

/**
 * 侧边栏分组导航（v0.2.46 信息架构 / v0.2.63 视觉重构）：
 * 顶部 6 页签放不下「MySQL/Redis 独立入口 + 配置中心 + 远程部署」，
 * 改左侧固定栏按运维心智分四组——服务运维 / 部署修复 / 数据管理 / 系统工具。
 * 服务详情（/service/:id）不高亮任何条目，返回入口在页内。
 *
 * v0.2.63：深色品牌侧边栏——海军蓝底 + 白字 + 圆角胶囊选中态，
 * 与 Windows 自绘标题栏的品牌深蓝衔接成「深色框架 + 浅色内容」；
 * 底部常驻运维后端连接状态（心跳 10s），服务起不来时第一眼就知道是不是后端的问题。
 */
const NAV_GROUPS: { title: string; items: { to: string; label: string; icon: React.ReactNode; end?: boolean }[] }[] = [
  {
    title: "服务运维",
    items: [
      { to: "/", label: "服务总览", icon: <DashboardRounded fontSize="small" />, end: true },
      { to: "/monitor", label: "监控中心", icon: <MonitorHeart fontSize="small" /> },
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

/** 深色侧边栏底色：品牌海军蓝的两段纵向微渐变（与 TitleBar 品牌色同源） */
const SIDEBAR_BG = "linear-gradient(180deg, #1c3d61 0%, #16324f 100%)";

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

  // 运维后端心跳：内嵌 Axum（127.0.0.1:18721）是否可达。
  // 服务全部「未运行」时，用户分不清是服务挂了还是运维后端挂了——这里给答案。
  const [backend, setBackend] = useState<"ok" | "down" | "checking">("checking");
  useEffect(() => {
    let alive = true;
    const ping = () =>
      api
        .health()
        .then(() => alive && setBackend("ok"))
        .catch(() => alive && setBackend("down"));
    void ping();
    const id = window.setInterval(ping, 10000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, []);

  return (
    <Box sx={{ display: "flex", flexDirection: "column", height: "100vh" }}>
      <CssBaseline />
      {customTitlebar && <TitleBar />}
      <Box sx={{ display: "flex", flex: 1, minHeight: 0 }}>
        {/* ===== 侧边导航栏（深色品牌底） ===== */}
        <Box
          component="nav"
          sx={{
            width: 224,
            flexShrink: 0,
            display: "flex",
            flexDirection: "column",
            background: SIDEBAR_BG,
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
                flexShrink: 0,
              }}
            />
            <Box sx={{ minWidth: 0 }}>
              <Typography
                variant="subtitle1"
                sx={{ fontWeight: 700, lineHeight: 1.2, letterSpacing: 0.3, color: "#fff" }}
              >
                DFY 运维平台
              </Typography>
              <Typography variant="caption" sx={{ color: alpha("#fff", 0.45) }}>
                v{pkg.version}
              </Typography>
            </Box>
          </Stack>
          {/* 导航分组：圆角胶囊选中态 */}
          <Box sx={{ flex: 1, overflowY: "auto", py: 1, px: 1 }}>
            {NAV_GROUPS.map((group) => (
              <Box key={group.title} sx={{ mb: 0.75 }}>
                <Typography
                  variant="caption"
                  sx={{
                    px: 1.75,
                    pt: 1.5,
                    pb: 0.5,
                    display: "block",
                    color: alpha("#fff", 0.4),
                    fontWeight: 700,
                    letterSpacing: 1.5,
                    fontSize: "0.7rem",
                  }}
                >
                  {group.title}
                </Typography>
                {group.items.map((item) => (
                  <NavLink key={item.to} to={item.to} end={item.end} style={{ textDecoration: "none", display: "block" }}>
                    {({ isActive }) => {
                      const active = isActive || (item.to === "/" && isServiceDetail);
                      return (
                        <ButtonBase
                          sx={{
                            width: 1,
                            justifyContent: "flex-start",
                            gap: 1.25,
                            px: 1.75,
                            py: 0.9,
                            mb: 0.25,
                            borderRadius: 2,
                            textTransform: "none",
                            fontSize: "0.855rem",
                            fontWeight: active ? 600 : 500,
                            color: active ? "#fff" : alpha("#fff", 0.72),
                            bgcolor: active ? alpha("#fff", 0.14) : "transparent",
                            transition: "background-color .12s ease, color .12s ease",
                            "&:hover": {
                              bgcolor: active ? alpha("#fff", 0.18) : alpha("#fff", 0.06),
                              color: "#fff",
                            },
                            // 选中项左侧短胶囊指示条：比整条描边轻盈
                            position: "relative",
                            ...(active
                              ? {
                                  "&::before": {
                                    content: '""',
                                    position: "absolute",
                                    left: -4,
                                    top: "50%",
                                    transform: "translateY(-50%)",
                                    width: 3,
                                    height: 16,
                                    borderRadius: 2,
                                    backgroundColor: "#7ea9d9",
                                  },
                                }
                              : {}),
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
          {/* 底部：运维后端连接状态（心跳 10s） */}
          <Box sx={{ px: 2, py: 1.5, borderTop: `1px solid ${alpha("#fff", 0.08)}` }}>
            <Stack direction="row" spacing={1.25} alignItems="center">
              <Box
                sx={{
                  width: 8,
                  height: 8,
                  borderRadius: "50%",
                  flexShrink: 0,
                  bgcolor: backend === "ok" ? "#8ab6e3" : backend === "down" ? "#e06161" : alpha("#fff", 0.35),
                  boxShadow: backend === "ok" ? "0 0 6px rgba(138,182,227,.8)" : "none",
                  transition: "background-color .2s ease",
                }}
              />
              <Box sx={{ minWidth: 0 }}>
                <Typography variant="caption" sx={{ display: "block", color: alpha("#fff", 0.6), lineHeight: 1.3 }}>
                  {backend === "ok" ? "运维后端已连接" : backend === "down" ? "运维后端未连接" : "正在连接运维后端…"}
                </Typography>
                <Typography variant="caption" sx={{ display: "block", color: alpha("#fff", 0.35), fontFamily: "monospace" }}>
                  127.0.0.1:18721
                </Typography>
              </Box>
            </Stack>
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
            // 浅灰渐变背景
            background: "linear-gradient(135deg, #f5f7fa 0%, #e9ecef 100%)",
          }}
        >
          <Outlet />
        </Box>
      </Box>
    </Box>
  );
}
