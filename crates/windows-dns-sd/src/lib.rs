#![cfg(windows)]

use std::{
    ffi::c_void,
    io,
    ptr::{from_ref, null_mut},
    sync::mpsc::{SendError, SyncSender, sync_channel},
};

use windows_sys::Win32::{
    Foundation::DNS_REQUEST_PENDING,
    NetworkManagement::Dns::{
        DNS_QUERY_REQUEST_VERSION1, DNS_SERVICE_INSTANCE, DNS_SERVICE_REGISTER_REQUEST,
        DnsServiceFreeInstance, DnsServiceRegister,
    },
    System::SystemInformation::{ComputerNameDnsHostname, GetComputerNameExW},
};

const DNS_LABEL_BYTES: usize = 63;

fn invalid_data() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "invalid DNS-SD service data")
}

fn wide(text: &str) -> io::Result<Vec<u16>> {
    if text.contains('\0') {
        return Err(invalid_data());
    }
    Ok(text.encode_utf16().chain([0]).collect())
}

#[expect(
    unsafe_code,
    reason = "Read the local DNS hostname through the Windows system API"
)]
fn hostname() -> io::Result<String> {
    let mut name = [0_u16; 256];
    let mut length = u32::try_from(name.len()).map_err(io::Error::other)?;
    // SAFETY: The buffer is writable and length holds its capacity in UTF-16 code units.
    if unsafe { GetComputerNameExW(ComputerNameDnsHostname, name.as_mut_ptr(), &raw mut length) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    let length = usize::try_from(length).map_err(io::Error::other)?;
    String::from_utf16(name.get(..length).ok_or_else(invalid_data)?).map_err(io::Error::other)
}

#[expect(
    unsafe_code,
    reason = "Consume the completion data defined by the Windows API"
)]
extern "system" fn complete(
    status: u32,
    context: *const c_void,
    instance: *const DNS_SERVICE_INSTANCE,
) {
    if !instance.is_null() {
        // SAFETY: Windows transfers ownership of each nonnull completion instance to the callback.
        unsafe { DnsServiceFreeInstance(instance) };
    }
    // SAFETY: The pending registration keeps the sender alive until the cloned sender reports completion.
    let sender = unsafe { &*context.cast::<SyncSender<u32>>() }.clone();
    match sender.send(status) {
        Ok(()) | Err(SendError(_)) => {}
    }
}

#[expect(
    unsafe_code,
    reason = "Register the borrowed instance and wait for the Windows callback"
)]
fn publish(instance: &mut DNS_SERVICE_INSTANCE) -> io::Result<()> {
    let (sender, result) = sync_channel::<u32>(1);
    let request = DNS_SERVICE_REGISTER_REQUEST {
        Version: DNS_QUERY_REQUEST_VERSION1,
        pServiceInstance: instance,
        pRegisterCompletionCallback: Some(complete),
        pQueryContext: from_ref(&sender).cast_mut().cast(),
        ..DNS_SERVICE_REGISTER_REQUEST::default()
    };
    // SAFETY: The request, sender, and instance buffers stay live until the single pending callback reports completion.
    let status = unsafe { DnsServiceRegister(&raw const request, null_mut()) };
    if status != DNS_REQUEST_PENDING.cast_unsigned() {
        return Err(io::Error::from_raw_os_error(status.cast_signed()));
    }
    let status = result.recv().map_err(io::Error::other)?;
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(status.cast_signed()))
    }
}

/// Registers a local service for the lifetime of this process.
/// Call from a worker thread since Windows reports the outcome asynchronously.
///
/// # Errors
/// Returns invalid service data or a Windows registration error.
pub fn register(label: &str, service_type: &str, port: u16, entries: &[String]) -> io::Result<()> {
    let host = hostname()?;
    let prefix = format!("{label} {port} on ");
    let available = DNS_LABEL_BYTES
        .checked_sub(prefix.len())
        .ok_or_else(invalid_data)?;
    let host_label = host
        .get(..host.floor_char_boundary(available))
        .ok_or_else(invalid_data)?;
    let mut name = wide(&format!("{prefix}{host_label}.{service_type}.local"))?;
    let mut hostname = wide(&format!("{host}.local"))?;
    let mut keys = Vec::new();
    let mut values = Vec::new();
    for entry in entries {
        let (key, value) = entry.split_once('=').ok_or_else(invalid_data)?;
        keys.push(wide(key)?);
        values.push(wide(value)?);
    }
    let mut key_pointers = keys.iter_mut().map(Vec::as_mut_ptr).collect::<Vec<_>>();
    let mut value_pointers = values.iter_mut().map(Vec::as_mut_ptr).collect::<Vec<_>>();
    let mut instance = DNS_SERVICE_INSTANCE {
        pszInstanceName: name.as_mut_ptr(),
        pszHostName: hostname.as_mut_ptr(),
        wPort: port,
        dwPropertyCount: u32::try_from(entries.len()).map_err(io::Error::other)?,
        keys: key_pointers.as_mut_ptr(),
        values: value_pointers.as_mut_ptr(),
        ..DNS_SERVICE_INSTANCE::default()
    };
    publish(&mut instance)
}
