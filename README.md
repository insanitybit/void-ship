### void-ship ⚫🚀

void-ship is a straightforward library to do one thing - remove the ability for a process to access the vDSO.

### vDSO? vvar?

To enable rapid access to the system clock **without** an expensive system call, Linux provides `vDSO` (Virtual Dynamic
Shared Object) and `vvar` mappings to user-space processes. These memory regions allow processes to access an accurate
and fast clock.

### Why remove access?

Accurate clocks are a fundamental primitive for side channel attacks. By removing the vDSO the process has to issue
a system call or otherwise "forge" a clock in order to get an accurate timer.

This library should be used alongside a seccomp filter to block access to the `clock_gettime` syscall as well
as a filter to prevent creating threads, allocating memory, or otherwise accessing primitives that an attacker
could use to create a clock. Consider a crate like [extrasafe](https://crates.io/crates/extrasafe) to help with this.

Note: This library will only work on Linux. On all other platforms it will simply do nothing and all
public functions return `Ok(())`.

### A Warning!

Manually unmapping the vDSO and vvar mappings is *weird* and will very likely cause things to break if you aren't
careful.
This library is intended to be used in a very specific context - a process that has an extremely restrictive seccomp
filter
applied to it that does virtually nothing but execute pure functions.

### Important Note
To further reduce access to a clock you should block access to `PR_SET_TSC` as it can be used to forge an accurate clock. In the future this may be added to the library in a minor version change.
```
extern crate libc;
fn main() {
    unsafe { libc::prctl(libc::PR_SET_TSC, libc::PR_TSC_SIGSEGV, 0, 0, 0) }.unwrap();
    unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) }.unwrap();
}
```
## Usage

`void-ship` provides two primary functions:

1. **remove_timer_mappings()**: Removes the vDSO and vvar mappings.
2. **replace_timer_mappings()**: Removes the mappings and sets up guard pages in their place.

## Example

```rust
use void_ship::{remove_timer_mappings, replace_timer_mappings};

fn main() {
    let should_replace = true;
    if should_replace {
        replace_timer_mappings().expect("Unable to replace timer mappings");
    } else {
        remove_timer_mappings().expect("Unable to remove timer mappings");
    }

    //  Attempting to get the system time via vDSO will now segfault.
}
```

### Testing

If you want to validate that the library is working as expected you can add the `test-clock` feature to the crate,
which exports the `test_clock` function.

Note that this function will either:

1. Segfault if the vDSO is removed (what you want)
2. Panic if the vDSO is not removed
3. Panic if the vDSO was supposedly removed but the `clock_gettime` syscall still works
4. Panic if executed on an unsupported platform

Basically, you never ever want to call this function if you aren't explicitly testing that this crate is
working properly.

```rust
use void_ship::{replace_timer_mappings, test_clock};

fn main() {
    replace_timer_mappings().expect("Unable to replace timer mappings");
    test_clock(); // will panic or segfault!!!
}
```
