# ADR-0321: `curl` and `wget` refuse every option that disables or replaces TLS verification or the roots

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0109, ADR-0224, ADR-0228, ADR-0310, ADR-0311, ADR-0312
- **Evidence:** CP-28

## Context
ADR-0228 item 3 and INV-0312-1 say that TLS verification and the roots at `/system/tls/roots` have "no option to turn that
off". ADR-0310 item 6 and TASK-27 part 9a carried that into `curl` and `wget`, and the descriptions (`user/describe/posix/curl.cmd`,
`wget.cmd`) do not offer `-k`, `--cacert` and the rest. The review of 9d found that the description is not where this holds:
a line that matches no description (an option it does not list) is run with default settings, and the `+net HOST` words
of that line still give the network. So `+net h curl -k https://h/` and `+net h wget --no-check-certificate https://h/` ran
the programs with their own `-k` and `--no-check-certificate` and the check was off. The architect decided (2026-11-13) that
the rule must hold in the programs themselves, not only in the shell's description (CP-28: a program gets what its line names, and nothing it can change by an option of its own).

Measured on the 9a build: curl 8.23.0 has 23 options that disable, replace or weaken TLS verification or the protocol and
cipher floor (`tool_getparam.c`'s table, identified by the `C_...` code); wget 1.25.0 has five settings (`checkcertificate`,
`cacertificate`, `cadirectory`, `ciphers`, `secureprotocol`) that every spelling and abbreviation of a command-line option
reaches through `setval_internal` in `init.c`.

## Decision drivers
1. The refusal must not depend on the shell: any path that starts the program with a line must meet it (INV-0228-1).
2. Structural: no spelling (`-ks`, `--no-X`, `--expand-X`, `--check-certificate=off`, an abbreviated long option) gets past it.
3. A clear error and a non-zero exit, before any connection is made.
4. Few lines in a third-party tree, in a patch under `__zoraya__` (ADR-0109 item 6).

## Considered options
### Option A: the shell refuses (today)
- **Evidence against:** the fallback of a line that matches no description keeps `+net`; driver 1 fails.

### Option B: delete the options from the programs' tables
- **Evidence for:** the program does not know them (`is unknown`).
- **Evidence against:** the tables are generated (curl) or order-sensitive (wget's `optmap`); a long diff, brittle to a version change; the abbreviation of wget's `getopt_long` could then resolve to another option.

### Option C: refuse at the one place every option passes, and force the settings again where they are applied (the recommendation)
- **Description:** curl: `getparameter` refuses by the option's `C_...` code, so a short, long, `--no-` and `--expand-` spelling is one case; `config2setopts` clears the flags and strings again before they reach libcurl. wget: `setval_internal` refuses by the setting's name (and, for `secureprotocol`, by value); `openssl.c` sets `check_cert` and clears `ca_cert` and `ca_directory` again before the context is made.
- **Evidence for:** drivers 1 to 4; two patches of about 50 lines each.
- **Cost:** *(estimate)* one hunk per file to carry to a new version.

## Recommendation
Option C. Marked as a recommendation, not a decision. Items:

1. **curl refuses** (exit 2, `curl: --NAME is refused: TLS verification and the roots cannot be turned off, replaced or weakened`):
   `-k`/`--insecure`, `--proxy-insecure`, `--doh-insecure` (verification off); `--cacert`, `--capath`, `--proxy-cacert`,
   `--proxy-capath`, `--ca-native`, `--proxy-ca-native` (the roots replaced); `--ssl-no-revoke`, `--ssl-revoke-best-effort`,
   `--ssl-allow-beast`, `--proxy-ssl-allow-beast` (the checks weakened); `-1`/`--tlsv1`, `--tlsv1.0`, `--tlsv1.1`,
   `--proxy-tlsv1`, `-2`, `-3` (a lower floor than the build's: the build has no TLS before 1.2, so these only fail later);
   `--ciphers`, `--tls13-ciphers`, `--proxy-ciphers`, `--proxy-tls13-ciphers` (a cipher string can name `aNULL` or `NULL`).
   Kept: `--tlsv1.2`, `--tlsv1.3`, `--tls-max`, `--cert-status`, `--pinnedpubkey`, `--crlfile`, `--cert` and `--key` (each adds a
   check or is client authentication), `--resolve` and `--connect-to` (the name checked is the URL's).
2. **wget refuses** (exit 2, `wget: --NAME is refused: ...`): `--no-check-certificate` and every value of
   `--check-certificate`, `--ca-certificate`, `--ca-directory`, `--ciphers`, and `--secure-protocol` with any value but `auto`,
   `TLSv1_2`, `TLSv1_3` (`PFS` restricts the key exchange only, and is refused with the others by the rule that the list is
   closed). Kept: `--certificate`, `--private-key` (client authentication), `--crl-file`, `--pinnedpubkey`, `--https-only`.
   `-e`/`--execute` and `--config` were already refused (9a).
3. The shell's descriptions keep not offering these options; the refusal is the program's.
4. Environment and configuration files stay as ADR-0310 item 6 has them (OpenSSL, curl and wget read none).

## Consequences
- Positive: INV-0228-1 and INV-0312-1 hold whoever builds the line; `+net h curl -k ...` ends before a connection.
- Negative: a legitimate use of a private CA (a test of one's own server) is not possible with these programs; a person who
  needs it changes the roots file, which is the system's (ADR-0228 item 3).
- Follow-up: the same rule for each later port that links OpenSSL and reads settings (the `ssl` module of ADR-0320 allows
  `check_hostname = False` and `CERT_NONE` in a program's own code: that is a program deciding, outside this ADR; see its open question).

## Invariants
- **INV-0321-1.** `curl` refuses every option of item 1 with a message and exit status 2, and `wget` every option of item 2, also in a line that matches no description and carries a `+net` word; the expired and the wrong-name servers see 0 request bytes. *Check:* `make test-apps` (the lines after "Nothing in the programs turns the checks off" in `tests/shell/apps.script`, and its server-log check).
- **INV-0321-2.** The settings that stay are not refused: `curl --tlsv1.2` and `wget --secure-protocol=TLSv1_2` fetch the valid page. *Check:* `make test-apps` (the same section).

## Open questions
- Whether `--cert-status` should be on by default (OCSP stapling would add a check; the test servers do not staple).
- A new curl or wget release adds options: the lists must be read again (the `C_...` codes of the new `tool_getparam.h`; the `commands[]` of the new `init.c`). A check that diffs the lists against the tables is not built.

## Revisit triggers
- A person's need for a private CA that ADR-0228 item 3's roots file does not meet.
- A port whose options are not reachable through one function.

## Architect's decision
(Left empty for the human: accepted / changed / rejected, date, notes.)
