use super::types::*;
use serde::{
    Deserialize,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    fmt,
};

fn error(code: &'static str) -> PluginError {
    PluginError::new(code)
}

struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<Deserializer: de::Deserializer<'de>>(
        deserializer: Deserializer,
    ) -> Result<Self, Deserializer::Error> {
        struct StrictVisitor;
        impl<'de> Visitor<'de> for StrictVisitor {
            type Value = StrictValue;
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("bounded JSON without duplicate keys")
            }
            fn visit_bool<Failure: de::Error>(self, value: bool) -> Result<Self::Value, Failure> {
                Ok(StrictValue(Value::Bool(value)))
            }
            fn visit_i64<Failure: de::Error>(self, value: i64) -> Result<Self::Value, Failure> {
                Ok(StrictValue(value.into()))
            }
            fn visit_u64<Failure: de::Error>(self, value: u64) -> Result<Self::Value, Failure> {
                Ok(StrictValue(value.into()))
            }
            fn visit_f64<Failure: de::Error>(self, value: f64) -> Result<Self::Value, Failure> {
                serde_json::Number::from_f64(value)
                    .map(|number| StrictValue(Value::Number(number)))
                    .ok_or_else(|| Failure::custom("invalid number"))
            }
            fn visit_str<Failure: de::Error>(self, value: &str) -> Result<Self::Value, Failure> {
                Ok(StrictValue(Value::String(value.to_owned())))
            }
            fn visit_string<Failure: de::Error>(
                self,
                value: String,
            ) -> Result<Self::Value, Failure> {
                Ok(StrictValue(Value::String(value)))
            }
            fn visit_none<Failure: de::Error>(self) -> Result<Self::Value, Failure> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_unit<Failure: de::Error>(self) -> Result<Self::Value, Failure> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_seq<Access: SeqAccess<'de>>(
                self,
                mut sequence: Access,
            ) -> Result<Self::Value, Access::Error> {
                let mut values = Vec::new();
                while let Some(StrictValue(value)) = sequence.next_element()? {
                    if values.len() >= 4096 {
                        return Err(de::Error::custom("too many elements"));
                    }
                    values.push(value);
                }
                Ok(StrictValue(Value::Array(values)))
            }
            fn visit_map<Access: MapAccess<'de>>(
                self,
                mut map: Access,
            ) -> Result<Self::Value, Access::Error> {
                let mut values = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.len() >= 4096 || values.contains_key(&key) {
                        return Err(de::Error::custom("invalid object keys"));
                    }
                    let StrictValue(value) = map.next_value()?;
                    values.insert(key, value);
                }
                Ok(StrictValue(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(StrictVisitor)
    }
}

pub fn strict_json(text: &str, max_bytes: usize) -> PluginResult<Value> {
    if text.len() > max_bytes {
        return Err(error("plugin_package_size"));
    }
    serde_json::from_str::<StrictValue>(text)
        .map(|value| value.0)
        .map_err(|_| error("plugin_schema"))
}

pub fn parse_package_json(text: &str) -> PluginResult<PluginPackage> {
    let package = serde_json::from_value(strict_json(text, MAX_PACKAGE_BYTES)?)
        .map_err(|_| error("plugin_schema"))?;
    validate_package(&package)?;
    Ok(package)
}

pub fn parse_published_json(text: &str) -> PluginResult<PublishedPlugin> {
    let published: PublishedPlugin =
        serde_json::from_value(strict_json(text, MAX_PACKAGE_BYTES + 2048)?)
            .map_err(|_| error("plugin_schema"))?;
    validate_published_metadata(&published)?;
    validate_package(&published.package)?;
    Ok(published)
}

