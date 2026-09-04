import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed dev server port and does not clear the screen.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 18720,
    strictPort: true,
    host: true,
  },
  build: {
    target: "es2021",
    outDir: "dist",
    emptyOutDir: true,
  },
});
