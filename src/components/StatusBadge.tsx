import { Chip } from "@mui/material";
import type { RunState } from "../types";

export function StatusBadge({
  running,
  healthy,
  runState,
}: {
  running: boolean;
  healthy: boolean;
  runState?: RunState;
}) {
  if (!running) {
    return <Chip size="small" label="未运行" color="default" variant="outlined" />;
  }
  if (healthy) {
    const label = runState === "detected" ? "外部运行 / 健康" : "运行中 / 健康";
    return <Chip size="small" label={label} color="success" />;
  }
  return <Chip size="small" label="运行异常" color="error" />;
}