pub fn canonical_package_bytes(package: &PluginPackage) -> PluginResult<Vec<u8>> {
    let bytes = serde_json::to_vec(package).map_err(|_| error("plugin_schema"))?;
    if bytes.len() > MAX_PACKAGE_BYTES {
        return Err(error("plugin_package_size"));
    }
    Ok(bytes)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn package_digest(package: &PluginPackage) -> PluginResult<String> {
    Ok(sha256_hex(&canonical_package_bytes(package)?))
}

pub fn signing_payload(id: &str, version: &str, digest: &str) -> PluginResult<Vec<u8>> {
    validate_id(id)?;
    parse_version(version)?;
    validate_hex(digest, 32).map_err(|_| error("plugin_digest"))?;
    Ok(format!("xy-switch-plugin-v1\n{id}\n{version}\n{digest}").into_bytes())
}

pub fn withdrawal_payload(notice: &PluginWithdrawal) -> PluginResult<Vec<u8>> {
    validate_id(&notice.id)?;
    parse_version(&notice.version)?;
    validate_hex(&notice.digest, 32).map_err(|_| error("plugin_digest"))?;
    Ok(format!(
        "xy-switch-plugin-revocation-v1\n{}\n{}\n{}\n{}",
        notice.id,
        notice.version,
        notice.digest,
        notice.reason.as_str()
    )
    .into_bytes())
}

pub fn validate_hex(text: &str, bytes: usize) -> PluginResult<()> {
    if text.len() != bytes * 2
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(error("plugin_signature"));
    }
    Ok(())
}

pub fn decode_hex(text: &str, bytes: usize) -> PluginResult<Vec<u8>> {
    validate_hex(text, bytes)?;
    let digit = |byte: u8| {
        if byte.is_ascii_digit() {
            byte - b'0'
        } else {
            byte - b'a' + 10
        }
    };
    Ok(text
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| digit(pair[0]) * 16 + digit(pair[1]))
        .collect())
}

pub fn validate_id(id: &str) -> PluginResult<()> {
    if id.is_empty()
        || id.len() > 96
        || id.split('.').any(|part| {
            part.is_empty()
                || part.len() > 48
                || !part.as_bytes()[0].is_ascii_lowercase()
                || part.ends_with('-')
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
    {
        return Err(error("plugin_schema"));
    }
    Ok(())
}

pub fn validate_client(client: &str) -> PluginResult<()> {
    if !SUPPORTED_CLIENTS.contains(&client) {
        return Err(error("plugin_client"));
    }
    Ok(())
}

pub fn validate_package_path(path: &str) -> PluginResult<()> {
    if path.is_empty() || path.len() > 240 || path.split('/').count() > 12 {
        return Err(error("plugin_path"));
    }
    for segment in path.split('/') {
        if segment.is_empty()
            || segment.len() > 96
            || segment == "."
            || segment == ".."
            || segment.ends_with('.')
            || segment.starts_with('.')
            || !segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(error("plugin_path"));
        }
        let stem = segment.split('.').next().unwrap_or("").to_ascii_uppercase();
        if matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" | "CLOCK$"
        ) || stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9')
        {
            return Err(error("plugin_path"));
        }
    }
    Ok(())
}

fn bounded_text(text: &str, max_chars: usize, empty: bool) -> PluginResult<()> {
    if (!empty && text.trim().is_empty()) || text.chars().count() > max_chars || text.chars().any(|character|
        character == '\0' || character.is_control() && !matches!(character, '\n' | '\r' | '\t')
        || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200b}' | '\u{feff}')) {
        return Err(error("plugin_schema"));
    }
    Ok(())
}

fn inspect_private_value(value: Value) -> PluginResult<()> {
    let inspection = crate::privacy::inspect(
        serde_json::json!({"metadata": value}),
        &Default::default(),
        &[],
    )
    .map_err(|_| error("plugin_privacy"))?;
    if inspection.blocked_code.is_some()
        || inspection.report.secret_redactions > 0
        || inspection.report.personal_redactions > 0
        || inspection.report.uninspected_attachments > 0
        || inspection.report.protected_fields > 0
    {
        return Err(error("plugin_privacy"));
    }
    Ok(())
}

