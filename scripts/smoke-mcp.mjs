import { spawn } from "node:child_process";
import path from "node:path";

const executable = process.argv[2] ?? path.join("mcp", "target", "debug", process.platform === "win32" ? "dicta-mcp.exe" : "dicta-mcp");
const requests = [
  "{",
  JSON.stringify({
    jsonrpc: "2.0",
    id: 1,
    method: "initialize",
    params: {
      protocolVersion: "2025-06-18",
      capabilities: {},
      clientInfo: { name: "stdio-smoke", version: "1" },
    },
  }),
  JSON.stringify({ jsonrpc: "2.0", id: 2, method: "tools/list", params: {} }),
].join("\n") + "\n";

const child = spawn(executable, [], { stdio: ["pipe", "pipe", "pipe"] });
let stdout = "";
let stderr = "";
child.stdout.setEncoding("utf8").on("data", (chunk) => { stdout += chunk; });
child.stderr.setEncoding("utf8").on("data", (chunk) => { stderr += chunk; });
child.stdin.end(requests);

const status = await new Promise((resolve, reject) => {
  const timeout = setTimeout(() => {
    child.kill();
    reject(new Error(`${executable} did not close its stdio transport within 10 seconds`));
  }, 10_000);
  child.once("error", reject);
  child.once("close", (code) => {
    clearTimeout(timeout);
    resolve(code);
  });
});
if (status !== 0) throw new Error(`${executable} exited ${status}: ${stderr.trim()}`);

const responses = stdout.trim().split("\n").map((line) => JSON.parse(line));
if (responses.length !== 3) throw new Error(`Expected 3 responses, got ${responses.length}`);
if (responses[0].error?.code !== -32700) throw new Error("Missing JSON-RPC parse error response");
if (responses[1].result?.serverInfo?.name !== "dicta") throw new Error("Initialize response is invalid");
if (responses[2].result?.tools?.length !== 4) throw new Error("Expected four Dicta tools");
console.log(`MCP stdio smoke: ${responses.length} responses, ${responses[2].result.tools.length} tools`);
