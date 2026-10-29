import { describe, expect, it } from "vitest";
import { formatKeyValueLines, parseKeyValueLines, validateMcpServer } from "./lib/mcp-validation";
import type { McpServer } from "./types/mcp";

const server: McpServer = {
  id: "context7.local",
  name: "Context7",
  description: "",
  transport: "stdio",
  command: "npx",
  args: ["-y", "@upstash/context7-mcp"],
  url: null,
  env: {},
  headers: {},
  apps: { claude: false, codex: false, gemini: false, opencode: false },
  builtIn: false,
  secretsConfigured: false,
};

describe("MCP validation", () => {
  it("parses values containing equals signs", () => {
    expect(parseKeyValueLines("TOKEN=a=b\nEMPTY=")).toEqual({ TOKEN: "a=b", EMPTY: "" });
  });

  it("rejects malformed and duplicate entries", () => {
    expect(() => parseKeyValueLines("TOKEN")).toThrow("KEY=value");
    expect(() => parseKeyValueLines("TOKEN=a\nTOKEN=b")).toThrow("重复");
  });

  it("formats key value records", () => {
    expect(formatKeyValueLines({ A: "1", B: "" })).toBe("A=1\nB=");
  });

  it("validates stdio and HTTP transports", () => {
    expect(validateMcpServer(server)).toBeNull();
    expect(validateMcpServer({ ...server, command: "" })).toContain("命令");
    expect(validateMcpServer({ ...server, transport: "http", command: null, url: "https://example.com/mcp" })).toBeNull();
    expect(validateMcpServer({ ...server, transport: "http", command: null, url: "file:///tmp/mcp" })).toContain("URL");
  });
});
