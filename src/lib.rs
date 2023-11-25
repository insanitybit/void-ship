use libc::*;
use libc::{mmap, MAP_ANONYMOUS, MAP_FIXED, MAP_PRIVATE, PROT_NONE};
use std::fs;
use std::io::Error as IoError;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("io error")]
    IoError(#[from] IoError),
    #[error("Invalid format: expected two parts separated by '-'. Line: {0}")]
    InvalidFormat(String),

    #[error("Failed to parse address: {0}")]
    ParseIntError(#[from] std::num::ParseIntError),
}

// Function to unmap a memory region
#[cfg(target_os = "linux")]
unsafe fn unmap_region(address: *mut c_void, size: size_t) -> Result<(), IoError> {
    if munmap(address, size) == 0 {
        Ok(())
    } else {
        Err(IoError::last_os_error())
    }
}

/// A helper to validate that vdso has been blocked, you should *never* call this
/// unless you're just validating that this crate works in your environment.
/// NOTE: This *will* cause a SIGSEGV if the vdso is blocked!!
/// Panics if the vdso is not blocked
#[cfg(feature = "test-clock")]
#[cfg(target_os = "linux")]
pub fn test_clock() -> ! {
    let mut ts = timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };

    let result = unsafe { clock_gettime(CLOCK_MONOTONIC, &mut ts) };

    if result == 0 {
        panic!("clock_gettime succeeded when it should have failed");
    } else {
        panic!("clock_gettime failed as expected, but code continued for some reason!");
    }
}

#[cfg(feature = "test-clock")]
#[cfg(not(target_os = "linux"))]
pub fn test_clock() -> ! {
    panic!("test_clock is only available on linux");
}

// This is used internally, exclusively, so I don't feel the need to refactor the return type
#[allow(clippy::type_complexity)]
#[cfg(target_os = "linux")]
fn find_mapping_addresses(
) -> Result<(Option<(*mut c_void, size_t)>, Option<(*mut c_void, size_t)>), Error> {
    let maps = fs::read_to_string("/proc/self/maps")?;
    let mut vdso_address = None;
    let mut vvar_address = None;

    for line in maps.lines() {
        if line.contains("[vdso]") {
            vdso_address = Some(parse_address_and_size(line)?);
        } else if line.contains("[vvar]") {
            vvar_address = Some(parse_address_and_size(line)?);
        }
    }

    Ok((vdso_address, vvar_address))
}

#[cfg(target_os = "linux")]
fn parse_address_and_size(line: &str) -> Result<(*mut c_void, size_t), Error> {
    // Safely attempt to split the line and collect parts
    let parts: Vec<&str> = line
        .split_whitespace()
        .next()
        .ok_or_else(|| Error::InvalidFormat(line.to_string()))?
        .split('-')
        .collect();

    // Ensure there are exactly two parts
    if parts.len() != 2 {
        return Err(Error::InvalidFormat(line.to_string()));
    }

    // Parse the addresses
    let start_address = usize::from_str_radix(parts[0], 16)?;
    let end_address = usize::from_str_radix(parts[1], 16)?;

    // Calculate size and convert start_address to a pointer
    let size = end_address - start_address;
    Ok((start_address as *mut c_void, size))
}

#[cfg(target_os = "linux")]
fn allocate_guard_page(address: *mut c_void, size: size_t) -> Result<(), Error> {
    let result = unsafe {
        mmap(
            address,
            size,
            PROT_NONE,
            MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED,
            -1,
            0,
        )
    };

    if result == libc::MAP_FAILED {
        Err(IoError::last_os_error().into())
    } else {
        Ok(())
    }
}

/// Unmaps the vdso and vvar mappings.
#[cfg(target_os = "linux")]
pub fn remove_timer_mappings() -> Result<(), Error> {
    let (Some((vdso_address, vdso_size)), Some((vvar_address, vvar_size))) =
        find_mapping_addresses()?
        else {
            return Err(Error::InvalidFormat(
                "Could not find vdso or vvar mappings".to_string(),
            ));
        };
    // Assuming the regions are at least one page in size
    unsafe {
        unmap_region(vdso_address, vdso_size)?;
        unmap_region(vvar_address, vvar_size)?;
    }
    Ok(())
}

/// This function will replace the vdso and vvar mappings with guard pages.
#[cfg(target_os = "linux")]
pub fn replace_timer_mappings() -> Result<(), Error> {
    let (Some((vdso_address, vdso_size)), Some((vvar_address, vvar_size))) =
        find_mapping_addresses()?
    else {
        return Err(Error::InvalidFormat(
            "Could not find vdso or vvar mappings".to_string(),
        ));
    };
    // Assuming the regions are at least one page in size
    unsafe {
        unmap_region(vdso_address, vdso_size)?;
        unmap_region(vvar_address, vvar_size)?;
    }

    allocate_guard_page(vdso_address, vdso_size)?;
    allocate_guard_page(vvar_address, vvar_size)?;

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn remove_timer_mappings() -> Result<(), Error> {
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn replace_timer_mappings() -> Result<(), Error> {
    Ok(())
}