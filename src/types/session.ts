export interface SessionMeta {
  providerId: string;
  sessionId: string;
  title?: string | null;
  summary?: string | null;
  projectDir?: string | null;
  createdAt?: number | null;
  lastActiveAt?: number | null;
  sourcePath?: string | null;
  resumeCommand?: string | null;
}

export interface SessionMessage {
  role: string;
  content: string;
  ts?: number | null;
}

export interface SessionIdPair {
  providerId: string;
  sessionId: string;
}

export interface SessionCatalog {
  sessions: SessionMeta[];
  warnings: string[];
}

export interface SessionDetail {
  messages: SessionMessage[];
  warnings: string[];
}
