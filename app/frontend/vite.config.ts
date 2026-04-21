import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  base: "./",
  plugins: [react()],
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      // During local dev, forward API calls to the FastAPI backend.
      "/tasks": {
        target: "http://localhost:8000",
        changeOrigin: true,
      },
      "/automations": {
        target: "http://localhost:8000",
        changeOrigin: true,
      },
      "/run/smart": {
        target: "http://localhost:8000",
        changeOrigin: true,
      },
      "/run": {
        target: "http://localhost:8000",
        changeOrigin: true,
      },
      "/logs": {
        target: "http://localhost:8000",
        changeOrigin: true,
      },
      "/health": {
        target: "http://localhost:8000",
        changeOrigin: true,
      },
      "/observer": {
        target: "http://localhost:8000",
        changeOrigin: true,
      },
      "/settings": {
        target: "http://localhost:8000",
        changeOrigin: true,
      },
    },
  },
});

