import { Box, Button, Card, Chip, CircularProgress, Stack, Typography } from "@mui/material";
import {
  Build,
  CheckCircle,
  Dns,
  PlayArrow,
  RocketLaunchOutlined,
  SettingsSuggest,
} from "@mui/icons-material";
import { useNavigate } from "react-router-dom";
import type { ServiceEnvReport, ServiceStatus } from "../types";

type Tone = "success" | "warning" | "error";

/** 待办行动项：一条「环境缺失/待配置/未运行」问题 + 直达解决入口 */
interface ActionItem {
  key: string;
  severity: "error" | "warning";
  icon: React.ReactNode;
  /** 问题一句话：点名服务与缺失内容 */
  text: string;
  /** 行动按钮文案 */
  action: string;
  onAct: () => void;
}

/**
 * 部署就绪度卡片。
 *
 * 就绪度 = 全部服务（含桌面客户端）中「运行且健康且环境满足」的占比。
 * 设计要点：
 *  - 桌面客户端（isDaemon=false，如 Tauri GUI）直接计入分母——它就是被运维的
 *    服务之一，未运行就拉低分数，与其他服务平等处理；
 *  - 总百分比 + 分段进度条：自研段（深）与第三方段（浅）分色，挂了哪类一眼可见；
 *  - 「待办行动」区块：未就绪不再只是标签——每条直接点名缺什么（服务 × 原因 ×
 *    具体缺失项），并给出一键解决入口（配置地址 / 启动 / 去修复），用户不用自己
 *    推断下一步；
 *  - 桌面客户端有一块**常驻**区域，用彩色圆点显示每个客户端的在线/未运行。
 */
