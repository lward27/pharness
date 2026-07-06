import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  optimizeDeps: {
    include: ["react", "react-dom/client"],
  },
  server: {
    proxy: {
      "/api": {
        target: process.env.PHARNESS_API_PROXY ?? "http://127.0.0.1:4777",
        changeOrigin: true,
      },
      "/health": {
        target: process.env.PHARNESS_API_PROXY ?? "http://127.0.0.1:4777",
        changeOrigin: true,
      },
    },
    warmup: {
      clientFiles: ["./src/main.jsx"],
    },
  },
  plugins: [react()],
});
