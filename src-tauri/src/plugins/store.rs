use super::{
    types::*,
    validation::{parse_published_json, validate_hex},
};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

#[cfg(windows)]
#[path = "store_windows.rs"]
mod windows;

#[cfg(all(windows, test))]
#[path = "store_windows_tests.rs"]
mod windows_tests;

fn error(code: &'static str) -> PluginError {
    PluginError::new(code)
}

fn linked(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn absolute_boundary(path: &Path) -> PluginResult<()> {
    if !path.is_absolute()
        || path
            .components()
            .filter(|component| matches!(component, Component::Normal(_)))
            .count()
            == 0
    {
        return Err(error("plugin_storage_boundary"));
    }
    for component in path.components() {
        if matches!(component, Component::CurDir | Component::ParentDir) {
            return Err(error("plugin_storage_boundary"));
        }
        #[cfg(windows)]
        if let Component::Prefix(prefix) = component {
            if !matches!(prefix.kind(), std::path::Prefix::Disk(_)) {
                return Err(error("plugin_storage_boundary"));
            }
        }
    }
    Ok(())
}

pub(crate) struct PackageStore {
    root: PathBuf,
    #[cfg(unix)]
    directory: File,
    #[cfg(windows)]
    directories: Vec<File>,
}

#[cfg(unix)]
fn unix_root(path: &Path, create: bool) -> PluginResult<File> {
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    };
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut directory = options
        .open("/")
        .map_err(|_| error("plugin_storage_boundary"))?;
    for component in path.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        let name = std::ffi::CString::new(name.as_bytes())
            .map_err(|_| error("plugin_storage_boundary"))?;
        if create {
            let result = unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) };
            if result != 0
                && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
            {
                return Err(error("plugin_storage"));
            }
        }
        let descriptor = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(error("plugin_storage_boundary"));
        }
        directory = unsafe { File::from_raw_fd(descriptor) };
        let metadata = directory.metadata().map_err(|_| error("plugin_storage"))?;
        if !metadata.is_dir() || linked(&metadata) {
            return Err(error("plugin_storage_boundary"));
        }
    }
    Ok(directory)
}

#[cfg(windows)]
fn windows_root(path: &Path, create: bool) -> PluginResult<Vec<File>> {
    use std::os::windows::fs::OpenOptionsExt;
    let mut current = PathBuf::new();
    let mut directories = Vec::new();
    for (index, component) in path.components().enumerate() {
        current.push(component.as_os_str());
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        if create && matches!(component, Component::Normal(_)) {
            match std::fs::create_dir(&current) {
                Ok(()) => {}
                Err(failure) if failure.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(error("plugin_storage")),
            }
        }
        let sharing = if index + 1 == path.components().count() {
            0x3
        } else {
            0x1
        };
        let directory = OpenOptions::new()
            .read(true)
            .share_mode(sharing)
            .custom_flags(0x02000000 | 0x00200000)
            .open(&current)
            .map_err(|_| error("plugin_storage_boundary"))?;
        let metadata = directory.metadata().map_err(|_| error("plugin_storage"))?;
        if !metadata.is_dir() || linked(&metadata) {
            return Err(error("plugin_storage_boundary"));
        }
        directories.push(directory);
    }
    if directories.is_empty() {
        return Err(error("plugin_storage_boundary"));
    }
    Ok(directories)
}

impl PackageStore {
    pub fn open(root: PathBuf) -> PluginResult<Self> {
        absolute_boundary(&root)?;
        #[cfg(unix)]
        {
            let directory = unix_root(&root, true)?;
            Ok(Self { root, directory })
        }
        #[cfg(windows)]
        {
            let directories = windows_root(&root, true)?;
            Ok(Self { root, directories })
        }
        #[cfg(not(any(unix, windows)))]
        {
            Err(error("plugin_storage_boundary"))
        }
    }

