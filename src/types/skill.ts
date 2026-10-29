export type SkillAppType = "claude" | "codex" | "gemini" | "grokbuild" | "opencode" | "hermes" | "pi";

export interface SkillApps {
  claude: boolean;
  codex: boolean;
  gemini: boolean;
  grokbuild: boolean;
  opencode: boolean;
  hermes: boolean;
  pi: boolean;
}

export interface SkillImportSettings {
  codex: boolean; deepseekHarness: boolean; claude: boolean; gemini: boolean; agents: boolean;
}
export interface SkillSourceStatus {
  source: keyof SkillImportSettings;
  directory: string;
  enabled: boolean;
  discovered: number;
  status: "disabled" | "missing" | "ready" | "partial" | "error";
}
export interface SkillCatalog {
  skills: InstalledSkill[];
  warnings: string[];
  sources: SkillSourceStatus[];
  isolated: boolean;
}
export interface SkillImportResult {
  path: string;
  matched: number;
  added: number;
  alreadyImported: number;
  warnings: string[];
}

export interface InstalledSkill {
  source: string;
  readOnly: boolean;
  id: string;
  name: string;
  description?: string;
  directory: string;
  repoOwner?: string;
  repoName?: string;
  repoBranch?: string;
  readmeUrl?: string;
  apps: SkillApps;
  installedAt: number;
  contentHash?: string;
  updatedAt: number;
}

export interface DiscoverableSkill {
  key: string;
  name: string;
  description: string;
  directory: string;
  readmeUrl?: string;
  repoOwner: string;
  repoName: string;
  repoBranch: string;
}

export interface SkillRepo {
  owner: string;
  name: string;
  branch: string;
  enabled: boolean;
}

export interface SkillUpdateInfo {
  id: string;
  name: string;
  currentHash?: string;
  remoteHash: string;
}
