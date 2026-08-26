import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  fullyParallel: false,
  use: {
    baseURL: "http://127.0.0.1:4173",
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
  },
  projects: [
    { name: "desktop", use: { ...devices["Desktop Chrome"] } },
    { name: "mobile", use: { ...devices["Desktop Chrome"], viewport: { width: 390, height: 844 }, isMobile: true } },
  ],
  webServer: {
    command: "PHARNESS_API_PROXY=http://127.0.0.1:4788 npm run dev -- --port 4173",
    url: "http://127.0.0.1:4173",
    reuseExistingServer: true,
  },
});
