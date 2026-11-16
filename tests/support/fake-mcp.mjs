// Tiny MCP stdio server for ge-bridge tests (newline-delimited JSON-RPC 2.0, no deps).
// argv[2] = serverInfo name (default "fake"). Env: FAKE_MCP_PAGES=1 splits tools/list in two pages,
// FAKE_MCP_HANG=1 never answers initialize. After initialize it pings the client and ignores the reply.
import readline from "node:readline";

const name = process.argv[2] || "fake";
const PNG = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x66, 0x61, 0x6b, 0x65]);
const obj = (properties = {}, required = []) => ({ type: "object", properties, required });
const TOOLS = [
  { name: "echo", description: "Echo text", inputSchema: obj({ text: { type: "string" } }, ["text"]) },
  { name: "image", description: "Make an image", inputSchema: obj() },
  { name: "fail", description: "Always fails", inputSchema: obj() },
  { name: "slow", description: "Answer after ms", inputSchema: obj({ ms: { type: "number" } }) },
  { name: "use_tool", description: "Run a named tool", inputSchema: obj({ name: { type: "string" }, arguments: { type: "object" } }) },
];

const send = (m) => process.stdout.write(JSON.stringify({ jsonrpc: "2.0", ...m }) + "\n");
const text = (t, extra = {}) => ({ content: [{ type: "text", text: t }], ...extra });

function callTool(n, a = {}) {
  switch (n) {
    case "echo": return text(`echo: ${a.text}`);
    case "image":
      return { content: [{ type: "image", mimeType: "image/png", data: PNG.toString("base64") }, { type: "text", text: "Image made." }] };
    case "fail": return text("it failed", { isError: true });
    case "use_tool": return text(`ran ${a.name} with ${JSON.stringify(a.arguments)}`);
    default: return null;
  }
}

function handle(msg) {
  const { id, method, params } = msg;
  if (method === undefined || id === undefined) return; // responses and notifications
  if (method === "initialize") {
    if (process.env.FAKE_MCP_HANG === "1") return;
    send({ id, result: { protocolVersion: "2025-06-18", capabilities: { tools: {} }, serverInfo: { name, version: "0.0.1" } } });
    send({ id: "ping-1", method: "ping" });
  } else if (method === "tools/list") {
    if (process.env.FAKE_MCP_PAGES === "1") {
      const second = params && params.cursor === "p2";
      send({ id, result: second ? { tools: TOOLS.slice(2) } : { tools: TOOLS.slice(0, 2), nextCursor: "p2" } });
    } else send({ id, result: { tools: TOOLS } });
  } else if (method === "tools/call") {
    const { name: n, arguments: a } = params;
    if (n === "slow") setTimeout(() => send({ id, result: text("slow done") }), Number(a?.ms) || 0);
    else {
      const r = callTool(n, a);
      send(r ? { id, result: r } : { id, error: { code: -32602, message: `unknown tool ${n}` } });
    }
  } else send({ id, error: { code: -32601, message: "method not found" } });
}

readline.createInterface({ input: process.stdin }).on("line", (l) => {
  try { handle(JSON.parse(l)); } catch { /* ignore garbage */ }
});