pub(super) fn inspect_embedded_json(value: &Value) -> PluginResult<()> {
    fn candidate(text: &str) -> bool {
        let text = text.trim();
        let bytes = text.as_bytes();
        match (bytes.first(), bytes.last()) {
            (Some(b'{'), Some(b'}')) => {
                matches!(text[1..].trim_start().as_bytes().first(), Some(b'"' | b'}'))
            }
            (Some(b'['), Some(b']')) => matches!(
                text[1..].trim_start().as_bytes().first(),
                Some(b'{' | b'[' | b'"' | b']' | b'-' | b'0'..=b'9' | b't' | b'f' | b'n')
            ),
            (Some(b'"'), Some(b'"')) => bytes.len() >= 2,
            _ => false,
        }
    }
    fn visit(
        value: &Value,
        depth: usize,
        nodes: &mut usize,
        scanned: &mut usize,
    ) -> PluginResult<()> {
        *nodes = nodes.saturating_add(1);
        if depth > 32 || *nodes > 32 * 1024 {
            return Err(error("plugin_privacy"));
        }
        match value {
            Value::String(text) => {
                *scanned = scanned.saturating_add(text.len());
                if *scanned > 4 * MAX_PACKAGE_BYTES {
                    return Err(error("plugin_privacy"));
                }
                if candidate(text) {
                    let decoded = strict_json(text, MAX_PACKAGE_BYTES)
                        .map_err(|_| error("plugin_privacy"))?;
                    visit(&decoded, depth + 1, nodes, scanned)?;
                    inspect_private_value(decoded)?;
                }
            }
            Value::Array(items) => {
                for item in items {
                    visit(item, depth + 1, nodes, scanned)?;
                }
            }
            Value::Object(fields) => {
                for (key, child) in fields {
                    *scanned = scanned.saturating_add(key.len());
                    if *scanned > 4 * MAX_PACKAGE_BYTES {
                        return Err(error("plugin_privacy"));
                    }
                    visit(child, depth + 1, nodes, scanned)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    visit(value, 0, &mut 0, &mut 0)
}

pub fn inspect_package_privacy(value: Value) -> PluginResult<()> {
    inspect_embedded_json(&value)?;
    inspect_private_value(value)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticVersion {
    core: [u64; 3],
    prerelease: Vec<String>,
}

pub fn parse_version(text: &str) -> PluginResult<SemanticVersion> {
    if text.is_empty() || text.len() > 64 || !text.is_ascii() {
        return Err(error("plugin_semver"));
    }
    let (without_build, build) = text
        .split_once('+')
        .map_or((text, None), |(head, tail)| (head, Some(tail)));
    let valid_identifiers = |identifiers: &str, numeric_leading_zero: bool| {
        !identifiers.is_empty()
            && identifiers.split('.').all(|identifier| {
                !identifier.is_empty()
                    && identifier
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                    && (!numeric_leading_zero
                        || !identifier.bytes().all(|byte| byte.is_ascii_digit())
                        || identifier == "0"
                        || !identifier.starts_with('0'))
            })
    };
    if build.is_some_and(|value| !valid_identifiers(value, false)) {
        return Err(error("plugin_semver"));
    }
    let (core, prerelease) = without_build
        .split_once('-')
        .map_or((without_build, None), |(head, tail)| (head, Some(tail)));
    let pieces: Vec<_> = core.split('.').collect();
    if pieces.len() != 3
        || pieces.iter().any(|piece| {
            piece.is_empty()
                || !piece.bytes().all(|byte| byte.is_ascii_digit())
                || piece.len() > 1 && piece.starts_with('0')
        })
        || prerelease.is_some_and(|value| !valid_identifiers(value, true))
    {
        return Err(error("plugin_semver"));
    }
    let mut numbers = [0; 3];
    for (index, piece) in pieces.iter().enumerate() {
        numbers[index] = piece.parse().map_err(|_| error("plugin_semver"))?;
    }
    Ok(SemanticVersion {
        core: numbers,
        prerelease: prerelease
            .map(|value| value.split('.').map(str::to_owned).collect())
            .unwrap_or_default(),
    })
}

impl Ord for SemanticVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        let core = self.core.cmp(&other.core);
        if core != Ordering::Equal {
            return core;
        }
        match (self.prerelease.is_empty(), other.prerelease.is_empty()) {
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            _ => {}
        }
        for (left, right) in self.prerelease.iter().zip(&other.prerelease) {
            let numeric = |value: &str| value.bytes().all(|byte| byte.is_ascii_digit());
            let comparison = match (numeric(left), numeric(right)) {
                (true, true) => left.len().cmp(&right.len()).then_with(|| left.cmp(right)),
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                _ => left.cmp(right),
            };
            if comparison != Ordering::Equal {
                return comparison;
            }
        }
        self.prerelease.len().cmp(&other.prerelease.len())
    }
}
impl PartialOrd for SemanticVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn parse_dependency(dependency: &str) -> PluginResult<(&str, &str)> {
    let (id, version) = dependency
        .split_once('@')
        .ok_or_else(|| error("plugin_dependency"))?;
    validate_id(id).map_err(|_| error("plugin_dependency"))?;
    parse_version(version).map_err(|_| error("plugin_dependency"))?;
    Ok((id, version))
}

pub fn approved_origin(origin: &str) -> PluginResult<String> {
    let url = reqwest::Url::parse(origin).map_err(|_| error("plugin_permissions"))?;
    let host = url.host_str().ok_or_else(|| error("plugin_permissions"))?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || !host.is_ascii()
        || !host.contains('.')
        || host.parse::<std::net::IpAddr>().is_ok()
        || host.ends_with('.')
        || [".localhost", ".local", ".internal", ".lan"]
            .iter()
            .any(|suffix| host.ends_with(suffix))
    {
        return Err(error("plugin_permissions"));
    }
    let canonical = url.origin().ascii_serialization();
    if canonical != origin {
        return Err(error("plugin_permissions"));
    }
    Ok(canonical)
}

fn validate_file_scope(scope: &str) -> PluginResult<()> {
    if let Some(relative) = scope.strip_prefix("package:") {
        return validate_package_path(relative.strip_suffix('/').unwrap_or(relative))
            .map_err(|_| error("plugin_permissions"));
    }
    let relative = if let Some(relative) = scope.strip_prefix('/') {
        relative
    } else if scope.len() > 3 && scope.as_bytes()[0].is_ascii_uppercase() && &scope[1..3] == ":/" {
        &scope[3..]
    } else {
        return Err(error("plugin_permissions"));
    };
    let relative = relative.strip_suffix('/').unwrap_or(relative);
    if relative.split('/').count() < 2 {
        return Err(error("plugin_permissions"));
    }
    validate_package_path(relative).map_err(|_| error("plugin_permissions"))
}

pub fn validate_permissions(permissions: &PluginPermissions) -> PluginResult<()> {
    for scopes in [
        &permissions.network,
        &permissions.read_paths,
        &permissions.write_paths,
        &permissions.credentials,
    ] {
        if scopes.len() > 16 || scopes.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(error("plugin_permissions"));
        }
    }
    for origin in &permissions.network {
        approved_origin(origin)?;
    }
    for scope in permissions
        .read_paths
        .iter()
        .chain(&permissions.write_paths)
    {
        validate_file_scope(scope)?;
    }
    for credential in &permissions.credentials {
        validate_id(credential).map_err(|_| error("plugin_permissions"))?;
    }
    Ok(())
}

pub fn require_supported_runtime(permissions: &PluginPermissions) -> PluginResult<()> {
    if permissions.process {
        return Err(error("plugin_process_unsupported"));
    }
    if !permissions.credentials.is_empty() {
        return Err(error("plugin_credentials_unsupported"));
    }
    if !permissions.write_paths.is_empty() {
        return Err(error("plugin_write_unsupported"));
    }
    if permissions
        .read_paths
        .iter()
        .any(|scope| !scope.starts_with("package:"))
    {
        return Err(error("plugin_external_read_unsupported"));
    }
    Ok(())
}

pub fn package_scope_allows(scope: &str, path: &str) -> bool {
    let Some(relative) = scope.strip_prefix("package:") else {
        return false;
    };
    if let Some(directory) = relative.strip_suffix('/') {
        path.strip_prefix(directory)
            .is_some_and(|tail| tail.starts_with('/') && tail.len() > 1)
    } else {
        relative == path
    }
}

fn argument_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 48
        && name.as_bytes()[0].is_ascii_lowercase()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn validate_tool_schema(schema: &ToolInputSchema) -> PluginResult<()> {
    if schema.schema_type != "object"
        || schema.additional_properties
        || schema.properties.len() > 24
        || schema.required.len() > 24
    {
        return Err(error("plugin_entry"));
    }
    let mut required = BTreeSet::new();
    for name in &schema.required {
        if !schema.properties.contains_key(name) || !required.insert(name) {
            return Err(error("plugin_entry"));
        }
    }
    for (name, property) in &schema.properties {
        if !argument_name(name)
            || !matches!(
                property.value_type.as_str(),
                "string" | "boolean" | "number" | "integer" | "object" | "array"
            )
            || property
                .max_length
                .is_some_and(|limit| limit == 0 || limit > 4096 || property.value_type != "string")
        {
            return Err(error("plugin_entry"));
        }
        if let Some(description) = &property.description {
            bounded_text(description, 256, true)?;
        }
        if let Some(values) = &property.enum_values {
            if values.is_empty()
                || values.len() > 32
                || values
                    .iter()
                    .any(|value| !scalar_matches(value, &property.value_type))
            {
                return Err(error("plugin_entry"));
            }
        }
    }
    Ok(())
}

fn validate_tools(bundle: &ToolBundle, permissions: &PluginPermissions) -> PluginResult<()> {
    if bundle.schema_version != 1 || bundle.tools.is_empty() || bundle.tools.len() > 32 {
        return Err(error("plugin_entry"));
    }
    let mut names = BTreeSet::new();
    for tool in &bundle.tools {
        if !argument_name(&tool.name) || !names.insert(&tool.name) {
            return Err(error("plugin_entry"));
        }
        bounded_text(&tool.description, 500, false)?;
        validate_tool_schema(&tool.input_schema)?;
        let source = match &tool.operation {
            ToolOperation::JsonPassthrough => None,
            ToolOperation::ReadText { path_argument } => {
                if permissions.read_paths.is_empty() {
                    return Err(error("plugin_permissions"));
                }
                Some(path_argument)
            }
            ToolOperation::HttpGet { url_argument } => {
                if permissions.network.is_empty() {
                    return Err(error("plugin_permissions"));
                }
                Some(url_argument)
            }
        };
        if let Some(name) = source {
            if !tool.input_schema.required.contains(name)
                || !tool
                    .input_schema
                    .properties
                    .get(name)
                    .is_some_and(|property| property.value_type == "string")
            {
                return Err(error("plugin_entry"));
            }
        }
    }
    Ok(())
}

fn model_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 120
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/' | b':')
        })
}

