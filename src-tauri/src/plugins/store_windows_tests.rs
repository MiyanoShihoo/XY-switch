use super::*;
use std::{
    ffi::c_void,
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    time::{SystemTime, UNIX_EPOCH},
};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn DeviceIoControl(
        device: *mut c_void,
        control_code: u32,
        input: *const c_void,
        input_size: u32,
        output: *mut c_void,
        output_size: u32,
        returned: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
}

struct Fixture {
    base: PathBuf,
    directory: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir();
        let directory = base.join(format!(
            "xy-store-tests-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        Self { base, directory }
    }

    fn packages(&self) -> PathBuf {
        self.directory
            .join("parent with spaces")
            .join("插件 packages")
    }
}

fn cleanup(root: &Path, path: &Path) {
    if !path.starts_with(root) {
        return;
    }
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_attributes() & 0x400 != 0 {
        if metadata.is_dir() {
            let _ = std::fs::remove_dir(path);
        } else {
            let _ = std::fs::remove_file(path);
        }
    } else if metadata.is_dir() {
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                cleanup(root, &entry.path());
            }
        }
        let _ = std::fs::remove_dir(path);
    } else if metadata.is_file() {
        let mut permissions = metadata.permissions();
        permissions.set_readonly(false);
        let _ = std::fs::set_permissions(path, permissions);
        let _ = std::fs::remove_file(path);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self.directory.parent() == Some(self.base.as_path()) {
            cleanup(&self.directory, &self.directory);
        }
    }
}

fn published() -> PublishedPlugin {
    let package: PluginPackage = serde_json::from_value(serde_json::json!({
        "manifest": {"schemaVersion": 1, "id": "test.store", "name": "Synthetic store test",
            "description": "A bounded local storage fixture.", "author": "Synthetic publisher",
            "version": "1.0.0", "minimumHostVersion": "0.0.0", "kind": "skill", "entry": "skill.md",
            "clients": ["codex-desktop"],
            "permissions": {"network": [], "readPaths": [], "writePaths": [], "process": false, "credentials": []},
            "dependencies": []},
        "files": {"skill.md": "# Synthetic skill\nUse approved local text.\n"}
    })).unwrap();
    PublishedPlugin {
        digest: super::super::validation::package_digest(&package).unwrap(),
        package,
        signature: "00".repeat(64),
        signing_key_id: "test.publisher".into(),
    }
}

fn immutable(file: &File) {
    let mut permissions = file.metadata().unwrap().permissions();
    permissions.set_readonly(true);
    file.set_permissions(permissions).unwrap();
    file.sync_all().unwrap();
}

fn junction_handle(path: &Path) -> File {
    OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(3)
        .custom_flags(0x02200000)
        .open(path)
        .unwrap()
}

