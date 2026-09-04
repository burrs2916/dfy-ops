import { Box, Stack, Typography } from "@mui/material";
import type { ReactNode } from "react";

/** 标准设置行：左 label+描述，右控件。web-craft SettingsPage 同款风格。 */
export function SettingRow({
  label,
  description,
  children,
}: {
  label: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <Stack
      direction="row"
      alignItems="flex-start"
      justifyContent="space-between"
      sx={{
        py: 2,
        px: 2.5,
        gap: 3,
        transition: "background-color 0.15s",
        "&:hover": { backgroundColor: "action.hover" },
      }}
    >
      <Box sx={{ flex: 1, minWidth: 0 }}>
        <Typography variant="body2" sx={{ fontWeight: 500 }}>
          {label}
        </Typography>
        {description && (
          <Typography variant="caption" color="text.secondary" sx={{ mt: 0.25, display: "block" }}>
            {description}
          </Typography>
        )}
      </Box>
      <Box sx={{ flexShrink: 0, display: "flex", alignItems: "center", minHeight: 36 }}>{children}</Box>
    </Stack>
  );
}
