use crate::database::Database;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const SETTINGS_KEY: &str = "skill_import_settings";
const IMPORTS_KEY: &str = "skill_local_imports";
const MAX_SKILL_BYTES: u64 = 256 * 1024;
const MAX_SKILLS: usize = 2000;
const MAX_DIRS: usize = 6000;
const MAX_SCAN_DEPTH: usize = 16;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SkillImportSettings {
    pub codex: bool,
    pub deepseek_harness: bool,
    pub claude: bool,
    pub gemini: bool,
    pub agents: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SkillApps {
    pub claude: bool,
    pub codex: bool,
    pub gemini: bool,
    pub deepseek_harness: bool,
    pub grokbuild: bool,
    pub opencode: bool,
    pub hermes: bool,
    pub pi: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledSkill {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub directory: String,
    pub repo_owner: Option<String>,
    pub repo_name: Option<String>,
    pub repo_branch: Option<String>,
    pub readme_url: Option<String>,
    pub apps: SkillApps,
    pub installed_at: i64,
    pub content_hash: Option<String>,
    pub updated_at: i64,
    pub source: String,
    pub read_only: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSourceStatus {
    pub source: String,
    pub directory: String,
    pub enabled: bool,
    pub discovered: usize,
    pub status: String,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalog {
    pub skills: Vec<InstalledSkill>,
    pub warnings: Vec<String>,
    pub sources: Vec<SkillSourceStatus>,
    pub isolated: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillImportResult {
    pub path: String,
    pub matched: usize,
    pub added: usize,
    pub already_imported: usize,
    pub warnings: Vec<String>,
}

struct ScanSummary {
    discovered: usize,
    status: &'static str,
}

pub(crate) fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

pub(crate) fn agent_home(home: &Path, variable: &str, directory: &str) -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
    if std::env::var_os("XY_SWITCH_TEST_HOME_DIR").is_some() {
        return Ok(home.join(directory));
    }
    let Some(value) = std::env::var_os(variable).filter(|v| !v.is_empty()) else {
        return Ok(home.join(directory));
    };
    let value = value.to_string_lossy();
    let path = if value == "~" {
        home.to_path_buf()
    } else if value.starts_with("~/") || value.starts_with("~\\") {
        home.join(&value[2..])
    } else {
        PathBuf::from(value.as_ref())
    };
    if !path.is_absolute() {
        return Err(format!("{variable} 必须是绝对路径"));
    }
    Ok(path)
}

pub fn settings(database: &Database) -> Result<SkillImportSettings, String> {
    database
        .get_setting(SETTINGS_KEY)?
        .map(|v| serde_json::from_str(&v).map_err(|e| e.to_string()))
        .unwrap_or_else(|| Ok(SkillImportSettings::default()))
}
pub fn save_settings(database: &Database, settings: &SkillImportSettings) -> Result<(), String> {
    database.set_setting(
        SETTINGS_KEY,
        &serde_json::to_string(settings).map_err(|e| e.to_string())?,
    )
}
fn imports(database: &Database) -> Result<Vec<String>, String> {
    database
        .get_setting(IMPORTS_KEY)?
        .map(|v| serde_json::from_str(&v).map_err(|e| e.to_string()))
        .unwrap_or_else(|| Ok(vec![]))
}
fn save_imports(database: &Database, paths: &[String]) -> Result<(), String> {
    database.set_setting(
        IMPORTS_KEY,
        &serde_json::to_string(paths).map_err(|e| e.to_string())?,
    )
}
fn path_id(path: &Path) -> String {
    let text = path.to_string_lossy();
    #[cfg(windows)]
    let text = text.to_lowercase();
    format!("local-{:x}", Sha256::digest(text.as_bytes()))
}

// Parse only presentation metadata; skill content is never evaluated or executed.
fn metadata(text: &str, fallback: &str) -> (String, Option<String>) {
    let text = text.trim_start_matches('\u{feff}');
    let mut name = fallback.to_string();
    let mut description = None;
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return (name, description);
    }
    let mut collecting = false;
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if collecting && (line.starts_with(' ') || line.starts_with('\t')) {
            let value = description.get_or_insert(String::new());
            if !value.is_empty() {
                value.push(' ');
            }
            value.push_str(line.trim());
            continue;
        }
        collecting = false;
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().trim_matches(|c| c == '\'' || c == '"');
        match key.trim() {
            "name" if !value.is_empty() => name = value.chars().take(150).collect(),
            "description" => {
                collecting = matches!(value, "|" | ">" | "|-" | ">-");
                description = Some(if collecting {
                    String::new()
                } else {
                    value.chars().take(2000).collect()
                });
            }
            _ => {}
        }
    }
    (name, description.map(|v| v.chars().take(2000).collect()))
}

fn read_skill(path: &Path, source: &str) -> Result<InstalledSkill, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}：{e}", path.display()))?;
    if is_link(&meta) || !meta.is_file() {
        return Err(format!("跳过链接或非普通技能文件：{}", path.display()));
    }
    if meta.len() > MAX_SKILL_BYTES {
        return Err(format!("技能文件超过 256 KiB：{}", path.display()));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(MAX_SKILL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_SKILL_BYTES {
        return Err("技能文件读取超出大小限制".into());
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| format!("技能文件不是 UTF-8：{}", path.display()))?;
    let canonical = fs::canonicalize(path).map_err(|e| e.to_string())?;
    let directory = canonical.parent().ok_or("技能目录不存在")?;
    let fallback = if path
        .file_name()
        .is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case("SKILL.md"))
    {
        directory.file_name()
    } else {
        path.file_stem()
    }
    .and_then(|s| s.to_str())
    .unwrap_or("Skill");
    let (name, description) = metadata(text, fallback);
    let modified = meta
        .modified()
        .ok()
        .and_then(|v| v.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|v| v.as_millis() as i64)
        .unwrap_or(0);
    let mut apps = SkillApps::default();
    match source {
        "codex" => apps.codex = true,
        "deepseekHarness" => apps.deepseek_harness = true,
        "claude" => apps.claude = true,
        "gemini" => apps.gemini = true,
        _ => {}
    }
    Ok(InstalledSkill {
        id: path_id(&canonical),
        name,
        description,
        directory: directory.to_string_lossy().into_owned(),
        repo_owner: None,
        repo_name: None,
        repo_branch: None,
        readme_url: None,
        apps,
        installed_at: modified,
        updated_at: modified,
        content_hash: Some(format!("{:x}", Sha256::digest(&bytes))),
        source: source.into(),
        read_only: true,
    })
}

fn scan(
    root: &Path,
    source: &str,
    flat: bool,
    result: &mut SkillCatalog,
    seen: &mut HashSet<String>,
) -> ScanSummary {
    let mut summary = ScanSummary {
        discovered: 0,
        status: "ready",
    };
    let warnings_before = result.warnings.len();
    let root_meta = match fs::symlink_metadata(root) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            summary.status = "missing";
            return summary;
        }
        Err(e) => {
            result.warnings.push(format!("{}：{e}", root.display()));
            summary.status = "error";
            return summary;
        }
    };
    if is_link(&root_meta) || !root_meta.is_dir() {
        result
            .warnings
            .push(format!("未读取链接或非目录来源：{}", root.display()));
        summary.status = "error";
        return summary;
    }
    let canonical_root = match fs::canonicalize(root) {
        Ok(p) => p,
        Err(e) => {
            result.warnings.push(format!("{}：{e}", root.display()));
            summary.status = "error";
            return summary;
        }
    };
    let mut stack = vec![(canonical_root.clone(), 0)];
    let mut visited = 0;
    while let Some((dir, depth)) = stack.pop() {
        visited += 1;
        if visited > MAX_DIRS || result.skills.len() >= MAX_SKILLS {
            result
                .warnings
                .push("技能扫描已达到数量限制，请缩小导入范围".into());
            break;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(v) => v,
            Err(e) => {
                result.warnings.push(format!("{}：{e}", dir.display()));
                continue;
            }
        };
        let mut entries = entries
            .filter_map(|entry| match entry {
                Ok(entry) => Some(entry),
                Err(error) => {
                    result.warnings.push(format!("{}：{error}", dir.display()));
                    None
                }
            })
            .collect::<Vec<_>>();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let meta = match fs::symlink_metadata(&path) {
                Ok(meta) => meta,
                Err(error) => {
                    result.warnings.push(format!("{}：{error}", path.display()));
                    continue;
                }
            };
            if is_link(&meta) {
                result
                    .warnings
                    .push(format!("跳过链接或目录联接：{}", path.display()));
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if meta.is_dir() {
                // Do not walk package/build caches, including Codex plugin caches.
                if matches!(
                    name.to_ascii_lowercase().as_str(),
                    ".git" | "node_modules" | "target" | "plugins"
                ) {
                    continue;
                }
                if depth >= MAX_SCAN_DEPTH {
                    result.warnings.push(format!(
                        "目录超过 {MAX_SCAN_DEPTH} 层扫描深度：{}",
                        path.display()
                    ));
                    continue;
                }
                match fs::canonicalize(&path) {
                    Ok(real) if real.starts_with(&canonical_root) => stack.push((real, depth + 1)),
                    Ok(_) => result
                        .warnings
                        .push(format!("目录位于已选来源之外：{}", path.display())),
                    Err(error) => result.warnings.push(format!("{}：{error}", path.display())),
                }
            } else if meta.is_file()
                && (name.eq_ignore_ascii_case("SKILL.md")
                    || (flat
                        && depth == 0
                        && name.to_ascii_lowercase().ends_with(".md")
                        && !name.eq_ignore_ascii_case("README.md")))
            {
                match read_skill(&path, source) {
                    Ok(skill) => {
                        summary.discovered += 1;
                        if seen.insert(skill.id.clone()) {
                            result.skills.push(skill);
                        }
                    }
                    Err(e) => result.warnings.push(e),
                }
                if result.skills.len() >= MAX_SKILLS {
                    result
                        .warnings
                        .push("技能扫描已达到数量限制，请缩小导入范围".into());
                    break;
                }
            }
        }
    }
    if result.warnings.len() > warnings_before {
        summary.status = "partial";
    }
    summary
}

