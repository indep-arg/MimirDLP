import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// The interface lives in ui/; Tauri serves what this builds into dist/.
export default defineConfig({
  root: "ui",
  plugins: [vue()],
  // Tauri's own output must stay readable in the terminal.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/crates/**", "**/target/**"] },
  },
  build: {
    outDir: "../dist",
    emptyOutDir: true,
    // The webviews Tauri runs on: WebView2 on Windows, WebKitGTK on Linux.
    target: ["chrome111", "safari16"],
  },
});
