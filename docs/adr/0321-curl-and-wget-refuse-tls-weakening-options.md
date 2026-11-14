# ADR-0321: `curl` and `wget` refuse every option that disables or replaces TLS verification or the roots

- **Status:** accepted
- **Date:** 2026-11-13
- **Depends on:** ADR-0109, ADR-0224, ADR-0228, ADR-0310, ADR-0311, ADR-0312
- **Evidence:** network/survey.md#what-a-fetch-over-https-needs

## Context
ADR-0228 item 3 makes `/system/tls/roots` the roots of every program with a network grant, INV-0228-1 and INV-0312-1 say
that a connection whose certificate is expired, for another name or signed by no root there fails before a request byte
is sent, and ADR-0310 item 6 says that the descriptions do not offer the options that name roots or turn the check off,
and that "both layers hold". The words "there is no option to turn that off" are the prose of the descriptions
(`user/describe/posix/curl.cmd`, `wget.cmd`), not of those ADRs. The review of 9d found that the description is not where
this holds: a line that matches no description (an option it does not list) is run with default settings, and the
`+net HOST` words of that line still give the network. So `+net h curl -k https://h/` and
`+net h wget --no-check-certificate https://h/` ran the programs with their own `-k` and `--no-check-certificate` and the
check was off. The architect decided (2026-11-13) that the rule must hold in the programs themselves, not only in the
shell's description. A fetch over HTTPS needs a validator, trust anchors, the name matched and TLS 1.2 or 1.3
(network/survey.md#what-a-fetch-over-https-needs): an option that removes one of them is what this ADR refuses.

Measured on the 9a build: curl 8.23.0 has 23 options that disable, replace or weaken TLS verification or the protocol and
cipher floor (`tool_getparam.c`'s table, identified by the `C_...` code), and `--tls-max` with a value below 1.2 lowers
the minimum to that value (`tlsversion()` in `config2setopts.c`); wget 1.25.0 has five settings (`checkcertificate`,
`cacertificate`, `cadirectory`, `ciphers`, `secureprotocol`) that every spelling and abbreviation of a command-line option
reaches through `setval_internal` in `init.c`, and two FTPS settings that leave the data, or the whole session, in the
clear (`ftpscleardataconnection`, `ftpsfallbacktoftp`: after a refused `AUTH TLS` the session goes on as plain FTP, which
an attacker on the path can force).

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
- **Description:** curl: `getparameter` refuses by the option's `C_...` code, so a short, long, `--no-` and `--expand-`
  spelling is one case, and the `--tls-max` handler refuses a value below 1.2; `config2setopts` clears the flags, the
  roots, the cipher lists, the native-store flags and a version floor or ceiling below TLS 1.2 again before they reach
  libcurl. wget: `setval_internal` refuses by the setting's name (and, for `secureprotocol`, by value); `ssl_init` sets
  `check_cert`, clears `ca_cert`, `ca_directory` and `tls_ciphers_string`, and puts a `secure_protocol` other than TLS 1.2
  or 1.3 back to `auto`, before the protocol and the ciphers are chosen; `ftp.c` keeps the data connection private and
  does not fall back to plain FTP.
- **Evidence for:** drivers 1 to 4; two patches of about 70 and 90 lines.
- **Cost:** *(estimate)* one hunk per file to carry to a new version.

## Recommendation
Option C. Marked as a recommendation, not a decision. Items:

1. **curl refuses** with exit status 2 and, on the error stream, `curl: --NAME is refused: TLS verification and the roots
   cannot be turned off, replaced or weakened` (the long name, also for `-k` or `--no-insecure`), then curl's own
   `curl: option -k: is badly used here` (the option as written) and its `try 'curl --help'` hint. `-s` hides the first
   line, not the other two. Refused: `-k`/`--insecure`, `--proxy-insecure`, `--doh-insecure` (verification off);
   `--cacert`, `--capath`, `--proxy-cacert`, `--proxy-capath`, `--ca-native`, `--proxy-ca-native` (the roots replaced);
   `--ssl-no-revoke`, `--ssl-revoke-best-effort`, `--ssl-allow-beast`, `--proxy-ssl-allow-beast` (the checks weakened);
   `-1`/`--tlsv1`, `--tlsv1.0`, `--tlsv1.1`, `--proxy-tlsv1`, `-2`, `-3`, and `--tls-max` with `1.0` or `1.1` (a lower
   floor than the build's: the build has no TLS before 1.2, so these only fail later; `--tls-max` below 1.2 lowers the
   minimum to it, so it is the same floor); `--ciphers`, `--tls13-ciphers`, `--proxy-ciphers`, `--proxy-tls13-ciphers`
   (a cipher string can name `aNULL` or `NULL`). Kept: `--tlsv1.2`, `--tlsv1.3`, `--tls-max` with `1.2`, `1.3` or
   `default`, `--cert-status`, `--pinnedpubkey`, `--crlfile`, `--cert` and `--key` (each adds a check or is client
   authentication), `--resolve` and `--connect-to` (the name checked is the URL's).
2. **wget refuses** with exit status 2 and, on the error stream, `--NAME is refused: TLS verification and the roots cannot
   be turned off, replaced or weakened` (no `wget:` prefix; NAME is the setting's long option, so `--no-check-certificate`
   is reported as `--check-certificate`): every value of `--check-certificate` (with `--no-check-certificate` and the
   abbreviations `getopt_long` accepts, such as `--no-check-cert`), `--ca-certificate`, `--ca-directory`, `--ciphers`,
   `--ftps-fallback-to-ftp` and `--ftps-clear-data-connection`, and `--secure-protocol` with any value but `auto`,
   `TLSv1_2`, `TLSv1_3` (`PFS` restricts the key exchange only, and is refused with the others by the rule that the list
   is closed). Kept: `--certificate`, `--private-key` (client authentication), `--crl-file`, `--pinnedpubkey`,
   `--https-only`. `-e`/`--execute` and `--config` were already refused (9a).
3. **Redirects.** curl with `-L` from an `https` URL follows redirects to `https` only (`CURLOPT_REDIR_PROTOCOLS_STR`
   set to `https` when `--proto-redir` is not given), so a redirect does not leave TLS unasked. `--proto-redir` is kept: a
   person who names `http` there asks for it, as when writing an `http` URL. wget follows a redirect from `https` to
   `http`: a known limit, not a weakening of verification (the `http` transfer has no certificate to check, and its host
   and port must be on the line, ADR-0310).
4. **The second line.** Behind the refusal, the places where the settings are applied force them again (Option C), so
   that a later code path that sets one (a new option, a change of the option table) does not reach the TLS context.
   Every refused setting is forced again, in both programs.
5. The shell's descriptions keep not offering these options; the refusal is the program's.
6. Environment and configuration files stay as ADR-0310 item 6 has them (OpenSSL, curl and wget read none).

## Consequences
- Positive: INV-0228-1 and INV-0312-1 hold whoever builds the line; `+net h curl -k ...` ends before a connection.
- Negative: a legitimate use of a private CA (a test of one's own server) is not possible with these programs; a person who
  needs it changes the roots file, which is the system's (ADR-0228 item 3). An FTPS server without `AUTH TLS` cannot be
  reached with wget; a `curl -L` from `https` to an `http` page needs `--proto-redir`.
- Follow-up: the same rule for each later port that links OpenSSL and reads settings (the `ssl` module of ADR-0320 allows
  `check_hostname = False` and `CERT_NONE` in a program's own code: that is a program deciding, outside this ADR; see its open question).

## Invariants
- **INV-0321-1.** `curl` refuses every option of item 1 with a message and exit status 2, and `wget` every option of item 2, also in a line that matches no description and carries a `+net` word; the expired and the wrong-name servers see 0 request bytes. *Check:* `make test-apps` (the lines after "Nothing in the programs turns the checks off" in `tests/shell/apps.script`, and its server-log check).
- **INV-0321-2.** The settings that stay are not refused: `curl --tlsv1.2` and `wget --secure-protocol=TLSv1_2` fetch the valid page. *Check:* `make test-apps` (the same section).

## Open questions
- Whether `--cert-status` should be on by default (OCSP stapling would add a check; the test servers do not staple).
- A new curl or wget release adds options: the lists must be read again (the `C_...` codes of the new `tool_getparam.h`; the `commands[]` of the new `init.c`). A check that diffs the lists against the tables is not built.
- Whether wget should follow a redirect from `https` to `http` (item 3); a patch like curl's would go into its redirect
  loop. Neither program's redirect rule is tested: the test servers send no redirect.

## Revisit triggers
- A person's need for a private CA that ADR-0228 item 3's roots file does not meet.
- A port whose options are not reachable through one function.

## Architect's decision
Accepted 2026-11-14. Decided by the architect; recorded by an agent at the architect's request.

Option C.
