use std::fmt::Formatter;
use std::io::Error as IoError;

use libc::*;
use libc::{mmap, MAP_ANONYMOUS, MAP_FIXED, MAP_PRIVATE, PROT_NONE};

#[derive(Debug)]
pub enum Error {
    IoError(IoError),
    InvalidFormat(&'static str),
    ParseIntError(std::num::ParseIntError),
}

impl From<IoError> for Error {
    fn from(e: IoError) -> Self {
        Error::IoError(e)
    }
}

impl From<std::num::ParseIntError> for Error {
    fn from(e: std::num::ParseIntError) -> Self {
        Error::ParseIntError(e)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::IoError(e) => Some(e),
            Error::InvalidFormat(_) => None,
            Error::ParseIntError(e) => Some(e),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::IoError(e) => write!(f, "IoError: {}", e),
            Error::InvalidFormat(s) => write!(f, "InvalidFormat: {}", s),
            Error::ParseIntError(e) => write!(f, "ParseIntError: {}", e),
        }
    }
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
#[cfg(target_os = "linux")]
#[allow(clippy::type_complexity)]
fn find_mapping_addresses() -> Result<
    (
        Option<(*mut libc::c_void, libc::size_t)>,
        Option<(*mut libc::c_void, libc::size_t)>,
    ),
    Error,
> {
    use std::fs::File;
    use std::io::{self, Read};

    let file = File::open("/proc/self/maps")?;
    let mut file = io::BufReader::new(file);
    let mut buffer = [0u8; 1024]; // Stack-allocated buffer
    let mut line = Vec::new(); // Collects a line

    let mut vdso_address = None;
    let mut vvar_address = None;

    while let Ok(bytes_read) = file.read(&mut buffer) {
        if bytes_read == 0 {
            break;
        }

        for &byte in &buffer[..bytes_read] {
            if byte == b'\n' {
                if line.windows(6).any(|window| window == b"[vdso]") {
                    vdso_address = Some(parse_address_and_size(&line)?);
                } else if line.windows(6).any(|window| window == b"[vvar]") {
                    vvar_address = Some(parse_address_and_size(&line)?);
                }
                line.clear();
            } else {
                line.push(byte);
            }
        }
    }

    Ok((vdso_address, vvar_address))
}

#[cfg(target_os = "linux")]
fn parse_address_and_size(line: &[u8]) -> Result<(*mut libc::c_void, libc::size_t), Error> {
    let mut parts = line
        .splitn(2, |&b| b == b' ')
        .next()
        .ok_or(Error::InvalidFormat("Invalid /proc/self/maps"))?
        .splitn(2, |&b| b == b'-');

    let start_str = parts
        .next()
        .ok_or(Error::InvalidFormat("Invalid /proc/self/maps"))?;
    let end_str = parts
        .next()
        .ok_or(Error::InvalidFormat("Invalid /proc/self/maps"))?;

    let start_address = parse_hex(start_str)?;
    let end_address = parse_hex(end_str)?;

    let size = end_address - start_address;
    Ok((start_address as *mut libc::c_void, size))
}

fn parse_hex(hex_slice: &[u8]) -> Result<usize, Error> {
    let mut num = 0;
    for &byte in hex_slice {
        num = num * 16
            + match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => 10 + byte - b'a',
                b'A'..=b'F' => 10 + byte - b'A',
                _ => return Err(Error::InvalidFormat("Invalid hexadecimal number")),
            } as usize;
    }
    Ok(num)
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
        return Err(Error::InvalidFormat("Could not find vdso or vvar mappings"));
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
        return Err(Error::InvalidFormat("Could not find vdso or vvar mappings"));
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
