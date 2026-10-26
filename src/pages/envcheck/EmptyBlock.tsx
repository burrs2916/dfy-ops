import type React from "react";
import { Stack, Typography } from "@mui/material";

/** 空态占位：图标 + 标题 + 引导文案（精致感的关键细节）。 */
export default function EmptyBlock({ icon, title, hint }: { icon: React.ReactNode; title: string; hint?: string }) {
  return (
    <Stack alignItems="center" sx={{ py: 5, color: "text.disabled" }}>
      {icon}
      <Typography variant="body2" sx={{ mt: 1.5, fontWeight: 600, color: "text.secondary" }}>
        {title}
      </Typography>
      {hint && (
        <Typography variant="caption" sx={{ mt: 0.5, maxWidth: 460, textAlign: "center" }}>
          {hint}
        </Typography>
      )}
    </Stack>
  );
}
