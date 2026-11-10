import { useEffect, useRef, useState } from "react";
import {
  Alert,
  Box,
  Button,
  CircularProgress,
  Paper,
  Stack,
  Tooltip,
  Typography,
} from "@mui/material";
import HubIcon from "@mui/icons-material/Hub";
import OpenInNewIcon from "@mui/icons-material/OpenInNew";
import RefreshIcon from "@mui/icons-material/Refresh";
import { useNavigate } from "react-router-dom";
import { api } from "../api";
import type { ExternalConfigResponse } from "../types";

/** 边缘控制台：gateway-console（Angular）打包产物由 Nginx 托管在 80 端口
 *  /console/（安装管线自动部署产物并接入 Nginx，与云端前端同一发布方式），
 *  本页不内嵌 iframe——直接用 Tauri 原生 WebView 窗口（Windows 上是
 *  WebView2/Chromium 内核）打开控制台首页，控制台自己的路由守卫
 *  （InitializationGuard/AuthGuard）负责初始化与登录流转。
 *  云端 API 基址经 ?cloudApi= 注入：取边缘计算配置的「上游 DFY 后端地址」
 *  （upstreamServerUrl），daemon 上报与控制台共用同一配置，配置中心改一处
 *  两边同时跟随。 */

export default function EdgeConsolePage() {
  const navigate = useNavigate();
  const [cfg, setCfg] = useState<ExternalConfigResponse | null>(null);
  const [loading, setLoading] = useState(true);
  // 自动开窗只做一次；手动「打开控制台」可重复（已开窗口 = 前置聚焦）
  const autoOpenedRef = useRef(false);
  const [opening, setOpening] = useState(false);
  const [windowErr, setWindowErr] = useState<string | null>(null);

  const refreshConfig = () => {
    setLoading(true);
    api
      .getExternalConfig("edge-conductor")
      .then((c) => setCfg(c))
      .catch(() => setCfg(null))
      .finally(() => setLoading(false));
  };

  useEffect(() => {
    refreshConfig();
  }, []);

  const edge = cfg?.config as Record<string, string> | undefined;
  const cloudApi = edge?.upstreamServerUrl?.trim() || "";
  const cloudQuery = cloudApi ? `?cloudApi=${encodeURIComponent(cloudApi)}` : "";
  const consoleUrl = `http://127.0.0.1/console/${cloudQuery}`;

  const openConsole = async () => {
    setOpening(true);
    const err = await api.openEdgeConsoleWindow();
    setOpening(false);
    // 成功无感知（窗口已存在则前置聚焦）；失败给出原因
    setWindowErr(err);
  };

  // 配置就绪且边缘计算已安装 → 进入本页自动弹出原生控制台窗口（每次进入一次）
  useEffect(() => {
    if (loading || !cfg?.exists || autoOpenedRef.current) return;
    autoOpenedRef.current = true;
    openConsole();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loading, cfg]);

  return (
    <Stack spacing={1} sx={{ height: "100%", minHeight: 0, p: 1.5, pt: 1 }}>
      {/* 顶部条：标题 + 控制台地址 + 打开窗口 + 刷新配置 */}
      <Stack direction="row" spacing={1.25} alignItems="center" sx={{ flexShrink: 0 }}>
        <Box
          sx={{
            width: 28,
            height: 28,
            borderRadius: 1.25,
            display: "grid",
            placeItems: "center",
            bgcolor: "primary.main",
            color: "primary.contrastText",
          }}
        >
          <HubIcon fontSize="small" />
        </Box>
        <Box sx={{ minWidth: 0 }}>
          <Typography variant="subtitle1" sx={{ fontWeight: 700, lineHeight: 1.2 }}>
            边缘控制台
          </Typography>
          <Typography variant="caption" color="text.secondary" sx={{ fontFamily: "monospace" }}>
            {consoleUrl}
          </Typography>
        </Box>
        <Box sx={{ flex: 1 }} />
        <Tooltip title="用 Tauri 原生 WebView 窗口打开控制台（独立任务栏图标，可并排使用；窗口已开时前置聚焦）">
          <Button
            size="small"
            startIcon={opening ? <CircularProgress size={14} /> : <OpenInNewIcon />}
            disabled={opening}
            onClick={openConsole}
            sx={{ textTransform: "none", flexShrink: 0 }}
          >
            打开控制台
          </Button>
        </Tooltip>
        <Tooltip title="重读边缘计算配置并再次自动开窗">
          <Button
            size="small"
            startIcon={<RefreshIcon />}
            onClick={() => {
              autoOpenedRef.current = false;
              refreshConfig();
            }}
            sx={{ textTransform: "none", flexShrink: 0 }}
          >
            刷新
          </Button>
        </Tooltip>
      </Stack>

      {/* 开窗失败原因 */}
      {windowErr && (
        <Alert severity="warning" variant="outlined" onClose={() => setWindowErr(null)} sx={{ flexShrink: 0, py: 0.5 }}>
          控制台窗口打开失败：{windowErr}
        </Alert>
      )}

      {/* 内容区 */}
      {loading ? (
        <Paper
          variant="outlined"
          sx={{ flex: 1, display: "grid", placeItems: "center", minHeight: 0 }}
        >
          <CircularProgress size={28} />
        </Paper>
      ) : !cfg?.exists ? (
        <Paper
          variant="outlined"
          sx={{ flex: 1, display: "grid", placeItems: "center", minHeight: 0 }}
        >
          <Stack spacing={1.5} alignItems="center" sx={{ maxWidth: 420, px: 3 }}>
            <HubIcon sx={{ fontSize: 44, color: "text.disabled" }} />
            <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
              边缘计算尚未安装
            </Typography>
            <Typography variant="body2" color="text.secondary" align="center">
              控制台打包产物随边缘计算安装自动部署到 Nginx 托管目录
              （与云端前端同一发布方式）。到修复中心一键安装/重装边缘计算即可。
            </Typography>
            <Button
              variant="contained"
              size="small"
              startIcon={<OpenInNewIcon />}
              onClick={() => navigate("/env?tab=repair")}
              sx={{ textTransform: "none" }}
            >
              去修复中心安装
            </Button>
          </Stack>
        </Paper>
      ) : (
        <Paper
          variant="outlined"
          sx={{
            flex: 1,
            minHeight: 0,
            display: "grid",
            placeItems: "center",
            bgcolor: "#fafafa",
          }}
        >
          <Stack spacing={2} alignItems="center" sx={{ maxWidth: 520, px: 3, py: 2 }}>
            <HubIcon sx={{ fontSize: 48, color: "primary.main" }} />
            <Typography variant="h6" sx={{ fontWeight: 700 }}>
              控制台已在原生窗口中打开
            </Typography>
            <Typography variant="body2" color="text.secondary" align="center">
              边缘控制台运行在独立原生窗口（Tauri WebView / Chromium 内核），
              不再内嵌 iframe。窗口未出现时点右上角「打开控制台」；
              已打开时点击会前置聚焦。
            </Typography>
            <Typography
              variant="caption"
              color="text.secondary"
              align="center"
              sx={{ fontFamily: "monospace", wordBreak: "break-all" }}
            >
              {consoleUrl}
            </Typography>
            <Stack direction="row" spacing={1}>
              <Button variant="contained" size="small" startIcon={<OpenInNewIcon />} onClick={openConsole}>
                打开控制台
              </Button>
              <Button
                variant="outlined"
                size="small"
                onClick={() => {
                  autoOpenedRef.current = false;
                  refreshConfig();
                }}
              >
                重载配置
              </Button>
            </Stack>
            <Alert severity="info" variant="outlined" sx={{ width: "100%", py: 0.5 }}>
              打不开时看 debug.log 的 console_diag 段：产物部署、Nginx 接入、
              进程、HTTP 探测逐项都有结论，无需人工排查。
            </Alert>
          </Stack>
        </Paper>
      )}
    </Stack>
  );
}
