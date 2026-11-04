import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// 派单要求：vite dev 嘅 watch 排除 src-tauri（避免 Rust 编译产物触发前端热重载死循环）
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});
