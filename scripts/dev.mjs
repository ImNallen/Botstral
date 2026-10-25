import { spawn } from "node:child_process";

const children = [
  spawn(
    process.execPath,
    ["node_modules/vite/bin/vite.js", "build", "--watch"],
    { cwd: "web", stdio: "inherit", detached: true },
  ),
  spawn(
    "go",
    [
      "run",
      "./cmd/botstral",
      "--data-dir",
      ".local/data",
      "--port",
      "8080",
      "--setup",
      "--dev-assets",
      "./web/dist",
    ],
    { stdio: "inherit", detached: true },
  ),
];
let stopping = false;
function stop(code) {
  if (stopping) return;
  stopping = true;
  process.exitCode = code;
  for (const child of children) {
    if (child.pid) {
      try {
        process.kill(-child.pid, "SIGTERM");
      } catch (error) {
        if (error.code !== "ESRCH") throw error;
      }
    }
  }
}
for (const child of children) {
  child.on("error", () => stop(1));
  child.on("exit", (code) => stop(code ?? 1));
}
process.on("SIGINT", () => stop(0));
process.on("SIGTERM", () => stop(0));
