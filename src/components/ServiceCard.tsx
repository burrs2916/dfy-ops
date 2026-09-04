import { useState } from "react";
import {
  Card,
  CardActionArea,
  CardContent,
  Chip,
  CircularProgress,
  IconButton,
  Stack,
  Tooltip,
  Typography,
} from "@mui/material";
import { PlayArrow, Replay, Stop } from "@mui/icons-material";
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
  const refresh = useOpsStore((st) => st.refresh);
  const [busy, setBusy] = useState(false);
  const [errMsg, setErrMsg] = useState<string | null>(null);

  // 卡片级快捷操作：执行期间禁点，完成后刷新全局状态，失败就地显示错误
  const act = async (label: string, fn: () => Promise<unknown>) => {
    setBusy(true);
    setErrMsg(null);
    try {
      await fn();
      await refresh();
    } catch (e) {
      setErrMsg(`${label}失败：${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card variant="outlined" sx={{ height: "100%" }}>
      <CardActionArea onClick={() => navigate(`/service/${s.id}`)} sx={{ height: "100%" }}>
        <CardContent>
          <Stack direction="row" justifyContent="space-between" alignItems="center">
            <Typography variant="subtitle1" sx={{ fontWeight: 600 }}>
              {s.name}
            </Typography>
            <Chip size="small" label={KIND_LABEL[s.kind] ?? s.kind} />
          </Stack>
          <Typography variant="body2" color="text.secondary" sx={{ mt: 1 }}>
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
              <StatusBadge running={s.running} healthy={s.healthy} />
              {envChip(env)}
              {s.ports.length > 0 && (
                <Typography variant="caption" color="text.secondary">
                  端口 {s.ports.join(", ")}
                </Typography>
              )}
            </Stack>
            {/* 快捷操作：阻止冒泡，避免误触卡片跳转 */}
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
            </Stack>
          </Stack>
          {errMsg && (
            <Typography variant="caption" color="error" sx={{ display: "block", mt: 0.5 }}>
              {errMsg}
            </Typography>
          )}
        </CardContent>
      </CardActionArea>
    </Card>
  );
}
