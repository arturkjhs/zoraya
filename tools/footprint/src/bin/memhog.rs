//! Holds `MIB` MiB of zeros, every page touched, then sleeps: memory that
//! macOS compresses to almost nothing, so its resident size can stay small
//! while its footprint is the whole. For the watchdog's test.
//!
//! Usage: `zoraya-memhog MIB`

fn main() {
    let mib: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(2048);
    let mut held: Vec<Vec<u8>> = Vec::new();
    for _ in 0..mib {
        // One MiB, written page by page so that every page is the process's.
        let mut chunk = vec![0u8; 1 << 20];
        for page in chunk.chunks_mut(4096) {
            // SAFETY: a write through a valid `&mut u8`; volatile, so the
            // compiler keeps the store that dirties the page.
            unsafe { std::ptr::write_volatile(&mut page[0], 0) };
        }
        held.push(chunk);
    }
    eprintln!("zoraya-memhog: holding {mib} MiB");
    std::thread::sleep(std::time::Duration::from_secs(300));
    drop(held);
}
