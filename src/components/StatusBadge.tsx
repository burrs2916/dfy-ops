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
    // 先设置、后检测：第三方服务未配置远程地址时不算「未运行」，
    // 用琥珀色引导用户先完成设置，避免误以为服务挂了
    if (runState === "needs-config") {
      return <Chip size="small" label="待配置地址" color="warning" variant="outlined" />;
    }
    return <Chip size="small" label="未运行" color="default" variant="outlined" />;
  }
  if (healthy) {
    const label = runState === "detected" ? "外部运行 / 健康" : "运行中 / 健康";
    return <Chip size="small" label={label} color="success" />;
  }
  return <Chip size="small" label="运行异常" color="error" />;
}
