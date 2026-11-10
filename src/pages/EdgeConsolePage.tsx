import { useEffect, useState } from "react";
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

/** 边缘控制台：gateway-console（Angular）由 edge-conductor HTTP 服务
 *  在 /console/ 静态托管，本页 iframe 内嵌。端口取边缘计算 config.toml
 *  的 httpPort（配置中心可改，改完控制台地址自动跟随）。
 *  云端 API 基址通过 ?cloudApi= 注入——控制台登录要先查云端租户、
 *  业务页调云端 admin-api（走 token 认证）。地址直接用边缘计算配置的
 *  「上游 DFY 后端地址」（upstreamServerUrl）：daemon 上报云端用的
 *  就是它，配置中心改一处，上报与控制台同时跟随——与运维访问云端
 *  后端的 IP:48085 同源，不是 Nginx 机内地址。 */
export default function EdgeConsolePage() {
  const navigate = useNavigate();
  const [cfg, setCfg] = useState<ExternalConfigResponse | null>(null);
  const [loading, setLoading] = useState(true);
  // iframe key：刷新按钮递增强制重载（控制台后端刚启动完需要手动刷一下的场景）
  const [frameKey, setFrameKey] = useState(0);
  // 独立窗口打开失败的提示（成功无感知，失败给出原因）
  const [windowErr, setWindowErr] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    setLoading(true);
    api
      .getExternalConfig("edge-conductor")
      .then((c) => {
        if (alive) setCfg(c);
      })
      .catch(() => {
        if (alive) setCfg(null);
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [frameKey]);

  const edge = cfg?.config as Record<string, string> | undefined;
  // 云端 DFY 后端基址：边缘计算配置的上游地址（含 http:// 前缀）。
  // 拿不到就不传参数——控制台回退 environment 默认，纯边缘场景仍可用。
  const cloudApi = edge?.upstreamServerUrl?.trim() || "";
  const cloudQuery = cloudApi ? `?cloudApi=${encodeURIComponent(cloudApi)}` : "";
  // 控制台走 Nginx 80 通道：装边缘计算时 install 管线已在 nginx.conf 自动接入
  // 「/console/ → web-console 产物、/api/ → 8084 反代」。edge-conductor 的 8084
  // 只有 /api 与 /ws 路由、不托管静态页，8084/console/ 是 404（页面空白的根因）。
  const consoleUrl = `http://127.0.0.1/console/${cloudQuery}`;

  return (
    <Stack spacing={1} sx={{ height: "100%", minHeight: 0, p: 1.5, pt: 1 }}>
      {/* 顶部条：标题 + 控制台地址 + 独立窗口 + 刷新 */}
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
        <Tooltip title="在独立原生窗口中打开（有自己的任务栏图标，可并排使用）">
          <Button
            size="small"
            startIcon={<OpenInNewIcon />}
            onClick={async () => {
              const err = await api.openEdgeConsoleWindow();
              if (err) setWindowErr(err);
            }}
            sx={{ textTransform: "none", flexShrink: 0 }}
          >
            独立窗口
          </Button>
        </Tooltip>
        <Button
          size="small"
          startIcon={<RefreshIcon />}
          onClick={() => setFrameKey((k) => k + 1)}
          sx={{ textTransform: "none", flexShrink: 0 }}
        >
          刷新
        </Button>
      </Stack>

      {/* 独立窗口打开失败提示（成功则弹出独立窗口，无感知） */}
      {windowErr && (
        <Alert severity="warning" variant="outlined" onClose={() => setWindowErr(null)} sx={{ flexShrink: 0, py: 0.5 }}>
          独立窗口打开失败：{windowErr}
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
              控制台由边缘计算（edge-conductor）内置托管，前端产物已随平台打包。
              到修复中心一键安装/重装边缘计算即可自动部署控制台。
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
        <>
          <Paper
            variant="outlined"
            sx={{ flex: 1, minHeight: 0, overflow: "hidden", bgcolor: "#fff" }}
          >
            <Box
              key={frameKey}
              component="iframe"
              src={consoleUrl}
              title="边缘控制台"
              sx={{
                width: "100%",
                height: "100%",
                border: "none",
                display: "block",
              }}
            />
          </Paper>
          <Alert severity="info" variant="outlined" sx={{ flexShrink: 0, py: 0.5 }}>
            若页面空白：确认 Nginx 已安装且边缘计算已完成安装/重装（安装时自动把
            /console/ 与 /api/ 接入 Nginx）。控制台经 Nginx 80 通道访问，
            安装目录的 web-console/ 为控制台产物。
          </Alert>
        </>
      )}
    </Stack>
  );
}