    pub fn identity(&self) -> PluginResult<String> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = self
                .directory
                .metadata()
                .map_err(|_| error("plugin_storage_boundary"))?;
            Ok(super::validation::sha256_hex(
                format!(
                    "{}\n{}\n{}",
                    self.root.display(),
                    metadata.dev(),
                    metadata.ino()
                )
                .as_bytes(),
            ))
        }
        #[cfg(windows)]
        {
            let directory = self
                .directories
                .last()
                .ok_or_else(|| error("plugin_storage_boundary"))?;
            let identity = windows_identity(directory)?;
            Ok(super::validation::sha256_hex(
                format!("{}\n{}\n{}", self.root.display(), identity.0, identity.1).as_bytes(),
            ))
        }
        #[cfg(not(any(unix, windows)))]
        {
            Err(error("plugin_storage_boundary"))
        }
    }

    pub fn check_boundary(&self) -> PluginResult<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let fresh = unix_root(&self.root, false)?
                .metadata()
                .map_err(|_| error("plugin_storage"))?;
            let original = self
                .directory
                .metadata()
                .map_err(|_| error("plugin_storage"))?;
            if fresh.dev() != original.dev() || fresh.ino() != original.ino() {
                return Err(error("plugin_storage_boundary"));
            }
        }
        #[cfg(windows)]
        {
            let fresh = windows_root(&self.root, false)?;
            if fresh.len() != self.directories.len() {
                return Err(error("plugin_storage_boundary"));
            }
            for (directory, current) in self.directories.iter().zip(&fresh) {
                if linked(
                    &directory
                        .metadata()
                        .map_err(|_| error("plugin_storage_boundary"))?,
                ) || windows_identity(directory)? != windows_identity(current)?
                {
                    return Err(error("plugin_storage_boundary"));
                }
            }
        }
        Ok(())
    }

    fn open_file(&self, name: &str, create: bool) -> PluginResult<File> {
        self.check_boundary()?;
        if name.contains('/') || name.contains('\\') || name.contains(':') {
            return Err(error("plugin_storage_boundary"));
        }
        #[cfg(unix)]
        {
            use std::os::fd::{AsRawFd, FromRawFd};
            let name =
                std::ffi::CString::new(name).map_err(|_| error("plugin_storage_boundary"))?;
            let flags = libc::O_NOFOLLOW
                | libc::O_CLOEXEC
                | libc::O_NONBLOCK
                | if create {
                    libc::O_RDWR | libc::O_CREAT | libc::O_EXCL
                } else {
                    libc::O_RDONLY
                };
            let descriptor =
                unsafe { libc::openat(self.directory.as_raw_fd(), name.as_ptr(), flags, 0o600) };
            if descriptor < 0 {
                return Err(error("plugin_storage"));
            }
            Ok(unsafe { File::from_raw_fd(descriptor) })
        }
        #[cfg(windows)]
        {
            let directory = self
                .directories
                .last()
                .ok_or_else(|| error("plugin_storage_boundary"))?;
            windows::open_file(directory, name, create, false)
        }
        #[cfg(not(any(unix, windows)))]
        {
            Err(error("plugin_storage_boundary"))
        }
    }

    fn regular_file(file: &File, immutable: bool) -> PluginResult<()> {
        let metadata = file.metadata().map_err(|_| error("plugin_storage"))?;
        if !metadata.is_file()
            || linked(&metadata)
            || metadata.len() > (MAX_PACKAGE_BYTES + 2048) as u64
            || immutable && !metadata.permissions().readonly()
        {
            return Err(error("plugin_storage_boundary"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.nlink() != 1 {
                return Err(error("plugin_storage_boundary"));
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::{
                BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
            };
            let mut information = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
            if unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) }
                == 0
                || unsafe { information.assume_init() }.nNumberOfLinks != 1
            {
                return Err(error("plugin_storage_boundary"));
            }
        }
        Ok(())
    }

    pub fn read(&self, digest: &str) -> PluginResult<PublishedPlugin> {
        validate_hex(digest, 32).map_err(|_| error("plugin_digest"))?;
        let file = self.open_file(&format!("{digest}.json"), false)?;
        Self::regular_file(&file, true)?;
        let mut bytes = Vec::new();
        file.take((MAX_PACKAGE_BYTES + 2049) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| error("plugin_storage"))?;
        if bytes.len() > MAX_PACKAGE_BYTES + 2048 {
            return Err(error("plugin_package_size"));
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| error("plugin_schema"))?;
        let published = parse_published_json(text)?;
        if published.digest != digest {
            return Err(error("plugin_digest"));
        }
        Ok(published)
    }

    fn remove_stage(&self, name: &str) -> PluginResult<()> {
        self.check_boundary()?;
        if !name.starts_with("stage-") || !name.ends_with(".tmp") || name.len() != 46 {
            return Err(error("plugin_storage_boundary"));
        }
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let name =
                std::ffi::CString::new(name).map_err(|_| error("plugin_storage_boundary"))?;
            if unsafe { libc::unlinkat(self.directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
                return Err(error("plugin_storage"));
            }
        }
        #[cfg(windows)]
        {
            let directory = self
                .directories
                .last()
                .ok_or_else(|| error("plugin_storage_boundary"))?;
            let file = windows::open_file(directory, name, false, true)?;
            Self::regular_file(&file, false)?;
            let mut permissions = file
                .metadata()
                .map_err(|_| error("plugin_storage"))?
                .permissions();
            permissions.set_readonly(false);
            file.set_permissions(permissions)
                .map_err(|_| error("plugin_storage"))?;
            windows::remove(&file)?;
        }
        Ok(())
    }

    fn publish_stage(&self, stage: &str, target: &str, file: &File) -> PluginResult<()> {
        self.check_boundary()?;
        Self::regular_file(file, true)?;
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let source =
                std::ffi::CString::new(stage).map_err(|_| error("plugin_storage_boundary"))?;
            let destination =
                std::ffi::CString::new(target).map_err(|_| error("plugin_storage_boundary"))?;
            if unsafe {
                libc::linkat(
                    self.directory.as_raw_fd(),
                    source.as_ptr(),
                    self.directory.as_raw_fd(),
                    destination.as_ptr(),
                    0,
                )
            } != 0
            {
                return Err(error("plugin_immutable_version"));
            }
            self.remove_stage(stage)?;
            self.directory
                .sync_all()
                .map_err(|_| error("plugin_storage"))?;
        }
        #[cfg(windows)]
        {
            let _ = stage;
            let directory = self
                .directories
                .last()
                .ok_or_else(|| error("plugin_storage_boundary"))?;
            windows::publish(directory, file, target)?;
        }
        Ok(())
    }

    pub fn put(&self, published: &PublishedPlugin) -> PluginResult<()> {
        validate_hex(&published.digest, 32).map_err(|_| error("plugin_digest"))?;
        let target = format!("{}.json", published.digest);
        if self.open_file(&target, false).is_ok() {
            if self.read(&published.digest)? != *published {
                return Err(error("plugin_immutable_version"));
            }
            return Ok(());
        }
        let stage = format!("stage-{}.tmp", uuid::Uuid::new_v4());
        let result = (|| {
            let mut file = self.open_file(&stage, true)?;
            Self::regular_file(&file, false)?;
            let bytes = serde_json::to_vec(published).map_err(|_| error("plugin_schema"))?;
            if bytes.len() > MAX_PACKAGE_BYTES + 2048 {
                return Err(error("plugin_package_size"));
            }
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| error("plugin_storage"))?;
            let mut permissions = file
                .metadata()
                .map_err(|_| error("plugin_storage"))?
                .permissions();
            permissions.set_readonly(true);
            file.set_permissions(permissions)
                .map_err(|_| error("plugin_storage"))?;
            file.sync_all().map_err(|_| error("plugin_storage"))?;
            self.publish_stage(&stage, &target, &file)?;
            drop(file);
            if self.read(&published.digest)? != *published {
                return Err(error("plugin_immutable_version"));
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = self.remove_stage(&stage);
        }
        result
    }
}

#[cfg(windows)]
fn windows_identity(file: &File) -> PluginResult<(u32, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let mut information = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) } == 0 {
        return Err(error("plugin_storage_boundary"));
    }
    let information = unsafe { information.assume_init() };
    Ok((
        information.dwVolumeSerialNumber,
        (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
    ))
}
