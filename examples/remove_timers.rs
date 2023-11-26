use void_ship::{remove_timer_mappings, test_clock};

fn main() {
    if let Err(e) = remove_timer_mappings() {
        eprintln!("Unable to remove timers. Error: {}", e);
    }

    // Read proc/self/maps and look for the vDSO and vvar mappings
    let map_file = std::fs::read("/proc/self/maps").unwrap();

    // Check for b"[vdso]" and b"[vvar]" in the file
    let vdso = map_file.windows(6).any(|window| window == b"[vdso]");
    let vvar = map_file.windows(6).any(|window| window == b"[vvar]");
    println!("vdso: {}, vvar: {}", vdso, vvar);

    if !vdso || !vvar {
        println!("Didn't find vdso, vvar - should now segfault.")
    }
    test_clock();
}
