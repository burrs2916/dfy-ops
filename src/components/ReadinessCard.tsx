import { Box, Card, Chip, Stack, Typography } from "@mui/material";
import type { ServiceEnvReport, ServiceStatus } from "../types";

type Tone = "success" | "warning" | "error";

/**
 * 部署就绪度卡片。
 *
 * 就绪度 = 常驻服务（自研 + 第三方依赖）中「运行且健康且环境满足」的占比（用户决策：并入总数）。
 * 设计要点：
 *  - 总百分比 + 分段进度条：自研段（深）与第三方段（浅）分色，挂了哪类一眼可见；
 *  - 文案拆解 6/6 = 自研 4/4 + 第三方 2/2，不再给一个看不出构成的裸数字；
 *  - 桌面客户端（isDaemon=false）不参与就绪度，仅在 >0 时展示；
 *  - 未就绪列表标注「第三方」来源，避免误以为自研服务出问题。
 */
export default function ReadinessCard({
  services,
  envMap,
}: {
  services: ServiceStatus[];
  envMap: Record<string, ServiceEnvReport>;
}) {
  const managed = services.filter((s) => s.isDaemon);
  const own = managed.filter((s) => !s.thirdParty);
  const third = managed.filter((s) => s.thirdParty);
  const desktopCount = services.length - managed.length;
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
      <Stack direction="row" spacing={3} alignItems="center" flexWrap="wrap" useFlexGap>
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
        <Box sx={{ flex: 1, minWidth: 200 }}>
          <Typography variant="h4" sx={{ fontWeight: 800, color: `${tone}.main`, lineHeight: 1 }}>
            {pct}%
          </Typography>
          <Box
            sx={{
              display: "flex",
              height: 8,
              borderRadius: 4,
              overflow: "hidden",
              bgcolor: "action.hover",
              mt: 0.75,
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
        </Box>
        <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
          <Chip size="small" color="success" variant="outlined" label={`运行健康 ${runningHealthy}/${total}`} />
          <Chip size="small" color="info" variant="outlined" label={`环境满足 ${envPass}/${total}`} />
          {desktopCount > 0 && (
            <Chip size="small" variant="outlined" label={`桌面客户端 ${desktopCount} · 不参与就绪度`} />
          )}
        </Stack>
      </Stack>
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
