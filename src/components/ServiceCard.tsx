import { useState } from "react";
import {
  Card,
  CardContent,
  Chip,
  CircularProgress,
  IconButton,
  Stack,
  Tooltip,
  Typography,
} from "@mui/material";
import { PlayArrow, Refresh, Replay, Stop } from "@mui/icons-material";
import { useNavigate } from "react-router-dom";
import { api } from "../api";
import { useOpsStore } from "../store";
import type { CheckStatus, ServiceEnvReport, ServiceStatus } from "../types";
import { StatusBadge } from "./StatusBadge";

const KIND_LABEL: Record<string, string> = {
  vertx: "Vert.x",
  vue_spa: "Vue3",
  angular_spa: "Angular",
  rust_bin: "Rust",
  tauri_app: "Tauri",
  mqtt_broker: "MQTT",
  object_storage: "S3存储",
};

function envChip(env?: ServiceEnvReport) {
  if (!env) return null;
  const missing = env.items.filter((it) => it.status !== "pass").length;
  const map: Record<CheckStatus, { label: string; color: "success" | "warning" | "error" }> = {
    pass: { label: "环境满足", color: "success" },
    warn: { label: `环境警告 ${missing}`, color: "warning" },
    fail: { label: `环境缺失 ${missing}`, color: "error" },
  };
  const m = map[env.overall];
  return <Chip size="small" label={m.label} color={m.color} variant="outlined" />;
}

export function ServiceCard({ s, env }: { s: ServiceStatus; env?: ServiceEnvReport }) {
  const navigate = useNavigate();
  const refreshServices = useOpsStore((st) => st.refreshServices);
  const [busy, setBusy] = useState(false);
  const [reloading, setReloading] = useState(false);
  const [errMsg, setErrMsg] = useState<string | null>(null);

  // 卡片级快捷操作：执行期间禁点，完成后只刷新本服务 + 其依赖
  //（启动会级联拉起 dependsOn，依赖的状态也可能变化），不做全量探测，
  // 失败就地显示错误
  const act = async (label: string, fn: () => Promise<unknown>) => {
    setBusy(true);
    setErrMsg(null);
    try {
      await fn();
      await refreshServices([s.id, ...s.dependsOn]);
    } catch (e) {
      setErrMsg(`${label}失败：${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  // 单服务状态刷新：只探测本服务（状态 + 环境），秒级返回
  const reload = async () => {
    setReloading(true);
    await refreshServices([s.id]);
    setReloading(false);
  };

  // 卡片整体可点击跳转详情。不能用 CardActionArea——它渲染成 <button>，
  // 卡片内的启停 IconButton 也是 <button>，会触发
  // validateDOMNesting: <button> cannot appear as a descendant of <button>。
  return (
    <Card
      variant="outlined"
      sx={{
        height: "100%",
        cursor: "pointer",
        transition: "transform .18s ease, box-shadow .18s ease, border-color .18s ease",
        "&:hover": {
          transform: "translateY(-2px)",
          boxShadow: "0 4px 12px rgba(13, 148, 136, 0.12)",
          borderColor: "rgba(13, 148, 136, 0.45)",
        },
      }}
      onClick={() => navigate(`/service/${s.id}`)}
    >
      <CardContent sx={{ height: "100%", "&:last-child": { pb: 2 } }}>
          <Stack direction="row" justifyContent="space-between" alignItems="center">
            <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
              {s.name}
            </Typography>
            <Chip size="small" label={KIND_LABEL[s.kind] ?? s.kind} />
          </Stack>
          <Typography
            variant="body2"
            color="text.secondary"
            sx={{
              mt: 1,
              display: "-webkit-box",
              WebkitLineClamp: 2,
              WebkitBoxOrient: "vertical",
              overflow: "hidden",
              minHeight: 40,
            }}
          >
            {s.description}
          </Typography>
          <Stack
            direction="row"
            justifyContent="space-between"
            alignItems="center"
            sx={{ mt: 2 }}
            flexWrap="wrap"
          >
            <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap" useFlexGap>
              <StatusBadge
                running={s.running}
                healthy={s.healthy}
                runState={s.runState}
                installed={s.installed}
              />
              {envChip(env)}
              {s.ports.length > 0 && (
                <Typography variant="caption" color="text.secondary">
                  端口 {s.ports.join(", ")}
                </Typography>
              )}
            </Stack>
            {/* 快捷操作：阻止冒泡，避免误触卡片跳转。
                第三方依赖（VerneMQ/MinIO）由系统服务/容器管理，平台只检测不托管 → 无启停按钮 */}
            {!s.thirdParty && (
              <Stack
                direction="row"
                spacing={0.25}
                alignItems="center"
                onClick={(e) => e.stopPropagation()}
                sx={{ flexShrink: 0 }}
              >
                {busy ? (
                  <CircularProgress size={16} sx={{ m: 0.5 }} />
                ) : !s.running ? (
                  <Tooltip title="启动">
                    <IconButton size="small" onClick={() => act("启动", () => api.start(s.id))}>
                      <PlayArrow fontSize="small" />
                    </IconButton>
                  </Tooltip>
                ) : (
                  <>
                    <Tooltip title="重启">
                      <IconButton size="small" onClick={() => act("重启", () => api.restart(s.id))}>
                        <Replay fontSize="small" />
                      </IconButton>
                    </Tooltip>
                    <Tooltip title="停止">
                      <IconButton size="small" onClick={() => act("停止", () => api.stop(s.id))}>
                        <Stop fontSize="small" />
                      </IconButton>
                    </Tooltip>
                  </>
                )}
                {/* 单服务刷新：只重探本服务，替代全量轮询 */}
                <Tooltip title="刷新此服务状态">
                  <IconButton size="small" onClick={reload} disabled={reloading || busy}>
                    {reloading ? (
                      <CircularProgress size={14} />
                    ) : (
                      <Refresh fontSize="small" />
                    )}
                  </IconButton>
                </Tooltip>
              </Stack>
            )}
          </Stack>
          {errMsg && (
            <Typography variant="caption" color="error" sx={{ display: "block", mt: 0.5 }}>
              {errMsg}
            </Typography>
          )}
        </CardContent>
    </Card>
  );
}
