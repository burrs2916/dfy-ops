import { Chip } from "@mui/material";
import type { RunState } from "../types";

export function StatusBadge({
  running,
  healthy,
  runState,
  installed = true,
}: {
  running: boolean;
  healthy: boolean;
  runState?: RunState;
  installed?: boolean;
}) {
  if (!running) {
    // 未声明 productNames 的服务 installed 恒为 true（检测不适用），
    // 只有明确判定「未安装」才显示该状态
    if (!installed) {
      return <Chip size="small" label="未安装" color="default" variant="outlined" />;
    }
    return <Chip size="small" label="未运行" color="default" variant="outlined" />;
  }
  if (healthy) {
    const label = runState === "detected" ? "外部运行 / 健康" : "运行中 / 健康";
    return <Chip size="small" label={label} color="success" />;
  }
  return <Chip size="small" label="运行异常" color="error" />;
}