pub fn validate_package(package: &PluginPackage) -> PluginResult<ValidatedPackage> {
    let bytes = canonical_package_bytes(package)?;
    let manifest = &package.manifest;
    if manifest.schema_version != PACKAGE_SCHEMA_VERSION {
        return Err(error("plugin_schema"));
    }
    validate_id(&manifest.id)?;
    bounded_text(&manifest.name, 80, false)?;
    bounded_text(&manifest.description, 2000, false)?;
    bounded_text(&manifest.author, 80, false)?;
    if manifest.name.trim() != manifest.name || manifest.author.trim() != manifest.author {
        return Err(error("plugin_schema"));
    }
    parse_version(&manifest.version)?;
    if parse_version(&manifest.minimum_host_version)? > parse_version(HOST_VERSION)? {
        return Err(error("plugin_host_version"));
    }
    if manifest.clients.is_empty() || manifest.clients.len() > SUPPORTED_CLIENTS.len() {
        return Err(error("plugin_client"));
    }
    let mut clients = BTreeSet::new();
    for client in &manifest.clients {
        validate_client(client)?;
        if !clients.insert(client) {
            return Err(error("plugin_client"));
        }
    }
    validate_permissions(&manifest.permissions)?;
    if manifest.dependencies.len() > 32 {
        return Err(error("plugin_dependency"));
    }
    let mut dependencies = BTreeSet::new();
    let mut dependency_privacy = Vec::with_capacity(manifest.dependencies.len());
    for dependency in &manifest.dependencies {
        let (id, version) = parse_dependency(dependency)?;
        if id == manifest.id || !dependencies.insert(id) {
            return Err(error("plugin_dependency"));
        }
        dependency_privacy.push(serde_json::json!({"id": id, "version": version}));
    }
    if package.files.is_empty() || package.files.len() > MAX_PACKAGE_FILES {
        return Err(error("plugin_package_size"));
    }
    validate_package_path(&manifest.entry)?;
    let mut segments = BTreeMap::new();
    let mut file_paths = BTreeSet::new();
    for (path, text) in &package.files {
        validate_package_path(path)?;
        if text.len() > MAX_FILE_BYTES {
            return Err(error("plugin_package_size"));
        }
        bounded_text(text, MAX_FILE_BYTES, true)?;
        let extension = path.rsplit_once('.').map(|(_, suffix)| suffix);
        if !matches!(extension, Some("json" | "md" | "txt")) {
            return Err(error("plugin_entry"));
        }
        let mut prefix = String::new();
        for segment in path.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(segment);
            if let Some(previous) = segments.insert(prefix.to_ascii_lowercase(), prefix.clone()) {
                if previous != prefix {
                    return Err(error("plugin_path"));
                }
            }
        }
        if !file_paths.insert(path.to_ascii_lowercase()) {
            return Err(error("plugin_path"));
        }
        if extension == Some("json") {
            inspect_package_privacy(strict_json(text, MAX_FILE_BYTES)?)?;
        }
    }
    for path in &file_paths {
        for (offset, _) in path.match_indices('/') {
            if file_paths.contains(&path[..offset]) {
                return Err(error("plugin_path"));
            }
        }
    }
    let mut privacy_package = serde_json::to_value(package).map_err(|_| error("plugin_schema"))?;
    let privacy_dependencies = privacy_package
        .get_mut("manifest")
        .and_then(|manifest| manifest.get_mut("dependencies"))
        .ok_or_else(|| error("plugin_schema"))?;
    *privacy_dependencies = Value::Array(dependency_privacy);
    inspect_package_privacy(privacy_package)?;
    let text = package
        .files
        .get(&manifest.entry)
        .ok_or_else(|| error("plugin_entry"))?;
    let entry = match manifest.kind {
        PluginKind::Mcp => {
            if !manifest.entry.ends_with(".json") {
                return Err(error("plugin_entry"));
            }
            let bundle: ToolBundle = serde_json::from_value(strict_json(text, MAX_FILE_BYTES)?)
                .map_err(|_| error("plugin_entry"))?;
            validate_tools(&bundle, &manifest.permissions)?;
            PluginEntry::Mcp(bundle)
        }
        PluginKind::Skill => {
            if !manifest.entry.ends_with(".md")
                || text.trim().is_empty()
                || !manifest.permissions.is_empty()
                || text.contains("<script")
                || text.contains("data:")
            {
                return Err(error("plugin_entry"));
            }
            PluginEntry::Skill(text.clone())
        }
        PluginKind::Provider => {
            if !manifest.entry.ends_with(".json") || !manifest.permissions.is_empty() {
                return Err(error("plugin_entry"));
            }
            let mapping: ProviderMapping =
                serde_json::from_value(strict_json(text, MAX_FILE_BYTES)?)
                    .map_err(|_| error("plugin_entry"))?;
            if mapping.schema_version != 1
                || mapping.models.is_empty()
                || mapping.models.len() > 128
                || mapping
                    .models
                    .iter()
                    .any(|(source, target)| !model_name(source) || !model_name(target))
            {
                return Err(error("plugin_entry"));
            }
            PluginEntry::Provider(mapping)
        }
    };
    Ok(ValidatedPackage {
        digest: sha256_hex(&bytes),
        canonical_size: bytes.len(),
        file_count: package.files.len(),
        entry,
    })
}