fn skill_file(directory: &Path) -> Result<PathBuf, String> {
    let expected = directory.join("SKILL.md");
    if expected.exists() {
        return Ok(expected);
    }
    // Windows paths are case-insensitive; accept the same names on other hosts.
    fs::read_dir(directory)
        .map_err(|e| format!("{}：{e}", directory.display()))?
        .filter_map(Result::ok)
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("SKILL.md")
        })
        .map(|e| e.path())
        .ok_or_else(|| format!("未找到 SKILL.md：{}", directory.display()))
}

pub fn catalog(
    database: &Database,
    home: &Path,
    codex_home: &Path,
) -> Result<SkillCatalog, String> {
    let settings = settings(database)?;
    let mut result = SkillCatalog::default();
    #[cfg(debug_assertions)]
    {
        result.isolated = std::env::var_os("XY_SWITCH_TEST_HOME_DIR").is_some();
    }
    let mut seen = HashSet::new();
    let sources = [
        (
            "codex",
            settings.codex,
            Ok(codex_home.join("skills")),
            false,
        ),
        (
            "deepseekHarness",
            settings.deepseek_harness,
            agent_home(home, "DSH_HOME", ".dsh").map(|p| p.join("skills")),
            true,
        ),
        (
            "claude",
            settings.claude,
            Ok(home.join(".claude/skills")),
            false,
        ),
        (
            "gemini",
            settings.gemini,
            Ok(home.join(".gemini/skills")),
            false,
        ),
        (
            "agents",
            settings.agents,
            agent_home(home, "DSH_AGENTS_HOME", ".agents").map(|p| p.join("skills")),
            false,
        ),
    ];
    // Closed switches only resolve display paths; they do not read the filesystem.
    // Scan platforms before manual references so their source label stays accurate.
    for (source, enabled, path, flat) in sources {
        let mut status = SkillSourceStatus {
            source: source.into(),
            directory: String::new(),
            enabled,
            discovered: 0,
            status: "disabled".into(),
        };
        match path {
            Ok(path) => {
                status.directory = path.to_string_lossy().into_owned();
                if enabled {
                    let scanned = scan(&path, source, flat, &mut result, &mut seen);
                    status.status = scanned.status.into();
                    status.discovered = scanned.discovered;
                }
            }
            Err(error) => {
                if enabled {
                    status.status = "error".into();
                    result.warnings.push(format!("{source}：{error}"));
                }
            }
        }
        result.sources.push(status);
    }
    // Imports store individual skill directories, not a recursive root. Removing
    // a child reference cannot accidentally rediscover it through its parent.
    for path in imports(database)? {
        let directory = Path::new(&path);
        let skill = fs::symlink_metadata(directory)
            .map_err(|e| format!("{}：{e}", directory.display()))
            .and_then(|meta| {
                if is_link(&meta) || !meta.is_dir() {
                    Err(format!("未读取链接或非目录来源：{}", directory.display()))
                } else {
                    skill_file(directory)
                }
            })
            .and_then(|path| read_skill(&path, "manual"));
        match skill {
            Ok(skill) if result.skills.len() < MAX_SKILLS => {
                if seen.insert(skill.id.clone()) {
                    result.skills.push(skill);
                }
            }
            Ok(_) => {
                result
                    .warnings
                    .push("技能扫描已达到数量限制，请缩小导入范围".into());
                break;
            }
            Err(error) => result.warnings.push(error),
        }
    }
    result
        .skills
        .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    result.warnings.truncate(30);
    Ok(result)
}

