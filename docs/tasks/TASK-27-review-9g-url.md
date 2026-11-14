# TASK-27: review of 9g-url, a URL's name rule under `net web` (branch `work/9g-url`)

An independent review by the `opus-review` subagent, which wrote none of the reviewed code, of
`git diff origin/main...origin/work/9g-url` at `2a012b6`: `crates/zoraya-abi/src/net.rs` (`Target::PublicName`, kind 6,
its subset rule, `admits_public_answers`, `name_grant`, `resolve_grant`, the decoder), `crates/zoraya-shell/src/netgrant.rs`
(`net_for`, `pills`, `describe`, `web_port`), the tests in `zoraya-abi`, `zoraya-net` (`conn_tests.rs`,
`service_tests.rs`), `zoraya-shell` and `zoraya-panel`, `tests/gui/netsurf.script`, `tools/test-httpd` and the `U-*`
mutants of `tools/mutants/net-web.toml`. Read with the code around it: `zoraya_http::parse_url` and `valid_host`,
`netd`'s `connection.rs` (`decide`), `conn.rs` (`open`, `next_candidate`), `service.rs` (connect and resolve,
`found_reply`), `zoraya-posix/src/socket.rs` (connect by a remembered name), `zoraya-panel` `window_record` and
`zoraya-shell` `grants::window_pills`; against ADR-0220 (item 5), ADR-0223 (item 4), ADR-0224 (items 2, 3), ADR-0324
(items 2, 3, 6, 7, INV-0324-1..3), ADR-0325, ADR-0328 (Option B, INV-0328-1) and the 9f-net review's finding 2.

What was run (host only, no QEMU): `cargo test -p zoraya-abi -p zoraya-net -p zoraya-shell -p zoraya-panel` on the
branch head (all pass); `tools/check-untrusted-gate.sh` (ok); `make adr-check` (0 errors); and four hand mutants, each
reverted after: in `net.rs` `(PublicName(n), Name(p)) => n == p` to `true`, `(PublicName(_), Public) => true` to
`false`, `(PublicName(n), Wildcard(s)) => under(n, s)` to `true` (all three killed by `cargo test -p zoraya-abi --lib`),
and in `conn.rs:197` the `continue` after a refused candidate to `break` (survives all of `cargo test -p zoraya-net
--lib`, finding 1). `make test-netsurf` was not run here; the report says it ran on the branch.

## Findings

No error (must-fix) found.

1. **warning (should-fix).** `crates/zoraya-net/src/conn_tests.rs:1951` and `service_tests.rs:938` (the new tests),
   against `conn.rs:189-198` and `service.rs:714-724`. **What.** INV-0328-1 says a public-only name uses only its
   public, non-local answers. Every test of the rule (and of the web rule before it) gives the name a single answer, so
   nothing checks the case the rebinding actually produces in practice: one reply with both a public and a private
   address. The code is right by reading (each candidate is filtered, `found_reply` filters each address), but the
   tests cannot tell a filter from a check of the first answer. Evidence: replacing the `continue` after
   `self.saw_local = true` (`conn.rs:197`) by `break` passes all 109 `zoraya-net` lib tests; under it a reply
   `[10.1.2.3, 93.184.215.14]` fails the connection instead of using the public answer. That mutant fails closed, but
   a fail-open slip of the same shape (checking only the first or only the last answer) would pass the same tests.
   **Scenario.** A later edit of the candidate loop, for instance one that sorts or deduplicates `candidates` and
   checks `refused` once, sends a URL name's private second answer through, and no host test notices.
   **Fix.** A conn test under `public_name_only("evil.example", 8080)` with a reply of two A records, in both orders
   (`[private, public]` and `[public, private]`, the public one refusing the connection with RST so that the private one
   would be tried next): exactly the public address is opened (`tcp_count`, and the address of the opened socket), never
   the private one. The same through `resolve_now` in `service_tests.rs` (two `net.hosts` entries or a two-record DNS
   reply): the reply holds the public address alone. Add the `break` mutant to `tools/mutants/net-web.toml`.

2. **warning (should-fix, for the architect).** `docs/adr/0324-a-web-grant-for-browsers.md:122` and
   `docs/adr/invariants.md:636`. **What.** INV-0324-2 reads "No rule but a web rule is within a web rule", and its
   Status is `checked: make test-unit`. Since this branch, `Target::PublicName` is within a web rule (`net.rs:307`),
   so the invariant as written is false while the table says it is checked. ADR-0328's accepted recommendation does
   decide this subset rule ("within a web rule on its port"), and the branch records it with a rule-6 note in ADR-0324's
   open questions, which is all an agent may do; but the binding text of INV-0324-2 and the table row still state the
   old rule, and `make adr-check` cannot see the conflict. **Scenario.** A later task reads INV-0324-2, writes a check
   that nothing but `Target::Public` is within a web rule, and either breaks NetSurf's narrowing or "fixes" the subset
   rule back. **Fix.** The architect amends INV-0324-2 (or records in ADR-0328's decision that it replaces INV-0324-2's
   first clause with "no rule but a web rule or a public-only name is within a web rule"); then the table row follows.
   The code needs no change.

3. **note.** `crates/zoraya-shell/src/netgrant.rs:320`, with `crates/zoraya-panel/src/lib.rs:602`. The pill of a
   public-only name is 7 bytes longer than a `+net` word's (`+net HOST:PORT public`). The panel refuses any pill line over
   200 bytes, so a URL whose host is 183 to 190 bytes long on a port with four or five digits now gives a window the
   panel refuses, where the old pill fitted. It fails closed (no window), so it is a usability limit, not a hole; a
   name that long is rare. Record it under the known limits, or have `window_pills` refuse such a line with its own
   message before the panel does.

