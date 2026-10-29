import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    setupFiles: ["src/vitest.setup.ts"],
    include: ["src/**/*.test.{ts,tsx}", "tests/**/*.test.{ts,tsx}"],
    exclude: ["node_modules/**", "dist/**", ".compare/**", "src-tauri/target/**"],
  },
});
