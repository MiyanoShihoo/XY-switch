use ring::{aead, rand::{SecureRandom, SystemRandom}};
use std::{fs, path::{Path, PathBuf}, sync::Arc};
#[cfg(not(test))]
use std::fs::OpenOptions;
use zeroize::Zeroizing;

const MAGIC: &[u8; 8] = b"\x89XYBK01\n";
const HEADER: usize = 20;
const MAX_PLAINTEXT: usize = 8 * 1024 * 1024;
const MAX_FILE: usize = MAX_PLAINTEXT + HEADER + 16;

fn backup_key_id(directory: &Path) -> Result<String, String> {
    let directory = fs::canonicalize(directory).map_err(|_| "无法确认备份保护目录")?;
    let digest = ring::digest::digest(&ring::digest::SHA256, directory.as_os_str().as_encoded_bytes());
    Ok(format!("takeover-backup-v1-{}", digest.as_ref().iter().map(|byte| format!("{byte:02x}")).collect::<String>()))
}

fn encrypted_backup_present(directory: &Path) -> Result<bool, String> {
    for entry in fs::read_dir(directory).map_err(|_| "无法检查既有客户端备份")? {
        let entry = entry.map_err(|_| "无法检查既有客户端备份")?;
        if entry.path().extension().is_some_and(|extension| extension == "bin") {
            let bytes = Zeroizing::new(BackupCipher::read(&entry.path())?);
            if BackupCipher::encrypted(&bytes) { return Ok(true); }
        }
    }
    Ok(false)
}

trait BackupKeys: Send + Sync {
    fn key(&self, lock: &Path, create: bool) -> Result<Zeroizing<[u8; 32]>, String>;
}

#[cfg(not(test))]
struct NativeKeys;

#[cfg(not(test))]
impl BackupKeys for NativeKeys {
    fn key(&self, path: &Path, create: bool) -> Result<Zeroizing<[u8; 32]>, String> {
        let directory = path.parent().ok_or("备份密钥锁路径无效")?;
        fs::create_dir_all(directory).map_err(|_| "无法创建备份保护目录")?;
        crate::client_config::regular_file(path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(path).map_err(|_| "无法打开备份密钥锁")?;
        lock.try_lock().map_err(|_| "其他应用实例正在初始化备份保护，请稍后重试")?;
        let key_id = backup_key_id(directory)?;
        let existing = crate::credential_store::get_secret("internal", &key_id).map_err(|_| "无法访问系统备份密钥，请检查系统凭据存储")?;
        if let Some(encoded) = existing {
            let encoded = Zeroizing::new(encoded);
            if encoded.len() != 64 { return Err("系统备份密钥格式无效，已保留原始备份".into()); }
            let mut key = Zeroizing::new([0u8; 32]);
            for (index, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
                let text = std::str::from_utf8(pair).map_err(|_| "系统备份密钥格式无效")?;
                key[index] = u8::from_str_radix(text, 16).map_err(|_| "系统备份密钥格式无效")?;
            }
            return Ok(key);
        }
        if !create { return Err("系统备份密钥缺失，已保留备份，请恢复系统凭据存储".into()); }
        if encrypted_backup_present(directory)? {
            return Err("既有加密备份的系统密钥缺失，已保留备份且未生成替代密钥".into());
        }
        let mut key = Zeroizing::new([0u8; 32]);
        SystemRandom::new().fill(key.as_mut()).map_err(|_| "安全随机数不可用")?;
        let encoded = Zeroizing::new(key.iter().map(|byte| format!("{byte:02x}")).collect::<String>());
        crate::credential_store::set_secret("internal", &key_id, &encoded).map_err(|_| "无法保护原始客户端备份，配置尚未修改")?;
        Ok(key)
    }
}

#[cfg(test)]
struct TestKeys([u8; 32]);

#[cfg(test)]
impl BackupKeys for TestKeys {
    fn key(&self, _: &Path, _: bool) -> Result<Zeroizing<[u8; 32]>, String> { Ok(Zeroizing::new(self.0)) }
}

#[derive(Clone)]
pub(crate) struct BackupCipher {
    keys: Arc<dyn BackupKeys>,
    key_lock: PathBuf,
}

impl BackupCipher {
    pub(crate) fn new(directory: &Path) -> Self {
        #[cfg(not(test))]
        let keys = Arc::new(NativeKeys);
        #[cfg(test)]
        let keys = Arc::new(TestKeys([0x47; 32]));
        Self { keys, key_lock: directory.join("takeover-key.lock") }
    }

    pub(crate) fn encrypted(bytes: &[u8]) -> bool { bytes.starts_with(MAGIC) }

