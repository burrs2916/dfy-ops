import { Box } from "@mui/material";
import type { CheckStatus } from "../../types";
import { STATUS_HEX } from "./shared";

/** 状态小圆点（检测结果列表每行前的视觉锚点）。 */
export default function StatusDot({ status }: { status: CheckStatus }) {
  return (
    <Box
      sx={{
        width: 10,
        height: 10,
        borderRadius: "50%",
        bgcolor: STATUS_HEX[status],
        flexShrink: 0,
      }}
    />
  );
}
