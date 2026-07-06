import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Tauri loads the built assets from `dist` (tauri.conf.json frontendDist).
// The dev server port must match tauri.conf.json build.devUrl.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
});
