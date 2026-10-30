import { useCallback, useEffect, useState } from "react";
import {
  Box,
  Button,
  Chip,
  IconButton,
  Paper,
  Stack,
  Tooltip,
  Typography,
} from "@mui/material";
import ChevronLeftIcon from "@mui/icons-material/ChevronLeft";
import ChevronRightIcon from "@mui/icons-material/ChevronRight";
import DeleteOutlineIcon from "@mui/icons-material/DeleteOutline";
import DeleteSweepIcon from "@mui/icons-material/DeleteSweep";
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

/** 每页条数：事件多了分页翻看，避免时间轴无限拉长把页面顶爆 */
const PAGE_SIZE = 10;

/** 运维启停事件时间轴：谁在何时对哪个组件做了什么、结果如何。
 *  `refreshKey` 变化时重新拉取（父级在启停/安装完成后递增）。
 *  `target` 传入时只展示该组件的事件（服务自己的页签内展示自己的历史）。
 *  事件可运维：分页浏览（10 条/页）、删单条、一键清空（二次确认防误删）。 */
export default function OpsEventTimeline({
  refreshKey,
  target,
}: {
  refreshKey: number;
  target?: string;
}) {
  const [events, setEvents] = useState<OpsEventItem[] | null>(null);
  const [page, setPage] = useState(0);
  /** 清空按钮二段确认：第一次点进入待确认态，再点才真清空 */
  const [clearArmed, setClearArmed] = useState(false);

  const load = useCallback(async () => {
    try {
      const r = await api.getOpsEvents(100);
      setEvents(r.events ?? []);
    } catch {
      setEvents([]);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load, refreshKey]);

  // 翻页越界校正（删除/清空后总页数变小）
  const scoped = target ? (events ?? []).filter((e) => e.target === target) : events ?? [];
  const pageCount = Math.max(1, Math.ceil(scoped.length / PAGE_SIZE));
  const safePage = Math.min(page, pageCount - 1);
  const shown = scoped.slice(safePage * PAGE_SIZE, safePage * PAGE_SIZE + PAGE_SIZE);

  const removeOne = async (id: string) => {
    try {
      await api.deleteOpsEvent(id);
      await load();
    } catch {
      /* 删除失败静默：下次刷新会重新拉取 */
    }
  };

  const clearAll = async () => {
    if (!clearArmed) {
      setClearArmed(true);
      return;
    }
    setClearArmed(false);
    try {
      await api.clearOpsEvents();
      setPage(0);
      await load();
    } catch {
      /* 清空失败静默 */
    }
  };

  return (
    <Paper variant="outlined" sx={{ px: 2, py: 1.5 }}>
      <Stack direction="row" alignItems="center" spacing={1}>
        <HistoryIcon fontSize="small" color="primary" />
        <Typography variant="subtitle2" sx={{ fontWeight: 700, flex: 1 }}>
          启停事件
          {scoped.length > 0 && (
            <Box component="span" sx={{ fontWeight: 400, color: "text.secondary", ml: 1 }}>
              共 {scoped.length} 条
            </Box>
          )}
        </Typography>
        <Button
          size="small"
          startIcon={<RefreshIcon />}
          onClick={() => void load()}
          sx={{ textTransform: "none", minWidth: 0 }}
        >
          刷新
        </Button>
        {scoped.length > 0 && (
          <Tooltip title={clearArmed ? "再点一次确认清空全部事件" : "清空全部事件"}>
            <Button
              size="small"
              color={clearArmed ? "error" : "inherit"}
              variant={clearArmed ? "contained" : "text"}
              startIcon={<DeleteSweepIcon />}
              onClick={() => void clearAll()}
              onBlur={() => setClearArmed(false)}
              sx={{ textTransform: "none", minWidth: 0 }}
            >
              {clearArmed ? "确认清空" : "清空"}
            </Button>
          </Tooltip>
        )}
      </Stack>
      <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 1 }}>
        手动启停与安装流程的自动动作全部留痕，便于安全审计；单条可删、支持分页
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
        <>
          <Stack spacing={0.75} sx={{ mt: 0.5 }}>
            {shown.map((e) => (
              <Stack
                key={e.id || `${e.ts}-${e.detail.slice(0, 12)}`}
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
                {e.id && (
                  <Tooltip title="删除这条事件">
                    <IconButton
                      size="small"
                      onClick={() => void removeOne(e.id)}
                      sx={{ p: 0.25, flexShrink: 0 }}
                    >
                      <DeleteOutlineIcon sx={{ fontSize: 15 }} />
                    </IconButton>
                  </Tooltip>
                )}
              </Stack>
            ))}
          </Stack>
          {pageCount > 1 && (
            <Stack direction="row" alignItems="center" spacing={0.5} sx={{ mt: 1 }}>
              <IconButton
                size="small"
                disabled={safePage === 0}
                onClick={() => setPage((p) => Math.max(0, p - 1))}
              >
                <ChevronLeftIcon fontSize="small" />
              </IconButton>
              <Typography variant="caption" color="text.secondary">
                第 {safePage + 1} / {pageCount} 页
              </Typography>
              <IconButton
                size="small"
                disabled={safePage >= pageCount - 1}
                onClick={() => setPage((p) => Math.min(pageCount - 1, p + 1))}
              >
                <ChevronRightIcon fontSize="small" />
              </IconButton>
            </Stack>
          )}
        </>
      )}
    </Paper>
  );
}
