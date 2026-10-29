import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  base: "./",
  plugins: [react()],
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
    // Native builds and isolated browser profiles contain locked/generated files.
    watch: { ignored: ["**/artifacts/**", "**/src-tauri/target/**"] },
  },
  envPrefix: ["VITE_", "TAURI_"],
});
