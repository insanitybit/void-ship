use core::ffi::CStr;
use core::fmt::Formatter;

#[cfg(target_os = "linux")]
use libc::*;

#[derive(Debug)]
pub enum Error {
    IoError(&'static str, c_int),
    InvalidFormat(&'static str),
    ParseIntError(core::num::ParseIntError),
}

impl From<core::num::ParseIntError> for Error {
    fn from(e: core::num::ParseIntError) -> Self {
        Error::ParseIntError(e)
    }
}

// When `error_in_core` lands this can be made `core::error::Error`
//  see issue #103765 https://github.com/rust-lang/rust/issues/103765
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::IoError(_, _) => None,
            Error::InvalidFormat(_) => None,
            Error::ParseIntError(e) => Some(e),
        }
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::IoError(s, e) => write!(f, "IoError: {} {}", s, e),
            Error::InvalidFormat(s) => write!(f, "InvalidFormat: {}", s),
            Error::ParseIntError(e) => write!(f, "ParseIntError: {}", e),
        }
    }
}

// Function to unmap a memory region
#[cfg(target_os = "linux")]
unsafe fn unmap_region(address: *mut c_void, size: size_t) -> Result<(), Error> {
    let errno = munmap(address, size);
    if errno == 0 {
        Ok(())
    } else {
        Err(Error::IoError("munmap", errno))
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
// let path = unsafe {
// // SAFETY: This is a valid, static C string
// CStr::from_bytes_until_nul(b"/proc/self/maps\x00").unwrap_unchecked()
// };
#[cfg(target_os = "linux")]
fn find_mapping_addresses() -> Result<
    (
        Option<(*mut libc::c_void, libc::size_t)>,
        Option<(*mut libc::c_void, libc::size_t)>,
    ),
    Error,
> {
    let path = unsafe {
        // SAFETY: This is a valid, static C string
        CStr::from_bytes_until_nul(b"/proc/self/maps\x00").unwrap_unchecked()
    };
    let fd = unsafe { open(path.as_ptr(), O_RDONLY) };
    if fd < 0 {
        return Err(Error::IoError("open", fd));
    }

    // One page size seems appropriate, especially since even a small /proc/self/maps
    // is typically > 2048 bytes
    let mut buffer = [0u8; 4096];
    // `line` should be at least 80 bytes, in order to hold the full `vdso` or `vvar` lines,
    // but we can make it larger in case there's every something odd about it
    let mut line = [0u8; 1024];
    let mut line_idx = 0;
    let mut vvar = None;
    let mut vdso = None;

    loop {
        let bytes_read =
            unsafe { read(fd, buffer.as_mut_ptr() as *mut libc::c_void, buffer.len()) };
        if bytes_read <= 0 {
            break; // EOF or error
        }

        for &byte in &buffer[..bytes_read as usize] {
            if byte == b'\n' {
                if line.windows(6).any(|window| window == b"[vdso]") {
                    vdso = Some(parse_addresses(&line[..12], &line[13..25])?);
                } else if line.windows(6).any(|window| window == b"[vvar]") {
                    vvar = Some(parse_addresses(&line[..12], &line[13..25])?);
                }
                line_idx = 0; // Reset for the next line
            } else {
                if line_idx < line.len() {
                    line[line_idx] = byte;
                    line_idx += 1;
                }
            }
        }
    }

    unsafe { close(fd) };
    Ok((vvar, vdso))
}

fn parse_addresses(
    start_addr: &[u8],
    end_addr: &[u8],
) -> Result<(*mut libc::c_void, libc::size_t), Error> {
    let start = parse_hex_address(start_addr)?;
    let end = parse_hex_address(end_addr)?;

    Ok((start as *mut libc::c_void, end - start))
}

fn parse_hex_address(addr: &[u8]) -> Result<usize, Error> {
    let mut num = 0;
    for &byte in addr {
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
        Err(Error::IoError("mmap", result as c_int))
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