pub fn import_folder(database: &Database, path: &str) -> Result<SkillImportResult, String> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err("请选择绝对路径的技能目录".into());
    }
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if is_link(&meta) || !meta.is_dir() {
        return Err("请选择普通文件夹，不支持符号链接或目录联接".into());
    }
    let mut found = SkillCatalog::default();
    scan(path, "manual", false, &mut found, &mut HashSet::new());
    if found.skills.is_empty() {
        return Err(if found.warnings.is_empty() {
            "所选目录及子目录中没有可识别的 SKILL.md".into()
        } else {
            format!("未导入技能：{}", found.warnings.join("；"))
        });
    }
    let mut paths = imports(database)?;
    let mut known: HashSet<_> = paths.iter().map(|p| path_id(Path::new(p))).collect();
    let mut added = 0;
    for skill in &found.skills {
        if known.insert(path_id(Path::new(&skill.directory))) {
            paths.push(skill.directory.clone());
            added += 1;
        }
    }
    if paths.len() > MAX_SKILLS {
        return Err(format!("导入后引用超过 {MAX_SKILLS} 个，请缩小导入范围"));
    }
    save_imports(database, &paths)?;
    found.warnings.truncate(30);
    Ok(SkillImportResult {
        path: path.to_string_lossy().into_owned(),
        matched: found.skills.len(),
        added,
        already_imported: found.skills.len() - added,
        warnings: found.warnings,
    })
}

