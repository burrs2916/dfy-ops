import type React from "react";
import { Storage, Coffee, Bolt, SettingsInputAntenna, Cloud, QueryStats } from "@mui/icons-material";

/** 软件图标：卡片视觉锚点（圆润底色块 + 白色图形，专业感） */
export const TARGET_ICON: Record<string, React.ReactNode> = {
  mysql: <Storage />,
  java: <Coffee />,
  redis: <Bolt />,
  vernemq: <SettingsInputAntenna />,
  minio: <Cloud />,
  greptime: <QueryStats />,
};
