import { Chip } from "@mui/material";

export function StatusBadge({
  running,
  healthy,
}: {
  running: boolean;
  healthy: boolean;
}) {
  if (!running) {
    return <Chip size="small" label="未运行" color="default" variant="outlined" />;
  }
  if (healthy) {
    return <Chip size="small" label="运行中 / 健康" color="success" />;
  }
  return <Chip size="small" label="运行异常" color="error" />;
}
