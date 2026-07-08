import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Dev-server proxy for the machine-facing API. Set PHARNESS_API_PROXY to
// target a non-default API (for example a port-forwarded cluster instance)
// and PHARNESS_API_PROXY_TOKEN to attach the operator bearer the same way
// the deployed console's nginx proxy does.
const proxyTarget = process.env.PHARNESS_API_PROXY ?? "http://127.0.0.1:4777";
const proxyToken = process.env.PHARNESS_API_PROXY_TOKEN;
const proxyEntry = {
  target: proxyTarget,
  changeOrigin: true,
  ...(proxyToken
    ? { headers: { Authorization: `Bearer ${proxyToken}` } }
    : {}),
};

export default defineConfig({
  optimizeDeps: {
    include: ["react", "react-dom/client"],
  },
  server: {
    proxy: {
      "/api": proxyEntry,
      "/health": proxyEntry,
    },
    warmup: {
      clientFiles: ["./src/main.jsx"],
    },
  },
  plugins: [react()],
});
