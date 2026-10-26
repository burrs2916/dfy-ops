import { Stack, Typography } from "@mui/material";

/** 顶部统计带的一个数字格。 */
export default function Stat({ value, label, color }: { value: number; label: string; color?: string }) {
  return (
    <Stack alignItems="center" sx={{ px: 3, py: 0.5 }}>
      <Typography variant="h5" sx={{ fontWeight: 700, color: color ?? "text.primary", lineHeight: 1.2 }}>
        {value}
      </Typography>
      <Typography variant="caption" color="text.secondary">
        {label}
      </Typography>
    </Stack>
  );
}
