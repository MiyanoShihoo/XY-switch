use super::{PluginResult, error};
use std::{
    ffi::c_void,
    fs::File,
    os::windows::io::{AsRawHandle, FromRawHandle},
};

type Handle = *mut c_void;

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *const u16,
}

#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root_directory: Handle,
    object_name: *const UnicodeString,
    attributes: u32,
    security_descriptor: *mut c_void,
    security_quality_of_service: *mut c_void,
}

#[repr(C)]
struct IoStatusBlock {
    status: isize,
    information: usize,
}

#[repr(C)]
struct RenameInformation {
    replace_if_exists: u8,
    root_directory: Handle,
    file_name_length: u32,
    file_name: [u16; 1],
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateFile(
        file_handle: *mut Handle,
        desired_access: u32,
        object_attributes: *const ObjectAttributes,
        io_status_block: *mut IoStatusBlock,
        allocation_size: *const i64,
        file_attributes: u32,
        share_access: u32,
        create_disposition: u32,
        create_options: u32,
        ea_buffer: *const c_void,
        ea_length: u32,
    ) -> i32;

    fn NtSetInformationFile(
        file_handle: Handle,
        io_status_block: *mut IoStatusBlock,
        file_information: *const c_void,
        length: u32,
        file_information_class: u32,
    ) -> i32;
}

fn name_units(name: &str) -> PluginResult<Vec<u16>> {
    if name.is_empty()
        || name.len() > 96
        || name == "."
        || name == ".."
        || name.ends_with(['.', ' '])
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(error("plugin_storage_boundary"));
    }
    Ok(name.encode_utf16().collect())
}

fn status_error(status: i32) -> super::PluginError {
    match status as u32 {
        0xc000050b | 0xc0000280 => error("plugin_storage_boundary"),
        _ => error("plugin_storage"),
    }
}

pub(super) fn open_file(
    directory: &File,
    name: &str,
    create: bool,
    remove: bool,
) -> PluginResult<File> {
    let units = name_units(name)?;
    let object_name = UnicodeString {
        length: (units.len() * 2) as u16,
        maximum_length: (units.len() * 2) as u16,
        buffer: units.as_ptr(),
    };
    let attributes = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root_directory: directory.as_raw_handle(),
        object_name: &object_name,
        attributes: 0x40 | 0x1000,
        security_descriptor: std::ptr::null_mut(),
        security_quality_of_service: std::ptr::null_mut(),
    };
    let mut io_status = IoStatusBlock {
        status: 0,
        information: 0,
    };
    let mut handle = std::ptr::null_mut();
    let desired_access = 0x00120089
        | if create { 0x00120116 | 0x00010000 } else { 0 }
        | if remove { 0x00010000 | 0x00000100 } else { 0 };
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            desired_access,
            &attributes,
            &mut io_status,
            std::ptr::null(),
            0x80,
            0x1,
            if create { 2 } else { 1 },
            0x40 | 0x20 | 0x00200000,
            std::ptr::null(),
            0,
        )
    };
    if status != 0 || handle.is_null() || handle as isize == -1 {
        return Err(status_error(status));
    }
    Ok(unsafe { File::from_raw_handle(handle) })
}

pub(super) fn publish(directory: &File, file: &File, target: &str) -> PluginResult<()> {
    let units = name_units(target)?;
    let offset = std::mem::offset_of!(RenameInformation, file_name);
    let bytes = offset + units.len() * 2;
    let allocated = bytes
        .max(std::mem::size_of::<RenameInformation>())
        .div_ceil(std::mem::size_of::<usize>());
    let mut buffer = vec![0usize; allocated];
    let information = buffer.as_mut_ptr().cast::<RenameInformation>();
    unsafe {
        std::ptr::addr_of_mut!((*information).replace_if_exists).write(0);
        std::ptr::addr_of_mut!((*information).root_directory).write(directory.as_raw_handle());
        std::ptr::addr_of_mut!((*information).file_name_length).write((units.len() * 2) as u32);
        std::ptr::copy_nonoverlapping(
            units.as_ptr(),
            buffer.as_mut_ptr().cast::<u8>().add(offset).cast::<u16>(),
            units.len(),
        );
    }
    let mut io_status = IoStatusBlock {
        status: 0,
        information: 0,
    };
    let status = unsafe {
        NtSetInformationFile(
            file.as_raw_handle(),
            &mut io_status,
            buffer.as_ptr().cast(),
            bytes as u32,
            10,
        )
    };
    if status as u32 == 0xc0000035 {
        return Err(error("plugin_immutable_version"));
    }
    if status != 0 {
        return Err(status_error(status));
    }
    file.sync_all().map_err(|_| error("plugin_storage"))
}

pub(super) fn remove(file: &File) -> PluginResult<()> {
    let disposition: u8 = 1;
    let mut io_status = IoStatusBlock {
        status: 0,
        information: 0,
    };
    let status = unsafe {
        NtSetInformationFile(
            file.as_raw_handle(),
            &mut io_status,
            std::ptr::from_ref(&disposition).cast(),
            1,
            13,
        )
    };
    if status != 0 {
        return Err(status_error(status));
    }
    Ok(())
}
