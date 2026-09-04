import { Box, Card, Chip, LinearProgress, Stack, Typography } from "@mui/material";
import type { ServiceEnvReport, ServiceStatus } from "../types";

type Tone = "success" | "warning" | "error";

export default function ReadinessCard({
  services,
  envMap,
}: {
  services: ServiceStatus[];
  envMap: Record<string, ServiceEnvReport>;
}) {
  // 就绪度只统计常驻服务（isDaemon）；Tauri 桌面客户端等非常驻应用
  // 本就不该 7×24 运行，计入"未运行"会拉低百分比、语义错误。
  const managed = services.filter((s) => s.isDaemon);
  const desktopCount = services.length - managed.length;
  const total = managed.length;
  const ready = managed.filter((s) => {
    const e = envMap[s.id];
    return s.running && s.healthy && e && e.overall === "pass";
  }).length;
  const runningHealthy = managed.filter((s) => s.running && s.healthy).length;
  const envPass = managed.filter((s) => envMap[s.id]?.overall === "pass").length;
  const pct = total === 0 ? 100 : Math.round((ready / total) * 100);

  const tone: Tone = pct === 100 ? "success" : pct === 0 ? "error" : "warning";

  const notReady = managed
    .filter((s) => {
      const e = envMap[s.id];
      return !(s.running && s.healthy && e && e.overall === "pass");
    })
    .map((s) => {
      const e = envMap[s.id];
      let reason = "";
      if (!s.running) reason = "未运行";
      else if (!s.healthy) reason = "端口不可达";
      else if (e && e.overall !== "pass")
        reason = e.overall === "fail" ? "环境缺失" : "环境警告";
      return { name: s.name, reason };
    });

  return (
    <Card variant="outlined" sx={{ p: 2.5 }}>
      <Stack direction="row" spacing={3} alignItems="center" flexWrap="wrap" useFlexGap>
        <Box sx={{ minWidth: 120 }}>
          <Typography variant="h6" sx={{ fontWeight: 700 }}>
            部署就绪度
          </Typography>
          <Typography variant="caption" color="text.secondary">
            {ready}/{total} 常驻服务完全就绪
          </Typography>
        </Box>
        <Box sx={{ flex: 1, minWidth: 200 }}>
          <Typography variant="h4" sx={{ fontWeight: 800, color: `${tone}.main`, lineHeight: 1 }}>
            {pct}%
          </Typography>
          <LinearProgress
            variant="determinate"
            value={pct}
            color={tone}
            sx={{ mt: 0.75, height: 8, borderRadius: 4 }}
          />
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
                label={`${n.name} · ${n.reason}`}
              />
            ))}
          </Stack>
        </Box>
      )}
    </Card>
  );
}
