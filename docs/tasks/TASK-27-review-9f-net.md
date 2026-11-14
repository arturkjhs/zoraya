# TASK-27 review of 9f-net: the `web` grant (ADR-0324) and NetSurf's refusal page (ADR-0325)

An independent review by the `opus-review` subagent, which did not write the code, of
`git diff origin/main...origin/work/9f-net` (head `80e0398`, merge base `88e03a0`). The code read around the diff:
`crates/zoraya-abi/src/net.rs` (targets, `within`, the scope's matchers, the decoder), `crates/zoraya-net/src/conn.rs`
(`is_local`, `next_candidate`), `connection.rs` (`decide`), `service.rs` (connect, resolve, `found_reply`, the
named ping), `config.rs` (`is_unicast`), `crates/zoraya-shell/src/netgrant.rs` (`parse_word`, `net_for`, `pills`,
`describe`, `web_refused`), `crates/zoraya-shell/src/syntax.rs` (single quotes), `crates/zoraya-http` (`parse_url`),
`crates/zoraya-posix/src/socket.rs` (the status to `errno` maps), `user/src/bin/zosh.rs` (every caller of
`net_grant_with` and `posix_net_scope`), `crates/zoraya-panel` (`window_record`), `ports/netsurf/zoraya_refusal.c`,
its test, patch 0008 of the curl fetcher, `tools/run-qemu.sh`, the `test-netsurf` recipe, `tests/gui/netsurf.script`
and `tools/test-httpd`. Checked against ADR-0220 to ADR-0230, ADR-0262, ADR-0268, ADR-0281, ADR-0297, ADR-0317,
ADR-0324, ADR-0325 and ADR-0330.

Run by the reviewer on a detached worktree of `80e0398`:
- `cargo test -p zoraya-abi -p zoraya-net -p zoraya-shell -p zoraya-describe -p zoraya-panel` (CARGO_BUILD_JOBS=3):
  all pass (75, 28, 106, 25 and 139 tests; the eleven new web tests among them).
- `ports/netsurf/zoraya_refusal.c` compiled with the host's `cc` against a scratch driver (not committed) that
  reproduces findings 1 and 4.
- The archive index of the `libc.a` files already built in three worktrees, read with a short script (the last
  section).

There was no QEMU run and no build of the ports or the kernel. Dispositions are left to the main model.

Severities: **must-fix** before the merge, **should-fix** (this part or a recorded follow-up), **note**.

| # | Severity | Where | Finding |
|---|---|---|---|
| 1 | must-fix | `ports/netsurf/zoraya_refusal.c:50-68`; `crates/zoraya-shell/src/netgrant.rs:79` | The refusal page offers `+net any:PORT` for a host named `any`. That grants every host on that port, the local network included. |
| 2 | should-fix | `crates/zoraya-shell/src/netgrant.rs:160-183`; `crates/zoraya-net/src/conn.rs:190-197` | Under `net web`, the URL on the line still gets a name rule. That name can rebind to a private address. |
| 3 | should-fix | `Makefile:756`; `tools/test-httpd/src/main.rs:208-211` | The `must-not-reach` check of `make test-netsurf` cannot fail. |
| 4 | should-fix | `ports/netsurf/zoraya_refusal.c:119-148`; patch 0008 `:61`, `:83` | A long URL gets NetSurf's bare error page, or a page that says the host cannot be written. |
| 5 | should-fix | patch 0008 `:28-48`, `:120` | Every failed lookup is asked again, synchronously, on NetSurf's only thread. A lookup that times out freezes the window 16 s, not 8 s. |
| 6 | should-fix | `crates/zoraya-abi/src/net.rs:337`, `:438`, `:618`; `crates/zoraya-net/src/service.rs:624` | `netd` enforces "a web rule admits no protocol but TCP" (INV-0324-1) only because the shell builds the rule that way. |
| 7 | note | `crates/zoraya-net/src/conn.rs:161`; ADR-0324 item 2 | Other hosts of the on-link subnet, and its directed broadcast, are reachable under `web` when the subnet is public. |
| 8 | note | `user/src/bin/zosh.rs:3917` | `grants` prints a native command's `net: web` in an untrusted room without the web refusal. |
| 9 | note | `crates/zoraya-shell/src/netgrant.rs:343` | `grants` prints only the low end of each port range of a web rule, and nothing for `Ports::ANY`. |
| 10 | note | tests | Untested: a redirect to a refused host, a host literally named `web` or `any`, and a line too long for the buffer. |

## Findings

### 1. must-fix: the page offers `+net any:PORT` for a host named `any`

`plain_host` (`zoraya_refusal.c:50-68`) accepts any lower-case label, so it accepts `any`, and
`zoraya_refusal_line` writes `+net any:80 netsurf http://any/`. The shell reads the word `any:80` as
`Named::Any` on port 80 with `Protocols::ALL` (`netgrant.rs:79`). That rule reaches every address on port 80
over TCP and UDP: 10.0.2.2, the gateway, the LAN and loopback. ADR-0325 item 3 and driver 2 say the line is
"the smallest grant ... never `any` or a wildcard". INV-0325-2's test does not cover this host
(`test_refusal.c`). The scratch driver printed `[+net any:80 netsurf http://any/]`.

*Scenario.* A page links to `http://any:8080/x`. Single-label names do resolve in practice: a router's DNS
answers its DHCP clients' names, and a search domain expands the name. Here `any` resolves to a public
address, so the connect to port 8080 is refused (`NotGranted`, `EACCES`), and NetSurf shows "This line, typed
in a terminal, starts NetSurf with that host and port too: `+net any:8080 netsurf http://any:8080/x`". If the
name resolves to a private address, the lookup is refused and the page offers `+net any:80`. A person who
trusts the page as Zoraya's own types it, and the window then reaches every host on that port.

*Fix.* In `plain_host`, refuse `any` (and, to be safe, `web`), so that there is no line, or write the line
with the address instead. Better still: let one function decide what the shell accepts as a `+net` word for
a single host (no `any`, no `*.`, no `/`), and test it against `parse_word`'s cases. Add `any` and `ANY` to
`test_refusal.c`.

### 2. should-fix: the URL on the line keeps a name rule that can rebind into the LAN

For NetSurf, a literal URL operand still adds `Named::Name(host)` on its port (`netgrant.rs:160-183`). A
connection by that name then follows ADR-0223's rule (`conn.rs:190-197`, the `else` arm): only this
machine, the gateway and the DNS servers are refused. 192.168.1.1 or 10.1.2.3 are accepted.

*Scenario.* The person types `netsurf http://evil.example/`. The first answer is public and gives a page
holding `<img src="http://evil.example/cgi-bin/reboot">`, with DNS TTL 0. The next lookup of `evil.example`
answers 192.168.1.1, and NetSurf sends that GET to the router on port 80. This is the rebinding that ADR-0324
driver 2 says the web grant prevents. It reaches only the URL's host and port, and with no JavaScript the
page cannot read the answer, but a GET or a form POST to a device of the LAN is possible.

ADR-0324 item 2 says that "the URL operand's rule" sits beside the web rule, so the code follows the accepted
text. The gap is in the decision, not in the code.

*Fix.* Write a proposed ADR (or raise it with the architect) saying that, when the description has `net web`,
a literal URL whose host is a name and whose port is 80 or 443 adds no rule of its own: the web rule already
covers it, with the answer check. A dotted address or another port keeps its rule.

### 3. should-fix: the "no request at the server" half of INV-0324-1's test cannot fail

The recipe fails if the server log holds `must-not-reach` (`Makefile:756`). But none of the four refused
links (`test-httpd/src/main.rs:208-211`) could reach the test server even if `netd` let the connection
through:
- `10.0.2.2:PORT` is not in the test network (203.0.113.0/24); SLIRP with `--net-open` sends it to the host's
  real 10.0.2.2.
- `127.0.0.1` is the guest's own loopback.
- `203.0.113.2:80` is the host's port 80, not the server's port.
- `two.web.test:8080` has no guestfwd on port 8080.

So the `grep` half of the check is dead. The colour checks (`ffe4b5`) do carry INV-0324-1 and INV-0325-1:
a connection that `netd` let through would fail with `ECONNREFUSED` or time out, which gives NetSurf's own
error page. The invariants table and ADR-0324's *Check* cite the server log as a second check, and it is
not one.

*Fix.* Add `--net-guestfwd "203.0.113.11:8080=$(NS_RELAY)"`, so that a leak through the port check reaches
the server and is logged. Say in the recipe's comment that the other three legs rest on the colour alone,
because SLIRP cannot route them to the server.

### 4. should-fix: long URLs lose the page, or get a false sentence

`zoraya_refusal_page` builds the line into `line[2400]` (`:119`). A URL longer than about 2100 bytes does not
fit, so `has_line` is 0 and the page says "That host cannot be written in a `+net` word, so no line can allow
it" (`:148`). That is false: the host is fine.

The whole page goes into `page[8192]` (patch `:61`). Escaped, a URL of about 3000 `&` does not fit, the
function returns 0, and the fetcher falls back to curl's "Couldn't connect" page (patch `:83`). That breaks
INV-0325-1 ("a fetch refused by the grant gets the page"). The scratch driver measured both: a 2400-byte URL
gave a page of 3131 bytes with the false sentence, and a URL of 2900 `&` gave 0. Tracking URLs of 2 to 8 KiB
are common.

*Fix.* When the line does not fit, say so ("the address is too long to show as a line"). Cut the shown
`Address:` to a bound, with a mark that it was cut, so that the page always fits. Add both cases to
`test_refusal.c`.

### 5. should-fix: the second lookup doubles a frozen UI on every failed name

curl is built with `--disable-threaded-resolver` (`ports/Makefile:551`), so its lookup already blocks
NetSurf's thread. `zoraya_refused` then calls `getaddrinfo` again for every `CURLE_COULDNT_RESOLVE_HOST`
(patch `:45`), whatever the first failure was. For a name whose lookup times out (`LOOKUP_US` is 8 s), the
window does not respond for 16 s, not 8.

*Scenario.* A hostile page holds twenty `<img>` under a domain whose authoritative server never answers:
more than five minutes of a frozen window instead of under three. ADR-0325 counted the second lookup's cost
only for refused names ("from netd's refusal at once, or its cache").

*Fix.* Ask again only when the first failure came back quickly, by timing the fetch's resolve phase
(`CURLINFO_NAMELOOKUP_TIME_T`, under a second). Or let `libposix` remember that its last lookup of that name
was refused, and read that instead of asking again.

### 6. should-fix: `netd` does not hold a web rule to TCP

The decoder accepts `Public` with any protocol bits and any ports (`net.rs:438`; `valid` at `:337` does not
look at them). `allows_echo_address` matches through `covers_address` (`:618`), so a `Public` rule with the
echo bit allows a ping to any public address. The named ping's exemption for local answers
(`service.rs:624`) uses the same matcher, so under such a rule a public gateway or DNS server can be pinged
by name, which an echo rule that names no address cannot do. Today only the shell builds web rules, always
TCP, and the only way to get `Public` with echo is to narrow from `any` (which already reaches everything),
so nothing escalates now. But INV-0324-1 says "a web rule admits ... no protocol but TCP", and `netd` does
not check it: a later `narrow` caller, or the room words of ADR-0324's follow-up, would inherit the gap.

*Fix.* Refuse `Public` with anything but `Protocols::TCP` in `ReachRule` validation (and listen rules never
take it). Alternatively, match echo by `names_address` for local answers. Add a decoder test.

### 7. note: the on-link subnet is not "local"

`is_local` (`conn.rs:161`) counts only this machine's address, the gateway and the DNS servers. When the
guest's subnet is public, every other host on it is reachable under `web` on 80 and 443: a printer, a NAS, or
the directed broadcast address. Driver 1 of ADR-0324 says "never reaches this machine or its local network".
Item 2 defines "local" as ADR-0223 item 4 does, so the code follows the ADR. The test even depends on it:
`one.web.test` and `two.web.test` are guestfwd hosts inside 203.0.113.0/24. Worth an open question in
ADR-0324: count the on-link prefix and its broadcast as local for a web rule. The test would then need its
public hosts off-link, which SLIRP's guestfwd cannot give.

CGNAT, 100.64.0.0/10, is decided correctly as not public: it is the ISP's shared network, and other
customers' CPE routers sit in it.

### 8. note: `grants` shows `net: web` in an untrusted room for a native command

`grants` takes a native command's scope from `net_grant_with` directly (`zosh.rs:3917`). It skips
`refuse_web_in_untrusted_room`, so a native command whose image description says `net web` would show the
line in an untrusted room, although running it is refused. No native command has `net web` today; the POSIX
path goes through `posix_net_scope` and is refused correctly. The fix is one call.

### 9. note: `describe` of a web rule with ranges

`rule.ports.list().iter().map(|&(l, _)| l)` (`netgrant.rs:343`) drops the high end of a range and prints
nothing for `Ports::ANY` (`ports ; not this machine ...`). The shell only makes `{80, 443}`, but `describe`
is total over scopes, and a narrowed web scope is a valid input.

### 10. note: tests not written

These cases are not tested:
- A redirect to a refused host: the page at the redirect's target. ADR-0325 claims it ("a refused link or
  redirect").
- `+net web` as a host, next to the `web` rule: the pill is `+net web` against `web`, and the two `grants`
  lines both start `net: web`, which a reader can confuse.
- The `any` host of finding 1.
- The long URL of finding 4.

## What was checked and found sound

- **The blocks.** `NOT_PUBLIC` covers 0/8, 10/8 (with 10.0.2.0/24), 100.64/10, 127/8, 169.254/16, 172.16/12,
  192.0.0/24, 192.168/16, 198.18/15, 224/4 and 240/4 (with 255.255.255.255), and the edge test is real.
  Encodings cannot get round them: every check is on the 4-byte answer or target, after resolution, so
  decimal or octal names, `0x7f.1` or a dotted name sent as a name all end as an address checked by
  `is_public` and `is_local`. There is no IPv6, so there is no mapped form.
- **Rebinding through the web rule alone.** `resolve` filters each answer (`found_reply`). A connection by
  name filters each candidate (`next_candidate`). A connection by address checks the address it connects to,
  at that moment (`service.rs:337`, plus `Conn`'s own local check). There is no window between the check and
  the use: the address checked is the address connected. Mixed answers keep only their public, non-local
  members. `net.hosts` gives no exemption under `PublicOnly`. A cached answer goes through the same filter.
- **Ports.** `name_grant` and `allows_address` match the rule's ports, so `host:port` URLs, redirects to
  another port and subresources all go to `connect` with their port and are refused off 80 and 443.
- **ICMP and UDP.** The shell's rule is TCP only. Pings by address or name need the echo bit, and the service
  test shows the refusal. There is no UDP path through `allows_address`.
- **Subset.** `(Public, Public)` is the only way into `Public`, and `Public` sits only inside `Public` or
  `Any`. The random property test now covers `name_grant`, `resolve_grant` and `names_address` monotonicity,
  which is what keeps a child from regaining the local exemption. `narrow` through the service is tested.
- **Untrusted rooms.** `tools/untrusted-gate.txt` is unchanged and `room new --untrusted` stays refused.
  `web_refused` is called on both run paths (native, and POSIX through `posix_net_scope`, which also feeds the
  pills and `/system/tls`). The gate script counts both markers. The mutant `W-shell-untrusted` is listed.
- **The `net web` parse.** The key is accepted once, for commands only, with exactly `web` after it. `+net web`
  stays a host named `web` (`Named::Name`), and its pill is `+net web`. The panel accepts exactly the line
  `web` beside the `+r`, `+w` and `+net` prefixes.
- **Escaping.** Host, URL and line are HTML-escaped (`& < > " '`) everywhere they are written into the page.
  The line quotes any URL with a byte outside the safe set. `'` becomes `%27`, and bytes outside printable
  ASCII become `%XX`. `zosh`'s single quotes have POSIX meaning (no escapes inside, `syntax.rs:835`), so the
  quoted URL is one literal word. `parse_url` refuses userinfo, so `http://a@b/` cannot make the line's URL
  rule name a host other than the one shown.
- **`run-qemu.sh`.** Both options are empty by default, so the default command line is unchanged, and
  `restrict=on` still applies when `--net-open` is not given. `--net-subnet` must match `A.B.C.0/24` and needs
  `--net`, it cannot be combined with `--net-forward`, and a guestfwd command with a comma is refused. Only the
  Makefile passes these options.

## The POSIX-sockets step and `__zoraya_calls`

This branch does not cause it, and `main` has the same problem. The branch changes nothing in
`crates/zoraya-posix`, `tools/zoraya-ar` or the libc rules of `ports/Makefile` (only NetSurf's unpack lines).
The `libc.a` built in the branch's worktree wraps 641 functions. Its `zoraya-calls.o` is 704 bytes with a
`.bss` of 641, and the archive index has none of `__zoraya_calls`, `__zoraya_calls_count` or
`__zoraya_calls_list`. The TASK-30 worktree's `libc.a` (687 functions) is the same.

`zoraya-ar`'s `section()` on `main` checks a `SHT_NOBITS` section as a file range, so once `.bss`'s offset
plus its size passes the end of the object, every symbol of the object is dropped. Calls are now past about
640. The fix is `548e8f2` (with `351baa4`) on `work/9f-text`. That worktree's `libc.a` (648 functions)
indexes all three symbols.