4. **note.** `crates/zoraya-shell/src/netgrant_tests.rs:760`. The comment says "A description outside the image has no
   web rule, so the URL keeps its name rule there", but the assertion is `outside == None`: outside the image
   (`base_system` false) the `url` operand gives no rule at all (`net_for` reads operands only for base-system
   commands). The test is right; the comment describes a case it does not test. Say "gives no rule" instead.

5. **note.** `crates/zoraya-shell/src/netgrant.rs:423-425`. The `reach, any port, public answers only` line of
   `describe` is unreachable from `net_for` (a public-only name always has the URL's one port) and has no test; it is
   reached only by a scope narrowed by hand. Harmless; a one-line test with `Ports::ANY` would keep it honest.

6. **note.** The subset rule goes one step past ADR-0328's words ("within a web rule on its port, and within a name
   rule of the same name"): a wildcard over the name and `any` are parents too. Both name the name, so the child is
   narrower (`PublicOnly` is below `Named` in every answer, as the random property test of
   `a_scope_within_another_allows_nothing_the_other_does_not` checks with `PublicName` now in its generator); the
   report records it as a decision taken without asking. Sound; listed so that the architect sees it.

## Checked and found sound

- **Decoder.** Kind 6 reads a length byte and the bytes as kind 0, then `valid()`: `valid_name` refuses empty, over 253
  bytes, an empty label, upper case, `*` and every character outside `[a-z0-9_-]`, and an all-digit last label (an
  address spelled as a name). Any protocol set but TCP alone is refused (`net.rs:478`), so UDP and echo never pass;
  `encode` re-decodes its output, so the shell cannot build what `netd` would refuse. Tested by
  `a_public_name_rule_with_udp_or_echo_or_a_bad_name_does_not_decode` and by `U-public-name-any-protocol`.
- **Subset rule.** `PublicName(n)` is within `Public` (ports and protocols compared separately), the same
  `PublicName`, `Name(n)`, a wildcard strictly above `n`, and `any`; only the same `PublicName` is within it (a `Name`,
  `Public`, `Wildcard`, `Address`, `Prefix` or `Any` child is not). Not within `Prefix(0/0)`, correctly: a prefix admits
  no connection by name. Narrowing cannot widen: under every parent above, the child's `name_grant` and
  `resolve_grant` are at most the parent's, and the child names no address. Three hand mutants of the arms are killed.
- **No more than the old rule, and no local answers.** `PublicName` covers and names no address (`covers_name`,
  `covers_address`, `names_address` all false), so `allows_name`/`reaches_name` never see it and a connection by its name
  is `PublicOnly`, whose candidate filter (`!named && (!is_public || is_local)`) refuses a superset of what `Named`
  refuses; `net.hosts` gives no exemption under it (tested). A connection by address to the name's answer on the
  rule's port is refused (no address rule); libposix sends a connect to a resolved address as a connect by the
  remembered name, and `netd` resolves again and filters each answer, so a rebinding between lookup and connect is
  caught.
- **A name that is both a URL host and a `+net` word.** `covered` is computed against the line's `+net` words only
  (before the web rule is added), so a word that covers the URL (exact name, `name:port`, `*.suffix` above it, `any`)
  keeps ADR-0223's rule and no `PublicName` is added; a word on another port, or `*.name` for `name` itself, does not
  cover, and the URL gets its public-only rule. `name_grant` takes `Named` only where a word names the name on that
  port. An address or prefix word lets a local answer through only on its own ports, which is INV-0328-1's "or the
  address".
- **Hosts and ports.** `parse_url` refuses userinfo (`user@host`, `http://host:8080@evil/`), IPv6 literals, `%`,
  a trailing dot, ports 0 and above 65535, and an all-digit last label that is not a dotted quad (`2130706433`,
  `0x7f.0.0.1`); upper case is lowered; leading zeros of a port parse to the same number (`:0080` is 80, no rule).
  Every host it accepts after lower-casing is a `valid_name`, so the encode cannot fail on the URL's host. A URL that
  does not parse adds no rule, and NetSurf's own reading of it then meets the web rule alone. `localhost`, single-label
  names and names under `nip.io`-like zones resolve, and their non-public answers are refused. A dotted address keeps
  its address rule, as ADR-0328 decides. Non-http schemes are refused by `parse_url`.
- **Ports 80 and 443.** `web_port` reads `WEB_PORTS`, the same table as the web rule; the scope of
  `netsurf http://evil.example/` is the web rule alone, the 9f-net review's rebinding scenario.
- **Rule count.** The URL adds at most one rule; more than `MAX_RULES` rules is refused before encoding.
- **Pills and the panel.** A pill is built from a decoded scope, so its target is a `valid_name` and cannot hold a
  space, a newline or a second word; a public-only pill always has two words after `+net`, a `+net` word's one, and the
  web pill is the bare word `web`, which no `+net ...` pill can equal. A host named `web` or `public` gives
  `+net web:8080 public` or `+net public:8080 public`, neither of which reads as another grant.
- **grants.** One line per rule, `reach, public answers only (operand URL)`; tested.
- **Untrusted rooms.** `web_refused` is unchanged and the new rule is only made under `net web`, which an untrusted room
  refuses as a whole; `tools/check-untrusted-gate.sh` passes.
- **curl and wget.** Their descriptions have no `net web` (`netsurf.cmd` is the only one), so `net_for` takes the
  unchanged `_ => words.push(word)` arm and they keep the URL's name rule on 80 and on 8080 (tested).
- **Mutants.** The ten `U-*` mutants are meaningful (each widens or narrows the rule observably) and the tests they
  name can fail; three further arms of the subset rule were mutated by hand above and killed.