export default function ReadinessCard({
  services,
  envMap,
  onOpenRemoteConfig,
  onStartService,
}: {
  services: ServiceStatus[];
  envMap: Record<string, ServiceEnvReport>;
  /** 待办「配置远程地址」：打开远程服务配置面板 */
  onOpenRemoteConfig: () => void;
  /** 待办「立即启动」：直接拉起受管服务 */
  onStartService?: (id: string) => void;
}) {
  const navigate = useNavigate();
  // 全部服务计入分母（桌面客户端平等参与）。
  const managed = services;
  const own = managed.filter((s) => !s.thirdParty);
  const third = managed.filter((s) => s.thirdParty);
  const desktops = services.filter((s) => !s.isDaemon);
  const desktopCount = desktops.length;
  const desktopRunning = desktops.filter((s) => s.running).length;
  const total = managed.length;

  const isReady = (s: ServiceStatus) => {
    const e = envMap[s.id];
    return s.running && s.healthy && e && e.overall === "pass";
  };
  const ownReady = own.filter(isReady).length;
  const thirdReady = third.filter(isReady).length;
  const ready = ownReady + thirdReady;

  // 首帧守卫：后端数据未到达（首轮渲染 / 后端未就绪）时 services 为空。
  // 此前 total=0 会被折算成 100%（0/0 → 100%），首次启动闪现「100% 健康」假象。
  if (total === 0) {
    return (
      <Card variant="outlined" sx={{ p: 2.5 }}>
        <Stack direction="row" spacing={1.5} alignItems="center">
          <CircularProgress size={16} />
          <Box>
            <Typography variant="h6" sx={{ fontWeight: 700 }}>
              部署就绪度
            </Typography>
            <Typography variant="caption" color="text.secondary">
              正在等待服务数据…
            </Typography>
          </Box>
        </Stack>
      </Card>
    );
  }

  const pct = Math.round((ready / total) * 100);
  const runningHealthy = managed.filter((s) => s.running && s.healthy).length;
  const envPass = managed.filter((s) => envMap[s.id]?.overall === "pass").length;
  // 环境报告未返回时不得报「0/9」——那是「还没检测」，不是「全不满足」。
  const envLoaded = Object.keys(envMap).length > 0;

  const tone: Tone = pct === 100 ? "success" : pct === 0 ? "error" : "warning";

  // ===== 待办行动清单：按严重度排序，每条带直达解决入口 =====
  const actions: ActionItem[] = [];
  for (const s of managed) {
    // 1. 待配置地址（先设置、后检测）：第三方服务未配置远程地址
    if (s.runState === "needs-config") {
      actions.push({
        key: `needs-config-${s.id}`,
        severity: "warning",
        icon: <Dns fontSize="small" />,
        text: `${s.name}：部署在虚拟机/远程主机，尚未配置远程地址，无法监测`,
        action: "去配置",
        onAct: onOpenRemoteConfig,
      });
      continue;
    }
    // 2. 未安装：明确判定缺失
    if (!s.installed && !s.thirdParty) {
      actions.push({
        key: `missing-${s.id}`,
        severity: "error",
        icon: <RocketLaunchOutlined fontSize="small" />,
        text: `${s.name}：尚未安装`,
        action: "去安装",
        onAct: () => navigate(`/service/${s.id}`),
      });
      continue;
    }
    // 3. 未运行（受管服务可直接拉起；第三方不可托管只能提示）
    if (!s.running && s.installed && !s.thirdParty) {
      actions.push({
        key: `stopped-${s.id}`,
        severity: "error",
        icon: <PlayArrow fontSize="small" />,
        text: `${s.name}：未运行`,
        action: "立即启动",
        onAct: () => onStartService?.(s.id),
      });
      continue;
    }
    // 4. 运行但端口不可达
    if (s.running && !s.healthy) {
      actions.push({
        key: `unreach-${s.id}`,
        severity: "error",
        icon: <Build fontSize="small" />,
        text: `${s.name}：进程在但端口不可达`,
        action: "查看详情",
        onAct: () => navigate(`/service/${s.id}`),
      });
      continue;
    }
    // 5. 环境检查不满足：点名具体的缺失项（缺什么一眼可见）
    const env = envMap[s.id];
    if (env && env.overall !== "pass") {
      const bad = env.items.filter((i) => i.status !== "pass").slice(0, 2);
      const detail = bad
        .map((i) => `${i.name}${i.status === "fail" ? "（缺失）" : "（警告）"}`)
        .join("、");
      actions.push({
        key: `env-${s.id}`,
        severity: env.overall === "fail" ? "error" : "warning",
        icon: <SettingsSuggest fontSize="small" />,
        text: `${s.name}：${detail}${env.items.filter((i) => i.status !== "pass").length > 2 ? " 等" : ""}`,
        action: "去修复",
        // 带 service 参数：修复中心直接定位滚动到该服务的报告分区
        onAct: () => navigate(`/env?service=${s.id}`),
      });
    }
  }
  const errorCount = actions.filter((a) => a.severity === "error").length;

  // 分段进度条：按就绪的自研/第三方占比分色，未就绪部分留底色。
  const seg = (n: number) => `${(n / Math.max(total, 1)) * 100}%`;

  return (
    <Card variant="outlined" sx={{ p: 2.5 }}>
      {/* 第一行：左侧标题+构成拆解，右上角统计 Chip */}
      <Stack
        direction="row"
        justifyContent="space-between"
        alignItems="flex-start"
        flexWrap="wrap"
        useFlexGap
      >
        <Box sx={{ minWidth: 150 }}>
          <Typography variant="h6" sx={{ fontWeight: 700 }}>
            部署就绪度
          </Typography>
          <Typography variant="caption" color="text.secondary">
            {ready}/{total} 服务完全就绪
            {third.length > 0 && (
              <>
                {" "}
                · 自研 {ownReady}/{own.length} · 第三方 {thirdReady}/{third.length}
              </>
            )}
          </Typography>
        </Box>
        <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
          <Chip size="small" color="success" variant="outlined" label={`运行健康 ${runningHealthy}/${total}`} />
          <Chip
            size="small"
            color="info"
            variant="outlined"
            label={envLoaded ? `环境满足 ${envPass}/${total}` : "环境检测中…"}
          />
          {actions.length > 0 && (
            <Chip
              size="small"
              color={errorCount > 0 ? "error" : "warning"}
              label={errorCount > 0 ? `待处理 ${actions.length}（${errorCount} 严重）` : `待处理 ${actions.length}`}
            />
          )}
        </Stack>
      </Stack>

      {/* 第二行：进度条拉满整行，百分数在右端 */}
      <Stack direction="row" spacing={2} alignItems="center" sx={{ mt: 1.5 }}>
        <Box
          sx={{
            flex: 1,
            display: "flex",
            height: 10,
            borderRadius: 5,
            overflow: "hidden",
            bgcolor: "action.hover",
          }}
        >
          {own.length > 0 && (
            <Box
              sx={{
                width: seg(ownReady),
                bgcolor: `${tone}.main`,
                transition: "width 0.3s",
              }}
            />
          )}
          {third.length > 0 && (
            <Box
              sx={{
                width: seg(thirdReady),
                bgcolor: `${tone}.light`,
                transition: "width 0.3s",
              }}
            />
          )}
        </Box>
        <Typography variant="h4" sx={{ fontWeight: 800, color: `${tone}.main`, minWidth: 96, textAlign: "right", fontVariantNumeric: "tabular-nums" }}>
          {pct}%
        </Typography>
      </Stack>

      {/* 桌面客户端：常驻区域，彩色圆点显示每个客户端在线/未运行 */}
      {desktopCount > 0 && (
        <Box sx={{ mt: 2 }}>
          <Typography variant="caption" color="text.secondary">
            桌面客户端 {desktopRunning}/{desktopCount} 在线
          </Typography>
          <Stack direction="row" spacing={2} flexWrap="wrap" useFlexGap sx={{ mt: 0.5 }}>
            {desktops.map((s) => (
              <Stack key={s.id} direction="row" spacing={0.75} alignItems="center">
                <Box
                  sx={{
                    width: 8,
                    height: 8,
                    borderRadius: "50%",
                    bgcolor: s.running ? "success.main" : "grey.400",
                    flexShrink: 0,
                  }}
                />
                <Typography variant="caption">{s.name}</Typography>
                <Typography variant="caption" color="text.secondary">
                  {s.running ? "在线" : "未运行"}
                </Typography>
              </Stack>
            ))}
          </Stack>
        </Box>
      )}

      {/* 待办行动：每条问题点名缺失内容 + 一键解决；全部就绪时给明确绿灯 */}
      {actions.length > 0 ? (
        <Box sx={{ mt: 2 }}>
          <Typography variant="caption" color="text.secondary">
            待处理（{actions.length}）——按严重度排序：
          </Typography>
          <Stack spacing={0.75} sx={{ mt: 0.75 }}>
            {actions.map((a) => (
              <Stack
                key={a.key}
                direction="row"
                spacing={1}
                alignItems="center"
                sx={{
                  px: 1.25,
                  py: 0.75,
                  borderRadius: 1.5,
                  bgcolor: a.severity === "error" ? "rgba(163,45,45,0.05)" : "rgba(186,117,23,0.05)",
                  border: 1,
                  borderColor: a.severity === "error" ? "rgba(163,45,45,0.18)" : "rgba(186,117,23,0.22)",
                }}
              >
                <Box sx={{ color: a.severity === "error" ? "error.main" : "warning.main", display: "flex", flexShrink: 0 }}>
                  {a.icon}
                </Box>
                <Typography variant="caption" sx={{ flex: 1, minWidth: 0, lineHeight: 1.45 }}>
                  {a.text}
                </Typography>
                <Button size="small" variant="outlined" onClick={a.onAct} sx={{ flexShrink: 0, whiteSpace: "nowrap" }}>
                  {a.action}
                </Button>
              </Stack>
            ))}
          </Stack>
        </Box>
      ) : (
        <Stack direction="row" spacing={1} alignItems="center" sx={{ mt: 2 }}>
          <CheckCircle color="success" fontSize="small" />
          <Typography variant="body2" sx={{ fontWeight: 600 }}>
            一切就绪：所有服务运行正常，环境全部满足
          </Typography>
        </Stack>
      )}
    </Card>
  );
}