pub fn validate_published_metadata(published: &PublishedPlugin) -> PluginResult<()> {
    validate_id(&published.signing_key_id)?;
    validate_hex(&published.digest, 32).map_err(|_| error("plugin_digest"))?;
    validate_hex(&published.signature, 64)?;
    Ok(())
}

pub fn validate_trust_key(input: &TrustedKeyInput) -> PluginResult<String> {
    validate_id(&input.id)?;
    bounded_text(&input.label, 80, false)?;
    inspect_package_privacy(serde_json::json!({"id":input.id,"label":input.label}))?;
    Ok(sha256_hex(&decode_hex(&input.public_key, 32)?))
}

pub fn verify_published(
    published: &PublishedPlugin,
    key: &TrustedSigningKey,
) -> PluginResult<ValidatedPackage> {
    validate_published_metadata(published)?;
    let validated = validate_package(&published.package)?;
    if validated.digest != published.digest {
        return Err(error("plugin_digest"));
    }
    if !key.enabled || key.id != published.signing_key_id {
        return Err(error("plugin_untrusted_key"));
    }
    let public_key = decode_hex(&key.public_key, 32)?;
    if sha256_hex(&public_key) != key.fingerprint {
        return Err(error("plugin_untrusted_key"));
    }
    let signature = decode_hex(&published.signature, 64)?;
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, public_key)
        .verify(
            &signing_payload(
                &published.package.manifest.id,
                &published.package.manifest.version,
                &published.digest,
            )?,
            &signature,
        )
        .map_err(|_| error("plugin_signature"))?;
    Ok(validated)
}

