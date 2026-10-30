import { useCallback, useEffect, useState } from "react";
import {
  Box,
  Button,
  Chip,
  Paper,
  Stack,
  Typography,
} from "@mui/material";
import HistoryIcon from "@mui/icons-material/History";
import RefreshIcon from "@mui/icons-material/Refresh";
import { api } from "../api";
import type { OpsEventItem } from "../types";
import { MONO_STACK } from "../theme";
import { TARGET_LABEL } from "../pages/envcheck/shared";

/** 动作中文名 */
const ACTION_LABEL: Record<string, string> = {
  start: "启动",
  stop: "停止",
  install: "安装",
  uninstall: "卸载",
  autostart: "自动启动",
};

/** 发起者中文名 */
const ACTOR_LABEL: Record<string, string> = {
  user: "手动操作",
  installer: "安装流程",
  watchdog: "看门狗",
};

/** 运维启停事件时间轴：谁在何时对哪个组件做了什么、结果如何。
 *  `refreshKey` 变化时重新拉取（父级在启停/安装完成后递增）。
 *  `target` 传入时只展示该组件的事件（服务自己的页签内展示自己的历史）。 */
export default function OpsEventTimeline({
  refreshKey,
  target,
}: {
  refreshKey: number;
  target?: string;
}) {
  const [events, setEvents] = useState<OpsEventItem[] | null>(null);
  const [expanded, setExpanded] = useState(false);

  const load = useCallback(async () => {
    try {
      const r = await api.getOpsEvents(50);
      setEvents(r.events ?? []);
    } catch {
      setEvents([]);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load, refreshKey]);

  const scoped = target ? (events ?? []).filter((e) => e.target === target) : events ?? [];
  const shown = expanded ? scoped.slice(0, 20) : scoped.slice(0, 6);

  return (
    <Paper variant="outlined" sx={{ px: 2, py: 1.5 }}>
      <Stack direction="row" alignItems="center" spacing={1}>
        <HistoryIcon fontSize="small" color="primary" />
        <Typography variant="subtitle2" sx={{ fontWeight: 700, flex: 1 }}>
          启停事件
        </Typography>
        <Button
          size="small"
          startIcon={<RefreshIcon />}
          onClick={() => void load()}
          sx={{ textTransform: "none", minWidth: 0 }}
        >
          刷新
        </Button>
      </Stack>
      <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 1 }}>
        手动启停与安装流程的自动动作全部留痕，便于安全审计
      </Typography>
      {events === null ? (
        <Typography variant="body2" color="text.secondary">
          读取事件…
        </Typography>
      ) : shown.length === 0 ? (
        <Typography variant="body2" color="text.secondary">
          暂无事件——安装组件或点击启停后会在这里记录
        </Typography>
      ) : (
        <Stack spacing={0.75} sx={{ mt: 0.5 }}>
          {shown.map((e, i) => (
            <Stack
              key={`${e.ts}-${i}`}
              direction="row"
              spacing={1}
              alignItems="baseline"
              sx={{ minWidth: 0, flexWrap: "wrap", rowGap: 0.25 }}
            >
              <Typography
                variant="caption"
                sx={{ fontFamily: MONO_STACK, color: "text.secondary", flexShrink: 0 }}
              >
                {e.ts}
              </Typography>
              <Chip
                size="small"
                color={e.ok ? "success" : "error"}
                variant="outlined"
                sx={{ height: 20 }}
                label={`${TARGET_LABEL[e.target] ?? e.target} ${
                  ACTION_LABEL[e.action] ?? e.action
                }${e.ok ? "成功" : "失败"}`}
              />
              <Typography variant="caption" sx={{ color: "text.secondary", flexShrink: 0 }}>
                [{ACTOR_LABEL[e.actor] ?? e.actor}]
              </Typography>
              <Typography
                variant="body2"
                sx={{ wordBreak: "break-all", minWidth: 0, flex: 1 }}
                title={e.detail}
              >
                {e.detail.length > 90 ? `${e.detail.slice(0, 90)}…` : e.detail}
              </Typography>
            </Stack>
          ))}
          {scoped.length > 6 && (
            <Box>
              <Button size="small" onClick={() => setExpanded((v) => !v)} sx={{ textTransform: "none" }}>
                {expanded ? "收起" : `展开更多（共 ${scoped.length} 条）`}
              </Button>
            </Box>
          )}
        </Stack>
      )}
    </Paper>
  );
}
