import { useMemo } from "react";
import { Alert, Box, Chip, Paper, Stack, Typography } from "@mui/material";
import { useNavigate } from "react-router-dom";
import { useOpsStore } from "../store";
import type { ServiceStatus } from "../types";

const NODE_W = 172;
const NODE_H = 56;
const H_GAP = 48;
const V_GAP = 96;
const PAD = 32;

function statusTone(s: ServiceStatus): "ok" | "warn" | "down" {
  // 先设置、后检测：待配置态按「需要处理」的暖黄提示，不渲染成红色 down
  if (s.runState === "needs-config") return "warn";
  if (!s.running) return "down";
  if (!s.healthy) return "warn";
  return "ok";
}

// 状态配色与全应用语义一致：success = 品牌青（全项目禁绿色系），
// ok 用品牌青的浅底+主色描边，warn/down 沿用主题的暖黄/暗红。
const TONE_FILL: Record<string, string> = {
  ok: "#E6F4F2",
  warn: "#FCF1DC",
  down: "#F4EEEE",
};
const TONE_STROKE: Record<string, string> = {
  ok: "#1e3f66",
  warn: "#BA7517",
  down: "#A32D2D",
};

export default function Topology() {
  const { services, loading, error } = useOpsStore();
  const navigate = useNavigate();

  const layout = useMemo(() => {
    if (services.length === 0) return null;
    const byId = new Map(services.map((s) => [s.id, s]));
    const layer = new Map<string, number>();
    const depth = (id: string, guard = 0): number => {
      if (layer.has(id)) return layer.get(id)!;
      if (guard > services.length + 2) return 0;
      const s = byId.get(id);
      if (!s || s.dependsOn.length === 0) {
        layer.set(id, 0);
        return 0;
      }
      let max = 0;
      for (const d of s.dependsOn) max = Math.max(max, depth(d, guard + 1) + 1);
      layer.set(id, max);
      return max;
    };
    services.forEach((s) => depth(s.id));

    const layers: ServiceStatus[][] = [];
    services.forEach((s) => {
      const l = layer.get(s.id) ?? 0;
      (layers[l] ??= []).push(s);
    });

    let maxLayerW = 0;
    const positions = new Map<string, { x: number; y: number }>();
    layers.forEach((nodes, l) => {
      const layerW = nodes.length * (NODE_W + H_GAP);
      maxLayerW = Math.max(maxLayerW, layerW);
      const startX = PAD + (Math.max(maxLayerW, 640) - layerW) / 2;
      nodes.forEach((s, i) => {
        positions.set(s.id, {
          x: startX + i * (NODE_W + H_GAP),
          y: PAD + l * (NODE_H + V_GAP),
        });
      });
    });

    const width = Math.max(maxLayerW, 640) + PAD * 2;
    const height = PAD * 2 + layers.length * (NODE_H + V_GAP);
    return { positions, width, height };
  }, [services]);

  return (
    <Stack spacing={3}>
      <Box>
        <Typography variant="h5" sx={{ fontWeight: 700 }}>
          服务依赖拓扑
        </Typography>
        <Typography variant="body2" color="text.secondary">
          箭头表示依赖方向（被依赖者在上方）；编排按依赖顺序自上而下启动。点击节点查看详情。
        </Typography>
      </Box>

      {error && <Alert severity="error">{error}</Alert>}

      {loading && services.length === 0 ? (
        <Alert severity="info">加载中…</Alert>
      ) : !layout ? (
        <Alert severity="info">暂无服务数据。</Alert>
      ) : (
        <Paper variant="outlined" sx={{ p: 2, overflowX: "auto" }}>
          <svg
            viewBox={`0 0 ${layout.width} ${layout.height}`}
            style={{ width: "100%", minWidth: layout.width, height: layout.height }}
          >
            <defs>
              <marker
                id="arrow"
                markerWidth="10"
                markerHeight="10"
                refX="8"
                refY="3"
                orient="auto"
                markerUnits="strokeWidth"
              >
                <path d="M0,0 L8,3 L0,6 Z" fill="#7d8996" />
              </marker>
            </defs>

            {services.map((s) =>
              s.dependsOn.map((dep) => {
                const a = layout.positions.get(dep);
                const b = layout.positions.get(s.id);
                if (!a || !b) return null;
                const x1 = a.x + NODE_W / 2;
                const y1 = a.y + NODE_H;
                const x2 = b.x + NODE_W / 2;
                const y2 = b.y;
                const my = (y1 + y2) / 2;
                return (
                  <path
                    key={`${dep}-${s.id}`}
                    d={`M ${x1} ${y1} C ${x1} ${my}, ${x2} ${my}, ${x2} ${y2}`}
                    stroke="#9aa5b1"
                    strokeWidth={1.5}
                    fill="none"
                    markerEnd="url(#arrow)"
                  />
                );
              })
            )}

            {services.map((s) => {
              const p = layout.positions.get(s.id)!;
              const tone = statusTone(s);
              return (
                <g
                  key={s.id}
                  style={{ cursor: "pointer" }}
                  onClick={() => navigate(`/service/${s.id}`)}
                >
                  <rect
                    x={p.x}
                    y={p.y}
                    width={NODE_W}
                    height={NODE_H}
                    rx={8}
                    fill={TONE_FILL[tone]}
                    stroke={TONE_STROKE[tone]}
                    strokeWidth={2}
                  />
                  <text
                    x={p.x + NODE_W / 2}
                    y={p.y + 23}
                    textAnchor="middle"
                    fontSize={13}
                    fontWeight={600}
                    fill="#1e2530"
                  >
                    {s.name}
                  </text>
                  <text
                    x={p.x + NODE_W / 2}
                    y={p.y + 43}
                    textAnchor="middle"
                    fontSize={11}
                    fill="#5b6572"
                  >
                    {s.id} ·{" "}
                    {s.runState === "needs-config"
                      ? "待配置"
                      : !s.running
                        ? "未运行"
                        : !s.healthy
                          ? "异常"
                          : s.runState === "detected"
                            ? "外部运行"
                            : "健康"}
                  </text>
                </g>
              );
            })}
          </svg>
        </Paper>
      )}

      <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap alignItems="center">
        <Chip
          size="small"
          label="运行中·健康"
          sx={{ bgcolor: TONE_FILL.ok, border: `1px solid ${TONE_STROKE.ok}` }}
        />
        <Chip
          size="small"
          label="运行中·异常"
          sx={{ bgcolor: TONE_FILL.warn, border: `1px solid ${TONE_STROKE.warn}` }}
        />
        <Chip
          size="small"
          label="未运行"
          sx={{ bgcolor: TONE_FILL.down, border: `1px solid ${TONE_STROKE.down}` }}
        />
      </Stack>
    </Stack>
  );
}
