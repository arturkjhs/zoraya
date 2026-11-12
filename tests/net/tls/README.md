# Test PKI for HTTPS

Every `.key` file here is **public and for tests only**. They are committed on
purpose, protect nothing and must never be trusted by a real system. The test
root (`root.pem`, valid until 2126) and `other-root.pem` carry the name
constraint `permitted;DNS:.zoraya`, so even a program that did trust them would
accept no certificate for a name outside `.zoraya`.

- `root.pem` goes into the test disk's `/system/tls/roots` by `make test-net-fetch`
  (Makefile), and nowhere else: never into `rootfs/`.
- `valid`, `expired` and `wrong` are leaves for `test.zoraya`, `expired.zoraya`
  and `other.zoraya` (served as `wrong.zoraya`).
- `outside` is a leaf for `www.example.org`, signed by the test root with its own key (`make-outside.sh`): the root's
  name constraint must make a verifier refuse it (`crates/zoraya-tls/tests/handshake.rs`).
- `make.sh` makes all of them again; run it only to renew them.
