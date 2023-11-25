use void_ship::remove_timer_mappings;

fn main() {
    if let Err(e) = remove_timer_mappings() {
        eprintln!("Unable to remove timers. Error: {}", e);
    }
}
