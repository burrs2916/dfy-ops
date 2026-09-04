import { createTheme } from "@mui/material/styles";

export const theme = createTheme({
  palette: {
    mode: "light",
    primary: { main: "#185FA5" },
    success: { main: "#3B6D11" },
    error: { main: "#A32D2D" },
    warning: { main: "#BA7517" },
  },
  typography: {
    fontFamily:
      '"Segoe UI", "PingFang SC", "Microsoft YaHei", system-ui, sans-serif',
  },
});
