import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

export async function startManager({
  binary,
  dataDir,
  port = 0,
  setup = false,
  cwd,
}) {
  const args = ["--data-dir", dataDir, "--port", String(port)];
  if (setup) args.push("--setup");
  const child = spawn(binary, args, {
    cwd,
    stdio: ["ignore", "ignore", "pipe"],
  });
  const closed = new Promise((resolve) =>
    child.once("close", (code, signal) => resolve({ code, signal })),
  );
  let output = "";
  let mainURL;
  let setupURL;
  let configured = false;
  const lines = createInterface({ input: child.stderr });
  try {
    await new Promise((resolve, reject) => {
      const timer = setTimeout(
        () => reject(new Error("Manager did not start within 10 seconds.")),
        10000,
      );
      const finish = (callback) => {
        clearTimeout(timer);
        callback();
      };
      child.once("error", (error) => finish(() => reject(error)));
      child.once("exit", () =>
        finish(() =>
          reject(new Error(`Manager exited during startup. ${output}`)),
        ),
      );
      lines.on("line", (line) => {
        output += line.replace(/\/#\S+/g, "/#[redacted]") + "\n";
        const main = line.match(/^Manager: (http:\/\/127\.0\.0\.1:\d+)$/);
        const activation = line.match(
          /^Setup .*: (http:\/\/127\.0\.0\.1:\d+\/#\S+)$/,
        );
        if (main) mainURL = main[1];
        if (activation) setupURL = activation[1];
        if (line.startsWith("Owner already configured.")) configured = true;
        if (mainURL && (!setup || setupURL || configured)) finish(resolve);
      });
    });
  } catch (error) {
    child.kill("SIGKILL");
    await closed;
    throw error;
  }
  let stopped = false;
  return {
    mainURL,
    setupURL,
    async stop() {
      if (stopped) return;
      stopped = true;
      if (child.exitCode === null && child.signalCode === null)
        child.kill("SIGTERM");
      const timer = setTimeout(() => child.kill("SIGKILL"), 12000);
      const result = await closed;
      clearTimeout(timer);
      lines.close();
      if (result.code !== 0)
        throw new Error(
          `Manager did not stop cleanly. Code ${result.code}, signal ${result.signal}. ${output}`,
        );
    },
  };
}
