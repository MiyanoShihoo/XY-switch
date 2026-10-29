use super::crypto::MAX_FILE;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

pub fn read(path: &Path) -> Result<Vec<u8>, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| "Cannot inspect encrypted sync file")?;
    if !meta.is_file() || crate::skills_local::is_link(&meta) || meta.len() > MAX_FILE as u64 {
        return Err(
            "Select a regular encrypted sync file within the size limit, not a link".into(),
        );
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT: inspect, never follow the final link.
    }
    let file = options
        .open(path)
        .map_err(|_| "Cannot open encrypted sync file")?;
    let meta = file
        .metadata()
        .map_err(|_| "Cannot inspect opened sync file")?;
    if !meta.is_file() || crate::skills_local::is_link(&meta) || meta.len() > MAX_FILE as u64 {
        return Err("Opened sync file is not a bounded regular file".into());
    }
    let mut data = Vec::new();
    file.take((MAX_FILE + 1) as u64)
        .read_to_end(&mut data)
        .map_err(|_| "Cannot read encrypted sync file")?;
    if data.len() > MAX_FILE {
        return Err("Encrypted sync file exceeds the size limit".into());
    }
    Ok(data)
}

pub fn write_new(path: &Path, data: &[u8]) -> Result<(), String> {
    if data.len() > MAX_FILE {
        return Err("Encrypted sync file exceeds the size limit".into());
    }
    if path.extension().and_then(|v| v.to_str()) != Some("xysync") {
        return Err("Use the .xysync file extension".into());
    }
    let mut options = File::options();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(
        |_| "Cannot create sync file; choose a new name (existing files are never overwritten)",
    )?;
    file.write_all(data)
        .and_then(|_| file.sync_all())
        .map_err(|_| "Encrypted file write failed; an incomplete encrypted file may remain".into())
}
