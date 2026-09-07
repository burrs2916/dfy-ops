import { Box, Card, Chip, Stack, Typography } from "@mui/material";
import type { ServiceEnvReport, ServiceStatus } from "../types";

type Tone = "success" | "warning" | "error";

/**
 * 部署就绪度卡片。
 *
 * 就绪度 = 参与统计的服务中「运行且健康且环境满足」的占比。
 * 设计要点：
 *  - 总百分比 + 分段进度条：自研段（深）与第三方段（浅）分色，挂了哪类一眼可见；
 *  - 文案拆解 6/6 = 自研 4/4 + 第三方 2/2，不再给一个看不出构成的裸数字；
 *  - 桌面客户端（isDaemon=false）默认不占分母（GUI 按需打开，常年不在线属常态），
 *    可在「设置 → 通用」打开 `desktopInReadiness` 让它参与；
 *  - 无论是否计入，桌面客户端都有一块**常驻**区域显示在线明细——此前它挂在
 *    notReady 分支里，常驻服务全就绪时就整块消失，状态变成隐形。
 */
export default function ReadinessCard({
  services,
  envMap,
  desktopInReadiness = false,
}: {
  services: ServiceStatus[];
  envMap: Record<string, ServiceEnvReport>;
  /** 「设置 → 通用」开关：桌面客户端是否计入就绪度分母。 */
  desktopInReadiness?: boolean;
}) {
  // 分母口径：默认只收常驻服务；开关打开后桌面客户端（非 daemon）也进来。
  const managed = desktopInReadiness ? services : services.filter((s) => s.isDaemon);
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
  const pct = total === 0 ? 100 : Math.round((ready / total) * 100);
  const runningHealthy = managed.filter((s) => s.running && s.healthy).length;
  const envPass = managed.filter((s) => envMap[s.id]?.overall === "pass").length;

  const tone: Tone = pct === 100 ? "success" : pct === 0 ? "error" : "warning";

  const notReady = managed
    .filter((s) => !isReady(s))
    .map((s) => {
      const e = envMap[s.id];
      let reason = "";
      if (!s.running) reason = "未运行";
      else if (!s.healthy) reason = "端口不可达";
      else if (e && e.overall !== "pass")
        reason = e.overall === "fail" ? "环境缺失" : "环境警告";
      return { name: s.name, reason, third: s.thirdParty };
    });

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
          <Chip size="small" color="info" variant="outlined" label={`环境满足 ${envPass}/${total}`} />
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
        <Typography
          variant="h4"
          sx={{ fontWeight: 800, color: `${tone}.main`, lineHeight: 1, minWidth: 96, textAlign: "right" }}
        >
          {pct}%
        </Typography>
      </Stack>

      {/* 桌面客户端：独立常驻区块——不占分母也要让用户看见在线与否 */}
      {desktopCount > 0 && (
        <Box sx={{ mt: 2 }}>
          <Typography variant="caption" color="text.secondary">
            桌面客户端 {desktopRunning}/{desktopCount} 在线 ·{" "}
            {desktopInReadiness ? "已计入上方就绪度" : "不占分母，未运行不扣分（设置 → 通用可切换）"}
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

      {notReady.length > 0 && (
        <Box sx={{ mt: 2 }}>
          <Typography variant="caption" color="text.secondary">
            未就绪（{notReady.length}）：
          </Typography>
          <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap sx={{ mt: 0.5 }}>
            {notReady.map((n) => (
              <Chip
                key={n.name}
                size="small"
                color={n.reason === "环境缺失" || n.reason === "未运行" ? "error" : "warning"}
                label={`${n.name}${n.third ? "（第三方）" : ""} · ${n.reason}`}
              />
            ))}
          </Stack>
        </Box>
      )}
    </Card>
  );
}