pub fn validation_summary(
    package: &PluginPackage,
    validated: &ValidatedPackage,
) -> PackageValidation {
    let unsupported = require_supported_runtime(&package.manifest.permissions).err();
    PackageValidation {
        digest: validated.digest.clone(),
        canonical_size: validated.canonical_size,
        file_count: validated.file_count,
        manifest: package.manifest.clone(),
        runtime_supported: unsupported.is_none(),
        warnings: unsupported
            .map(|failure| vec![failure.code.to_owned()])
            .unwrap_or_default(),
    }
}

fn scalar_matches(value: &Value, kind: &str) -> bool {
    match kind {
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        _ => false,
    }
}

pub fn validate_tool_arguments(schema: &ToolInputSchema, arguments: &Value) -> PluginResult<()> {
    let bytes = serde_json::to_vec(arguments).map_err(|_| error("plugin_tool_arguments"))?;
    if bytes.len() > MAX_TOOL_BYTES {
        return Err(error("plugin_tool_arguments"));
    }
    let mut nodes = 0;
    fn bounded(value: &Value, depth: usize, nodes: &mut usize) -> bool {
        *nodes += 1;
        if depth > 16 || *nodes > 2048 {
            return false;
        }
        match value {
            Value::Array(items) => {
                items.len() <= 128 && items.iter().all(|item| bounded(item, depth + 1, nodes))
            }
            Value::Object(fields) => {
                fields.len() <= 128
                    && fields
                        .iter()
                        .all(|(key, value)| key.len() <= 128 && bounded(value, depth + 1, nodes))
            }
            Value::String(text) => text.len() <= MAX_TOOL_BYTES && !text.contains('\0'),
            _ => true,
        }
    }
    if !bounded(arguments, 0, &mut nodes) {
        return Err(error("plugin_tool_arguments"));
    }
    let fields = arguments
        .as_object()
        .ok_or_else(|| error("plugin_tool_arguments"))?;
    if schema
        .required
        .iter()
        .any(|name| !fields.contains_key(name))
    {
        return Err(error("plugin_tool_arguments"));
    }
    for (name, value) in fields {
        let property = schema
            .properties
            .get(name)
            .ok_or_else(|| error("plugin_tool_arguments"))?;
        if !scalar_matches(value, &property.value_type)
            || property.max_length.is_some_and(|limit| {
                value
                    .as_str()
                    .is_some_and(|text| text.chars().count() > limit as usize)
            })
            || property
                .enum_values
                .as_ref()
                .is_some_and(|allowed| !allowed.contains(value))
        {
            return Err(error("plugin_tool_arguments"));
        }
    }
    inspect_package_privacy(arguments.clone())
}
