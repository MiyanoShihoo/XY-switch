export interface SessionMeta {
  providerId: string; // "claude" | "codex" | "gemini" | "grokbuild" | "opencode" | "openclaw" | "hermes" | "pi"
  sessionId: string;
  title?: string;
  summary?: string;
  projectDir?: string | null;
  createdAt?: number;
  lastActiveAt?: number;
  sourcePath?: string;
  resumeCommand?: string;
}

export interface SessionMessage {
  role: string; // "user" | "assistant" | "system" | "tool"
  content: string;
  ts?: number;
}
