# Test PKI for git's HTTPS test

Every `.key` file here is **public and for tests only**, like the ones in `../tls`: committed on purpose, they protect
nothing and must never be trusted by a real system.

The chain is the one of `../tls` (`valid` for `test.zoraya`, `expired` for `expired.zoraya`, `wrong` for `other.zoraya`,
served as `wrong.zoraya`) with one difference: the root's name constraint `permitted;DNS:.zoraya` is **not critical**.
`git-remote-http` is built on mbedTLS 3.6 (ADR-0461), which cannot read a certificate with a critical name constraint
(`mbedtls_x509_crt_parse_file` fails with `-0x2562`); it parses and ignores a non-critical one. OpenSSL, rustls and any other
verifier that implements RFC 5280 enforce the constraint whatever its criticality, so a leaked copy of these keys still
cannot vouch for a name outside `.zoraya` where a conforming program trusts the root.

- `root.pem` goes into the git test disk's `/system/tls/roots` (`tools/test-apps-git.sh`) and nowhere else: never into
  `rootfs/`. The production roots (`rootfs/system/tls/roots`, 108 certificates) all parse under mbedTLS.
- `make.sh` makes all of them again; run it only to renew them.
