import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Tauri loads the built assets from `dist` (tauri.conf.json frontendDist).
// The dev server port must match tauri.conf.json build.devUrl.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  build: {
    rollupOptions: {
      // Two webview surfaces: the main window and the overlay pill.
      input: {
        main: "index.html",
        overlay: "overlay.html",
        onboarding: "onboarding.html",
      },
    },
  },
  server: {
    port: 1420,
    strictPort: true,
  },
});
