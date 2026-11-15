use std::io;

#[cfg(any(test, all(debug_assertions, desktop)))]
fn isolated_identifier(identifier: &str, data_dir: &std::path::Path) -> io::Result<String> {
    use sha2::{Digest, Sha256};

    let canonical = data_dir.canonicalize()?;
    let scope = canonical.to_string_lossy();
    #[cfg(windows)]
    let scope = scope.to_lowercase();
    let digest = Sha256::digest(scope.as_bytes());
    Ok(format!("{identifier}.test.{digest:x}"))
}

pub fn prepare_context(_context: &mut tauri::Context<tauri::Wry>) -> io::Result<()> {
    #[cfg(all(debug_assertions, desktop))]
    if let Some(data_dir) = crate::debug_path_override("XY_SWITCH_TEST_DATA_DIR") {
        std::fs::create_dir_all(&data_dir)?;
        let identifier = isolated_identifier(&_context.config().identifier, &data_dir)?;
        _context.config_mut().identifier = identifier;
    }
    #[cfg(windows)]
    set_windows_identity(&_context.config().identifier)?;
    Ok(())
}

#[cfg(windows)]
fn set_windows_identity(identifier: &str) -> io::Result<()> {
    use windows_sys::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{
            GetCurrentProcessExplicitAppUserModelID, SetCurrentProcessExplicitAppUserModelID,
        },
    };

    let expected: Vec<u16> = identifier.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let result = SetCurrentProcessExplicitAppUserModelID(expected.as_ptr());
        if result < 0 {
            return Err(io::Error::other(format!(
                "application identity: {result:#x}"
            )));
        }
        let mut actual = std::ptr::null_mut();
        let result = GetCurrentProcessExplicitAppUserModelID(&mut actual);
        if result < 0 || actual.is_null() {
            return Err(io::Error::other(format!(
                "application identity verification: {result:#x}"
            )));
        }
        let mut length = 0;
        while *actual.add(length) != 0 {
            length += 1;
        }
        let matches = std::slice::from_raw_parts(actual, length) == &expected[..expected.len() - 1];
        CoTaskMemFree(actual.cast());
        if !matches {
            return Err(io::Error::other("application identity mismatch"));
        }
    }
    eprintln!("XY Switch application identity verified: {identifier}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::isolated_identifier;
    use std::path::Path;

    const IDENTIFIER: &str = "com.xyswitch.desktop";

    #[test]
    fn path_aliases_share_one_instance_scope() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(
            isolated_identifier(IDENTIFIER, root).unwrap(),
            isolated_identifier(IDENTIFIER, &root.join("src/..")).unwrap()
        );
    }

    #[test]
    fn isolated_directories_have_distinct_nonproduction_identities() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let first = isolated_identifier(IDENTIFIER, root).unwrap();
        let second = isolated_identifier(IDENTIFIER, &root.join("src")).unwrap();
        assert_ne!(first, second);
        assert_ne!(first, IDENTIFIER);
        assert!(first.len() < 128);
    }

    #[test]
    fn missing_scope_never_falls_back_to_production() {
        let missing = Path::new(env!("CARGO_MANIFEST_DIR")).join(uuid::Uuid::new_v4().to_string());
        assert!(isolated_identifier(IDENTIFIER, &missing).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_case_aliases_share_one_instance_scope() {
        let upper = env!("CARGO_MANIFEST_DIR").to_uppercase();
        let lower = env!("CARGO_MANIFEST_DIR").to_lowercase();
        assert_eq!(
            isolated_identifier(IDENTIFIER, Path::new(&upper)).unwrap(),
            isolated_identifier(IDENTIFIER, Path::new(&lower)).unwrap()
        );
    }
}