fn set_junction(path: &Path, target: &Path) {
    let substitute: Vec<u16> = std::ffi::OsString::from(format!("\\??\\{}", target.display()))
        .encode_wide()
        .collect();
    let print: Vec<u16> = target.as_os_str().encode_wide().collect();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0xa0000003u32.to_le_bytes());
    bytes.extend_from_slice(&((8 + (substitute.len() + print.len() + 2) * 2) as u16).to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&((substitute.len() * 2) as u16).to_le_bytes());
    bytes.extend_from_slice(&(((substitute.len() + 1) * 2) as u16).to_le_bytes());
    bytes.extend_from_slice(&((print.len() * 2) as u16).to_le_bytes());
    for unit in substitute
        .into_iter()
        .chain(Some(0))
        .chain(print)
        .chain(Some(0))
    {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let handle = junction_handle(path);
    let mut returned = 0;
    let result = unsafe {
        DeviceIoControl(
            handle.as_raw_handle(),
            0x000900a4,
            bytes.as_ptr().cast(),
            bytes.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    assert_ne!(
        result,
        0,
        "synthetic junction setup: {:?}",
        std::io::Error::last_os_error()
    );
    assert_ne!(handle.metadata().unwrap().file_attributes() & 0x400, 0);
}

fn clear_junction(path: &Path) {
    let mut bytes = [0u8; 8];
    bytes[..4].copy_from_slice(&0xa0000003u32.to_le_bytes());
    let handle = junction_handle(path);
    let mut returned = 0;
    let result = unsafe {
        DeviceIoControl(
            handle.as_raw_handle(),
            0x000900ac,
            bytes.as_ptr().cast(),
            bytes.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    assert_ne!(
        result,
        0,
        "synthetic junction cleanup: {:?}",
        std::io::Error::last_os_error()
    );
}

#[test]
fn ordinary_disk_paths_publish_readonly_objects_and_keep_directory_handles_pinned() {
    let fixture = Fixture::new();
    let packages = fixture.packages();
    let store = PackageStore::open(packages.clone()).unwrap();
    let object = published();
    let original_identity = store.identity().unwrap();
    store.put(&object).unwrap();
    assert!(store.read(&object.digest).unwrap() == object);
    store.put(&object).unwrap();
    assert_eq!(std::fs::read_dir(&packages).unwrap().count(), 1);
    assert!(
        std::fs::metadata(packages.join(format!("{}.json", object.digest)))
            .unwrap()
            .permissions()
            .readonly()
    );
    assert!(std::fs::rename(&packages, fixture.directory.join("replaced-root")).is_err());
    assert!(
        std::fs::rename(
            packages.parent().unwrap(),
            fixture.directory.join("replaced-parent")
        )
        .is_err()
    );
    store.check_boundary().unwrap();
    assert_eq!(store.identity().unwrap(), original_identity);
}

#[test]
fn relative_publication_never_replaces_an_existing_object() {
    let fixture = Fixture::new();
    let packages = fixture.packages();
    let store = PackageStore::open(packages.clone()).unwrap();
    let stage = format!("stage-{}.tmp", uuid::Uuid::new_v4());
    let mut file = store.open_file(&stage, true).unwrap();
    file.write_all(b"Replacement synthetic contents.").unwrap();
    immutable(&file);
    let target = format!("{}.json", "a".repeat(64));
    std::fs::write(packages.join(&target), b"Original synthetic contents.").unwrap();
    let mut permissions = std::fs::metadata(packages.join(&target))
        .unwrap()
        .permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(packages.join(&target), permissions).unwrap();
    assert_eq!(
        store
            .publish_stage(&stage, &target, &file)
            .unwrap_err()
            .code,
        "plugin_immutable_version"
    );
    assert_eq!(
        std::fs::read(packages.join(&target)).unwrap(),
        b"Original synthetic contents."
    );
    drop(file);
    store.remove_stage(&stage).unwrap();
    assert_eq!(std::fs::read_dir(&packages).unwrap().count(), 1);
}

#[test]
fn relative_stage_cleanup_preserves_other_files() {
    let fixture = Fixture::new();
    let packages = fixture.packages();
    let store = PackageStore::open(packages.clone()).unwrap();
    let marker = fixture.directory.join("outside-marker.txt");
    std::fs::write(&marker, b"Unchanged synthetic marker.").unwrap();
    let stage = format!("stage-{}.tmp", uuid::Uuid::new_v4());
    let mut file = store.open_file(&stage, true).unwrap();
    file.write_all(b"Incomplete synthetic object.").unwrap();
    immutable(&file);
    drop(file);
    store.remove_stage(&stage).unwrap();
    assert_eq!(std::fs::read_dir(packages).unwrap().count(), 0);
    assert_eq!(
        std::fs::read(marker).unwrap(),
        b"Unchanged synthetic marker."
    );
}

#[test]
fn staged_handle_prevents_content_and_identity_replacement() {
    let fixture = Fixture::new();
    let packages = fixture.packages();
    let store = PackageStore::open(packages.clone()).unwrap();
    let stage = format!("stage-{}.tmp", uuid::Uuid::new_v4());
    let mut file = store.open_file(&stage, true).unwrap();
    let identity = windows_identity(&file).unwrap();
    file.write_all(b"Approved synthetic contents.").unwrap();
    assert!(
        std::fs::rename(
            packages.join(&stage),
            fixture.directory.join("replaced-stage")
        )
        .is_err()
    );
    assert!(
        OpenOptions::new()
            .write(true)
            .open(packages.join(&stage))
            .is_err()
    );
    immutable(&file);
    let target = format!("{}.json", "b".repeat(64));
    store.publish_stage(&stage, &target, &file).unwrap();
    assert_eq!(windows_identity(&file).unwrap(), identity);
    drop(file);
    let reopened = store.open_file(&target, false).unwrap();
    assert_eq!(windows_identity(&reopened).unwrap(), identity);
    PackageStore::regular_file(&reopened, true).unwrap();
}

#[test]
fn junction_ancestors_and_final_roots_are_rejected_without_external_writes() {
    let fixture = Fixture::new();
    let outside = fixture.directory.join("outside");
    let alias = fixture.directory.join("junction");
    std::fs::create_dir(&outside).unwrap();
    std::fs::create_dir(&alias).unwrap();
    set_junction(&alias, &outside);
    assert_eq!(
        PackageStore::open(alias.clone()).err().unwrap().code,
        "plugin_storage_boundary"
    );
    assert_eq!(
        PackageStore::open(alias.join("packages"))
            .err()
            .unwrap()
            .code,
        "plugin_storage_boundary"
    );
    assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
    clear_junction(&alias);
}

#[test]
fn changing_a_pinned_directory_to_a_junction_never_redirects_relative_io() {
    let fixture = Fixture::new();
    let packages = fixture.packages();
    let store = PackageStore::open(packages.clone()).unwrap();
    let outside = fixture.directory.join("outside");
    std::fs::create_dir(&outside).unwrap();
    let target = format!("{}.json", "c".repeat(64));
    let outside_target = outside.join(&target);
    std::fs::write(&outside_target, b"Outside synthetic contents.").unwrap();
    set_junction(&packages, &outside);
    assert_eq!(
        store.check_boundary().unwrap_err().code,
        "plugin_storage_boundary"
    );
    let directory = store.directories.last().unwrap();
    assert!(windows::open_file(directory, &target, false, false).is_err());
    let stage = format!("stage-{}.tmp", uuid::Uuid::new_v4());
    if let Ok(mut file) = windows::open_file(directory, &stage, true, false) {
        file.write_all(b"Inside synthetic contents.").unwrap();
        immutable(&file);
        let _ = windows::publish(directory, &file, &target);
        assert!(!outside.join(&stage).exists());
    }
    assert_eq!(
        std::fs::read(&outside_target).unwrap(),
        b"Outside synthetic contents."
    );
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1);
    clear_junction(&packages);
    store.check_boundary().unwrap();
}

#[test]
fn relative_rename_with_a_held_source_never_follows_a_changed_destination_junction() {
    let fixture = Fixture::new();
    let packages = fixture.packages();
    let store = PackageStore::open(packages.clone()).unwrap();
    let source = PackageStore::open(fixture.directory.join("staging")).unwrap();
    let stage = format!("stage-{}.tmp", uuid::Uuid::new_v4());
    let mut file = source.open_file(&stage, true).unwrap();
    file.write_all(b"Approved synthetic contents.").unwrap();
    immutable(&file);
    let identity = windows_identity(&file).unwrap();
    let outside = fixture.directory.join("outside");
    std::fs::create_dir(&outside).unwrap();
    let marker = outside.join("marker.txt");
    std::fs::write(&marker, b"Unchanged synthetic marker.").unwrap();
    let target = format!("{}.json", "d".repeat(64));
    set_junction(&packages, &outside);
    assert_eq!(
        store.check_boundary().unwrap_err().code,
        "plugin_storage_boundary"
    );
    let result = windows::publish(store.directories.last().unwrap(), &file, &target);
    assert_eq!(
        std::fs::read(&marker).unwrap(),
        b"Unchanged synthetic marker."
    );
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1);
    assert!(!outside.join(&target).exists());
    assert_eq!(windows_identity(&file).unwrap(), identity);
    clear_junction(&packages);
    store.check_boundary().unwrap();
    drop(file);
    if result.is_ok() {
        let reopened = store.open_file(&target, false).unwrap();
        assert_eq!(windows_identity(&reopened).unwrap(), identity);
        PackageStore::regular_file(&reopened, true).unwrap();
        assert!(!source.root.join(&stage).exists());
    } else {
        assert!(!packages.join(&target).exists());
        source.remove_stage(&stage).unwrap();
    }
}

#[test]
fn immutable_file_reads_reject_extra_hard_links_and_writable_content() {
    let fixture = Fixture::new();
    let packages = fixture.packages();
    let store = PackageStore::open(packages.clone()).unwrap();
    let object = published();
    store.put(&object).unwrap();
    let original = packages.join(format!("{}.json", object.digest));
    let alias = packages.join("object-alias.json");
    std::fs::hard_link(&original, &alias).unwrap();
    assert_eq!(
        store.read(&object.digest).err().unwrap().code,
        "plugin_storage_boundary"
    );
    let mut permissions = std::fs::metadata(&original).unwrap().permissions();
    permissions.set_readonly(false);
    std::fs::set_permissions(&original, permissions.clone()).unwrap();
    std::fs::remove_file(alias).unwrap();
    assert_eq!(
        store.read(&object.digest).err().unwrap().code,
        "plugin_storage_boundary"
    );
    permissions.set_readonly(true);
    std::fs::set_permissions(original, permissions).unwrap();
    assert!(store.read(&object.digest).unwrap() == object);
}
