import type { ReactNode } from "react";
import { Box, Stack, Typography } from "@mui/material";

/**
 * 全站统一页头：标题 + 副文案居左，操作区居右。
 *
 * 精致度的关键在「一致性」——此前各页页头各自手写（h5/h6 混用、
 * 副文案间距不一、操作按钮对齐方式各异），页面切换时观感跳动。
 * 所有顶级页面统一走本组件，字号/间距/对齐由这里唯一负责。
 */
export function PageHeader({
  title,
  description,
  actions,
  icon,
}: {
  title: string;
  description?: ReactNode;
  /** 右侧操作区（按钮组/过滤控件），垂直居中对齐标题 */
  actions?: ReactNode;
  icon?: ReactNode;
}) {
  return (
    <Stack
      direction={{ xs: "column", sm: "row" }}
      justifyContent="space-between"
      alignItems={{ xs: "flex-start", sm: "center" }}
      spacing={1.25}
      sx={{ mb: 2.5 }}
    >
      <Box sx={{ minWidth: 0 }}>
        <Stack direction="row" spacing={1} alignItems="center">
          {icon}
          <Typography variant="h5" sx={{ fontWeight: 700, lineHeight: 1.25, letterSpacing: 0.2 }}>
            {title}
          </Typography>
        </Stack>
        {description && (
          <Typography variant="body2" color="text.secondary" sx={{ mt: 0.5, maxWidth: 720 }}>
            {description}
          </Typography>
        )}
      </Box>
      {actions && (
        <Stack direction="row" spacing={1.25} alignItems="center" sx={{ flexShrink: 0 }}>
          {actions}
        </Stack>
      )}
    </Stack>
  );
}

export default PageHeader;