pub fn remove_reference(database: &Database, id: &str) -> Result<(), String> {
    let mut paths = imports(database)?;
    let before = paths.len();
    paths.retain(|p| {
        let directory = Path::new(p);
        let file = skill_file(directory).unwrap_or_else(|_| directory.join("SKILL.md"));
        path_id(&file) != id
    });
    if before == paths.len() {
        return Err("这是平台扫描结果，请关闭对应读取开关；源文件不会删除".into());
    }
    save_imports(database, &paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn switches_default_closed_and_manual_import_is_read_only() {
        let root = std::env::temp_dir().join(format!("xy-skills-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join(".dsh/skills/demo")).unwrap();
        let path = root.join(".dsh/skills/demo");
        let text = "---\nname: Harness 测试\ndescription: >\n  first line\n  second line\n---\nnever execute this";
        fs::write(path.join("SKILL.md"), text).unwrap();
        let db = Database::open(&root.join("test.db")).unwrap();
        assert!(catalog(&db, &root, &root.join(".codex"))
            .unwrap()
            .skills
            .is_empty());
        let mut found = SkillCatalog::default();
        scan(
            &root.join(".dsh/skills"),
            "deepseekHarness",
            true,
            &mut found,
            &mut HashSet::new(),
        );
        assert_eq!(found.skills.len(), 1);
        assert_eq!(
            found.skills[0].description.as_deref(),
            Some("first line second line")
        );
        import_folder(&db, path.to_str().unwrap()).unwrap();
        import_folder(&db, path.to_str().unwrap()).unwrap();
        let list = catalog(&db, &root, &root.join(".codex")).unwrap();
        assert_eq!(list.skills.len(), 1);
        assert!(list.skills[0].read_only);
        remove_reference(&db, &list.skills[0].id).unwrap();
        assert_eq!(fs::read_to_string(path.join("SKILL.md")).unwrap(), text);
        assert!(catalog(&db, &root, &root.join(".codex"))
            .unwrap()
            .skills
            .is_empty());
        drop(db);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn oversized_and_invalid_utf8_skills_are_reported() {
        let root = std::env::temp_dir().join(format!("xy-skills-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("bad")).unwrap();
        fs::create_dir_all(root.join("huge")).unwrap();
        fs::write(root.join("bad/SKILL.md"), [0xff]).unwrap();
        fs::write(
            root.join("huge/SKILL.md"),
            vec![b'x'; MAX_SKILL_BYTES as usize + 1],
        )
        .unwrap();
        let mut found = SkillCatalog::default();
        scan(&root, "codex", false, &mut found, &mut HashSet::new());
        assert!(found.skills.is_empty());
        assert_eq!(found.warnings.len(), 2);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn platform_permission_round_trips_and_closed_sources_are_not_scanned() {
        let root = std::env::temp_dir().join(format!("xy-skills-{}", uuid::Uuid::new_v4()));
        let codex = root.join(".codex");
        let skill = codex.join("skills/demo/SKILL.md");
        fs::create_dir_all(skill.parent().unwrap()).unwrap();
        fs::create_dir_all(root.join(".dsh/skills/bad")).unwrap();
        fs::write(root.join(".dsh/skills/bad/SKILL.md"), [0xff]).unwrap();
        fs::write(&skill, "---\nname: approved only\n---\ncontent").unwrap();
        let db = Database::open(&root.join("test.db")).unwrap();
        let settings = SkillImportSettings {
            codex: true,
            ..Default::default()
        };
        save_settings(&db, &settings).unwrap();
        let approved = catalog(&db, &root, &codex).unwrap();
        assert_eq!(approved.skills.len(), 1);
        assert_eq!(approved.skills[0].source, "codex");
        assert!(approved.warnings.is_empty());
        assert!(remove_reference(&db, &approved.skills[0].id).is_err());
        drop(db);
        let db = Database::open(&root.join("test.db")).unwrap();
        assert!(super::settings(&db).unwrap().codex);
        save_settings(&db, &SkillImportSettings::default()).unwrap();
        let closed = catalog(&db, &root, &codex).unwrap();
        assert!(closed.skills.is_empty() && closed.warnings.is_empty());
        assert!(skill.exists());
        drop(db);
        fs::remove_dir_all(root).unwrap();
    }

    fn fixture_dir() -> PathBuf {
        let root = std::env::temp_dir().join(format!("xy-skills-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }
    fn write_fixture(root: &Path, relative: &str, name: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            format!("---\nname: {name}\ndescription: Fixture only\n---\nDo not execute."),
        )
        .unwrap();
    }

    #[test]
    fn recursive_sources_include_system_and_nested_skills_but_not_plugin_cache() {
        let root = fixture_dir();
        let codex = root.join(".codex");
        for (relative, name) in [
            ("skills/top/SKILL.md", "Top"),
            ("skills/.system/system-skill/SKILL.md", "System"),
            ("skills/suite/SKILL.md", "Parent"),
            ("skills/suite/child/SKILL.md", "Child"),
            ("skills/lower/skill.md", "Lowercase"),
            (
                "plugins/cache/package/skills/hidden/SKILL.md",
                "Hidden sibling plugin",
            ),
            (
                "skills/plugins/cache/hidden/SKILL.md",
                "Hidden nested plugin",
            ),
        ] {
            write_fixture(&codex, relative, name);
        }
        let db = Database::open(&root.join("test.db")).unwrap();
        save_settings(
            &db,
            &SkillImportSettings {
                codex: true,
                ..Default::default()
            },
        )
        .unwrap();
        let found = catalog(&db, &root, &codex).unwrap();
        assert_eq!(found.skills.len(), 5);
        assert!(!found.skills.iter().any(|s| s.name.starts_with("Hidden")));
        let source = found.sources.iter().find(|s| s.source == "codex").unwrap();
        assert_eq!(source.directory, codex.join("skills").to_string_lossy());
        assert_eq!(source.discovered, 5);
        assert_eq!(source.status, "ready");
        assert!(found.warnings.is_empty());
        drop(db);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn catalog_explains_missing_closed_and_partial_sources() {
        let root = fixture_dir();
        let codex = root.join(".codex");
        write_fixture(&codex, "skills/good/SKILL.md", "Good");
        fs::create_dir_all(codex.join("skills/bad")).unwrap();
        fs::write(codex.join("skills/bad/SKILL.md"), [0xff]).unwrap();
        let db = Database::open(&root.join("test.db")).unwrap();
        let closed = catalog(&db, &root, &codex).unwrap();
        assert_eq!(closed.sources.len(), 5);
        assert!(closed
            .sources
            .iter()
            .all(|s| s.status == "disabled" && s.discovered == 0));
        assert!(closed.warnings.is_empty());
        save_settings(
            &db,
            &SkillImportSettings {
                codex: true,
                claude: true,
                ..Default::default()
            },
        )
        .unwrap();
        let opened = catalog(&db, &root, &codex).unwrap();
        assert_eq!(opened.skills.len(), 1);
        assert_eq!(
            opened
                .sources
                .iter()
                .find(|s| s.source == "codex")
                .unwrap()
                .status,
            "partial"
        );
        assert_eq!(
            opened
                .sources
                .iter()
                .find(|s| s.source == "claude")
                .unwrap()
                .status,
            "missing"
        );
        assert!(opened.warnings[0].contains("UTF-8"));
        drop(db);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bulk_import_counts_all_skills_deduplicates_and_removes_only_one_reference() {
        let root = fixture_dir();
        let collection = root.join("collection");
        for (relative, name) in [
            ("parent/SKILL.md", "Parent"),
            ("parent/child/SKILL.md", "Child"),
            (".system/builtin/SKILL.md", "Builtin"),
        ] {
            write_fixture(&collection, relative, name);
        }
        let db = Database::open(&root.join("test.db")).unwrap();
        let first = import_folder(&db, collection.to_str().unwrap()).unwrap();
        assert_eq!(
            (first.matched, first.added, first.already_imported),
            (3, 3, 0)
        );
        let second = import_folder(&db, collection.to_str().unwrap()).unwrap();
        assert_eq!(
            (second.matched, second.added, second.already_imported),
            (3, 0, 3)
        );
        let list = catalog(&db, &root, &root.join(".codex")).unwrap();
        assert_eq!(list.skills.len(), 3);
        let child = list.skills.iter().find(|s| s.name == "Child").unwrap();
        remove_reference(&db, &child.id).unwrap();
        let after = catalog(&db, &root, &root.join(".codex")).unwrap();
        assert_eq!(after.skills.len(), 2);
        assert!(!after.skills.iter().any(|s| s.name == "Child"));
        assert!(collection.join("parent/child/SKILL.md").exists());
        drop(db);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bulk_import_reports_invalid_entries_and_rejects_an_empty_folder() {
        let root = fixture_dir();
        let collection = root.join("collection");
        write_fixture(&collection, "valid/SKILL.md", "Valid");
        fs::create_dir_all(collection.join("bad")).unwrap();
        fs::write(collection.join("bad/SKILL.md"), [0xff]).unwrap();
        fs::create_dir_all(root.join("empty")).unwrap();
        let db = Database::open(&root.join("test.db")).unwrap();
        let imported = import_folder(&db, collection.to_str().unwrap()).unwrap();
        assert_eq!((imported.matched, imported.added), (1, 1));
        assert_eq!(imported.warnings.len(), 1);
        assert!(import_folder(&db, root.join("empty").to_str().unwrap())
            .unwrap_err()
            .contains("SKILL.md"));
        assert_eq!(
            catalog(&db, &root, &root.join(".codex"))
                .unwrap()
                .skills
                .len(),
            1
        );
        drop(db);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn platform_source_wins_over_duplicate_manual_references() {
        let root = fixture_dir();
        let codex = root.join(".codex");
        write_fixture(&codex, "skills/demo/SKILL.md", "Demo");
        let db = Database::open(&root.join("test.db")).unwrap();
        import_folder(&db, codex.join("skills").to_str().unwrap()).unwrap();
        save_settings(
            &db,
            &SkillImportSettings {
                codex: true,
                ..Default::default()
            },
        )
        .unwrap();
        let enabled = catalog(&db, &root, &codex).unwrap();
        assert_eq!(enabled.skills.len(), 1);
        assert_eq!(enabled.skills[0].source, "codex");
        save_settings(&db, &SkillImportSettings::default()).unwrap();
        let closed = catalog(&db, &root, &codex).unwrap();
        assert_eq!(closed.skills.len(), 1);
        assert_eq!(closed.skills[0].source, "manual");
        drop(db);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn directory_manifest_matches_catalog_without_modifying_source_files() {
        let fixture = fixture_dir();
        // Default CI uses fixtures. An explicit manifest enables read-only local
        // acceptance against a user-selected skills directory, never client config.
        let (skills_root, expected): (PathBuf, Vec<PathBuf>) =
            if let Some(manifest) = std::env::var_os("XY_SWITCH_SKILLS_ACCEPTANCE_MANIFEST") {
                let manifest: serde_json::Value =
                    serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
                let root = PathBuf::from(manifest["root"].as_str().unwrap());
                assert!(root.is_absolute());
                let paths = manifest["files"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| PathBuf::from(s["path"].as_str().unwrap()))
                    .collect();
                (root, paths)
            } else {
                let root = fixture.join(".codex/skills");
                write_fixture(&root, "main/SKILL.md", "Main");
                write_fixture(&root, ".system/nested/SKILL.md", "Nested");
                let expected = vec![
                    root.join("main/SKILL.md"),
                    root.join(".system/nested/SKILL.md"),
                ];
                (root, expected)
            };
        let canonical_root = fs::canonicalize(&skills_root).unwrap();
        let before: Vec<_> = expected
            .iter()
            .map(|path| {
                let canonical = fs::canonicalize(path).unwrap();
                assert!(canonical.starts_with(&canonical_root));
                let digest = format!("{:x}", Sha256::digest(fs::read(&canonical).unwrap()));
                (canonical, digest)
            })
            .collect();
        let db = Database::open(Path::new(":memory:")).unwrap();
        save_settings(
            &db,
            &SkillImportSettings {
                codex: true,
                ..Default::default()
            },
        )
        .unwrap();
        let found = catalog(&db, &fixture, skills_root.parent().unwrap()).unwrap();
        assert!(found.warnings.is_empty(), "{:?}", found.warnings);
        let expected_ids: HashSet<_> = before.iter().map(|(p, _)| path_id(p)).collect();
        let actual_ids: HashSet<_> = found.skills.iter().map(|s| s.id.clone()).collect();
        assert_eq!(actual_ids, expected_ids);
        for (path, digest) in before {
            assert_eq!(
                format!("{:x}", Sha256::digest(fs::read(path).unwrap())),
                digest
            );
        }
        println!(
            "SKILLS_DIRECTORY_ACCEPTANCE {}",
            serde_json::json!({
                "directory": skills_root, "expected": expected.len(), "recognized": found.skills.len(),
                "warnings": found.warnings, "sourceFilesUnchanged": true,
                "names": found.skills.iter().map(|s| &s.name).collect::<Vec<_>>()
            })
        );
        drop(db);
        fs::remove_dir_all(fixture).unwrap();
    }
}
