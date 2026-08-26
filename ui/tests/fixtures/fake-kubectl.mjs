#!/usr/bin/env node

// The real API builds and dispatches the exact Kubernetes Job manifests in
// the server-backed browser journey. This adapter acknowledges creation; the
// test then reports deterministic worker/provider outcomes through the real
// authenticated internal API routes.
const args = process.argv.slice(2);
if (args[0] === "get" && args[1] === "jobs") {
  process.stdout.write(JSON.stringify({ items: [] }));
  process.exit(0);
}
if (args[0] === "get" && args[1] === "job") {
  process.stdout.write(JSON.stringify({ status: { succeeded: 1 } }));
  process.exit(0);
}
process.stdin.resume();
process.stdin.on("data", () => {});
process.stdin.on("end", () => process.exit(0));
setTimeout(() => process.exit(0), 1000).unref();
