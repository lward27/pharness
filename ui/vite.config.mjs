import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";

// Dev-server proxy for the machine-facing API. Set PHARNESS_API_PROXY to
// target a non-default API (for example a port-forwarded cluster instance)
// and PHARNESS_API_PROXY_TOKEN to attach the operator bearer the same way
// the deployed console's nginx proxy does.
const proxyTarget = process.env.PHARNESS_API_PROXY ?? "http://127.0.0.1:4777";
const proxyToken = process.env.PHARNESS_API_PROXY_TOKEN;
const proxyEntry = {
  target: proxyTarget,
  changeOrigin: true,
  // The Campus terminal is a WebSocket under /api/campus/.
  ws: true,
  ...(proxyToken
    ? { headers: { Authorization: `Bearer ${proxyToken}` } }
    : {}),
};

export default defineConfig({
  build: {
    rollupOptions: {
      // The Campus terminal is a separate page so it shares no code path
      // or credentials with the operator console.
      input: {
        main: fileURLToPath(new URL("./index.html", import.meta.url)),
        campus: fileURLToPath(new URL("./campus.html", import.meta.url)),
      },
    },
  },
  optimizeDeps: {
    include: ["react", "react-dom/client"],
  },
  server: {
    proxy: {
      "/api": proxyEntry,
      "/health": proxyEntry,
    },
    warmup: {
      clientFiles: ["./src/main.tsx"],
    },
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.{test,spec}.{js,jsx,ts,tsx}"],
    setupFiles: ["./src/test/setup.ts"],
  },
  plugins: [react()],
});
