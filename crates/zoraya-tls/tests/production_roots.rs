//! The roots the production image ships (ADR-0228 item 3) are Mozilla's
//! store and every block of it loads into the verifier: a root that webpki
//! cannot parse would be silently absent, and the shell would grant a store
//! smaller than the file says. `make check-tls-roots` checks the file on the
//! source; this loads it.

use zoraya_tls::{Config, pem_certificates};

zoraya_test_alloc::cap_memory!();

const ROOTS: &[u8] = include_bytes!("../../../rootfs/system/tls/roots");

fn no_date() -> Option<u64> {
    None
}

#[test]
fn every_production_root_loads_into_the_verifier() {
    let roots = pem_certificates(ROOTS);
    assert!(roots.len() >= 100, "only {} roots", roots.len());
    // Config::new keeps what parses and fails only when nothing does: each
    // root alone must be accepted.
    for (i, der) in roots.iter().enumerate() {
        assert!(
            Config::new(core::slice::from_ref(der), no_date).is_ok(),
            "root {i} does not parse"
        );
    }
    assert!(Config::new(&roots, no_date).is_ok());
}