    fn cipher(&self, create: bool) -> Result<aead::LessSafeKey, String> {
        let key = self.keys.key(&self.key_lock, create)?;
        aead::UnboundKey::new(&aead::AES_256_GCM, key.as_ref()).map(aead::LessSafeKey::new).map_err(|_| "备份加密密钥初始化失败".into())
    }

    pub(crate) fn seal(&self, content: &[u8], context: &[u8]) -> Result<Vec<u8>, String> {
        if content.len() > MAX_PLAINTEXT { return Err("客户端备份超过保护大小限制".into()); }
        let cipher = self.cipher(true)?;
        let mut header = [0u8; HEADER];
        header[..8].copy_from_slice(MAGIC);
        SystemRandom::new().fill(&mut header[8..]).map_err(|_| "安全随机数不可用")?;
        let nonce: [u8; 12] = header[8..].try_into().unwrap();
        let mut content = Zeroizing::new(content.to_vec());
        cipher.seal_in_place_append_tag(aead::Nonce::assume_unique_for_key(nonce), aead::Aad::from(context), &mut *content).map_err(|_| "客户端备份加密失败")?;
        let mut output = header.to_vec();
        output.extend_from_slice(&content);
        Ok(output)
    }

    pub(crate) fn open(&self, file: &[u8], context: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
        if file.len() < HEADER + 16 || file.len() > MAX_FILE || !Self::encrypted(file) { return Err("客户端加密备份格式无效".into()); }
        let cipher = self.cipher(false)?;
        let nonce: [u8; 12] = file[8..HEADER].try_into().unwrap();
        let mut content = Zeroizing::new(file[HEADER..].to_vec());
        let length = cipher.open_in_place(aead::Nonce::assume_unique_for_key(nonce), aead::Aad::from(context), &mut content)
            .map_err(|_| "备份完整性验证失败，已保留备份且未覆盖客户端配置")?.len();
        content.truncate(length);
        Ok(content)
    }

    pub(crate) fn read(path: &Path) -> Result<Vec<u8>, String> {
        let metadata = fs::symlink_metadata(path).map_err(|_| "无法读取客户端备份")?;
        #[cfg(windows)]
        let redirected = {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let redirected = false;
        if !metadata.is_file() || metadata.file_type().is_symlink() || redirected || metadata.len() > MAX_FILE as u64 { return Err("客户端备份不是安全的普通文件或超过大小限制".into()); }
        fs::read(path).map_err(|_| "无法读取客户端备份".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn data_directories_use_distinct_system_key_ids() {
        let root = std::env::temp_dir().join(format!("xy-backup-scope-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("first")).unwrap();
        fs::create_dir_all(root.join("second")).unwrap();
        assert_ne!(backup_key_id(&root.join("first")).unwrap(), backup_key_id(&root.join("second")).unwrap());
        assert_eq!(backup_key_id(&root.join("first")).unwrap(), backup_key_id(&root.join("first/.")).unwrap());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn existing_encrypted_backups_prevent_rekeying() {
        let root = std::env::temp_dir().join(format!("xy-backup-rekey-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("legacy.bin"), b"synthetic-original").unwrap();
        assert!(!encrypted_backup_present(&root).unwrap());
        let cipher = BackupCipher::new(&root);
        fs::write(root.join("codex-auth.bin"), cipher.seal(b"synthetic-original", b"scope").unwrap()).unwrap();
        assert!(encrypted_backup_present(&root).unwrap());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn disk_envelope_hides_synthetic_credentials_and_binds_original_target() {
        let cipher = BackupCipher::new(Path::new("isolated-backup-fixture"));
        let source = b"{\"OPENAI_API_KEY\":\"synthetic-original-secret\",\"email\":\"private@example.test\"}";
        let sealed = cipher.seal(source, b"codex-auth/authorized-home/auth.json").unwrap();
        assert!(!String::from_utf8_lossy(&sealed).contains("synthetic-original-secret"));
        assert!(!String::from_utf8_lossy(&sealed).contains("private@example.test"));
        assert_eq!(cipher.open(&sealed, b"codex-auth/authorized-home/auth.json").unwrap().as_slice(), source);
        assert!(cipher.open(&sealed, b"codex-auth/other-home/auth.json").is_err());
        let other = BackupCipher { keys: Arc::new(TestKeys([0x48;32])), key_lock: PathBuf::new() };
        assert!(other.open(&sealed, b"codex-auth/authorized-home/auth.json").is_err());
        let mut corrupted = sealed;
        *corrupted.last_mut().unwrap() ^= 1;
        assert!(cipher.open(&corrupted, b"codex-auth/authorized-home/auth.json").is_err());
    }
}
