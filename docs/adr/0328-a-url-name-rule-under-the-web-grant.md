# ADR-0328: The rule of a URL's host name under the `web` grant

- **Status:** accepted
- **Date:** 2026-11-14
- **Depends on:** ADR-0220, ADR-0223, ADR-0224, ADR-0324, ADR-0325
- **Evidence:** CP-01, alternatives-postmortem.md#genode, network/survey.md#container-and-agent-sandboxes, network/survey.md#name-resolution-as-authority, network/survey.md#the-unix-baseline

## Context
ADR-0324 item 2 gives NetSurf the web rule (`Target::Public`, ports 80 and 443, TCP) and keeps "the URL operand's rule"
beside it. For `netsurf http://evil.example/` the shell therefore adds two rules: the web rule and a name rule
`evil.example:80` (ADR-0224 item 2). A connection by that name matches the name rule first (`Scope::name_grant` gives
`Named`), and a named connection follows ADR-0223 item 4: only this machine, the gateway and the DNS servers are
refused, so any other answer is used, 192.168.1.1 and 10.1.2.3 included.

The 9f-net review (finding 2, `docs/tasks/TASK-27-review-9f-net.md` on the review branch) gives the scenario: the first
answer for `evil.example` is public and serves a page with `<img src="http://evil.example/cgi-bin/reboot">` and a TTL
of 0; the next lookup answers 192.168.1.1, and NetSurf sends that GET to the router on port 80. It reaches only the URL's
host name and port, and without JavaScript the page cannot read the answer, but a GET or a form POST reaches a device of
the local network. This is the rebinding that ADR-0324 driver 2 says the web grant prevents
(`network/survey.md#name-resolution-as-authority`). The code follows the accepted text; the gap is in the decision.

What is measured: the host tests of 9f-net show the name rule beside the web rule
(`the_web_rule_is_the_descriptions_and_shows_as_web` in `crates/zoraya-shell/src/netgrant_tests.rs`) and that a named
connection takes a private answer (`crates/zoraya-net/src/conn.rs`, the `else` arm of the candidate filter). What is
assumed: that a person who types a URL means the site on the Internet, not a device that a name later points at.

## Decision drivers
1. No ambient authority (CP-01): what a rule reaches is what the person named. A name typed on the line names the
   site, not whatever address the name's owner chooses later.
2. Resolution is inside the grant (`network/survey.md#name-resolution-as-authority`): a name's answer is checked
   against what the rule allows, so DNS cannot move a connection into the local network.
3. The local network stays closed unless the person names it (`network/survey.md#container-and-agent-sandboxes`: the
   Internet allowed, the host and the LAN closed), ADR-0324 driver 1.
4. An intranet name must still be reachable by a word the person types: ADR-0325's page offers `+net HOST:PORT`.

## Considered options
### Option A: keep the URL's name rule as it is (the accepted text of ADR-0324 item 2)
- **Description:** The URL operand adds `Named::Name(host)` on its port, as for curl and wget; a connection by that
  name uses any answer that is not this machine, the gateway or a DNS server.
- **Evidence for:** One rule for every program with a `url` operand; an intranet URL typed on the line works without a
  `+net` word; nothing to build.
- **Evidence against:** Drivers 2 and 3: the rebinding of the review's scenario reaches the LAN on the URL's port. It
  is what the Unix baseline does for every name (`network/survey.md#the-unix-baseline`: a name means whatever it
  resolves to).
- **Cost:** None.
- **Precedent:** curl and wget here (ADR-0224 item 2); every Unix program.

### Option B: under `net web`, a URL's name rule is public-only, like the web rule
- **Description:** When the description has `net web`, a literal URL whose host is a name gives a rule that admits only
  public, non-local answers (`PublicOnly`), on its own port. On ports 80 and 443 the web rule already covers it, so the
  shell adds no rule of its own; on another port (`http://example.org:8080/`) the shell adds a public-only name rule,
  which needs a new reach kind in the network ABI (a name whose answers are checked as the web rule's are, encoding
  kind 6) or a flag on the name rule. A dotted address keeps its own rule: the person typed the address, and an address
  does not rebind. A `+net HOST` word keeps ADR-0223's rule: the person named that host, local answers included.
- **Evidence for:** Drivers 1 to 4. The same check as the web rule, so one rule of thought: a name the person did not
  give with `+net` never answers into the LAN. An intranet URL is refused with ADR-0325's page, which offers the
  `+net HOST:PORT` word that allows it. The shape of Genode's NIC router, where a domain's uplink and its local domains
  are separated by prefix, not by name (`alternatives-postmortem.md#genode`).
- **Evidence against:** `netsurf http://intranet/` needs `+net intranet` as well (one more word, offered by the page);
  a second meaning of a URL operand, for programs with `net web` only; a new reach kind for the other-port case.
- **Cost:** *(estimate)* 20 lines in the shell and its tests for ports 80 and 443; 60 more in `zoraya-abi` and `netd`,
  with tests, for a public-only name rule on other ports.
- **Precedent:** Chrome's and Firefox's private network access: a public page may not reach a private address, whatever
  the name says.

### Option C: under `net web`, a URL operand adds no name rule at all
- **Description:** When the description has `net web`, a literal URL whose host is a name adds nothing: the web rule
  decides. A URL on another port than 80 or 443 is refused by `netd` and gets ADR-0325's page.
- **Evidence for:** Drivers 1 to 3; the smallest change (a condition in `netgrant::net_for`); no ABI change.
- **Evidence against:** `netsurf http://example.org:8080/` stops working without `+net example.org:8080`, a word whose
  rule then accepts private answers (ADR-0223): the person is pushed to a wider rule than Option B gives. Driver 4 holds
  only through the page.
- **Cost:** *(estimate)* 10 lines in the shell and its tests.
- **Precedent:** Agent sandboxes whose allow-list is the only rule (`network/survey.md#container-and-agent-sandboxes`).

## Recommendation
**Option B.** It closes the rebinding of the review's scenario with the check the web rule already has, keeps a URL on
another port working without widening it, and leaves the local network to the person's `+net` words, which ADR-0325's
page offers. If accepted, it supersedes the words "and the URL operand's rule" of ADR-0324 item 2 for a URL whose host
is a name. The first slice (ports 80 and 443, no rule of its own) needs no ABI change; the other-port rule needs a
public-only name kind in `zoraya_abi::net`, with its subset rule (within a web rule on its port, and within a name rule
of the same name).

## Consequences
- **Positive.** No name typed as a URL under `net web` answers into the local network; the web grant's promise
  (ADR-0324 driver 2) holds for the URL's own host too.
- **Negative.** An intranet URL needs a `+net` word under NetSurf, unlike under curl; a new reach kind to encode,
  decode and test.
- **Follow-up.** The shell's `net_for` and its tests; the reach kind in `zoraya-abi` and `netd`'s candidate filter;
  the `grants` line and the pill of such a rule (`+net HOST:PORT` with a public-only mark, or none on 80 and 443).

## Invariants
- **INV-0328-1.** Under a description's `net web`, a connection or lookup by a name that only a literal URL operand
  gave uses no address that `is_public` refuses and no local address, unless a `+net` word of the line names the host
  or the address. *Check:* `cargo test -p zoraya-shell` (netgrant: no name rule on 80 and 443, a public-only rule on
  another port), `cargo test -p zoraya-net` (a named connection with a private answer refused under that rule).

## Open questions
- Whether curl and wget should get the same rule (a URL's name public-only unless a `+net` word names it); they have
  no web grant, and an intranet URL is their common use.
- Whether a public-only name rule is a new kind or a flag bit on the name rule (an encoding question for
  `zoraya_abi::net`).
  - *Note (9g-url, ADR-0001 rule 6).* Built as a new kind, `Target::PublicName`, encoding kind 6 (a length byte and the
    name, as kind 0), TCP only in the decoder. A flag on kind 0 would let a decoder that ignores the flag read the rule
    as a name rule, which takes every answer; a decoder that does not know kind 6 refuses the scope. Its subset rule:
    within a web rule on its ports, the same public-only name, a name rule of the same name or a wildcard rule over it,
    and `any`; only the same public-only name is within it. The pill is `+net HOST:PORT public` (not a bare
    `+net HOST:PORT`, which would read as a word that takes local answers; the panel takes `web` only as that one word),
    and `grants` says `reach, public answers only (operand URL)`.
    INV-0328-1's checks: `under_web_*` in `crates/zoraya-shell/src/netgrant_tests.rs`, `a_public_name_rule_*` in
    `crates/zoraya-abi/src/net_tests.rs` and in `crates/zoraya-net/src/{conn,service}_tests.rs`, `make test-netsurf`
    (`two.web.test:8080` reached, `lan.web.test:8080` refused with the page), the `U-*` mutants of
    `tools/mutants/net-web.toml`.

## Revisit triggers
- A browser with JavaScript: a rebinding could then read the answer, which makes Option A's gap a data leak.
- IPv6 (ADR-0229 item 5): the public check gains the IPv6 blocks.

## Architect's decision
Accepted 2026-11-14. Decided by the architect; recorded by an agent at the architect's request.

Option B: under `net web`, a URL's name rule is public-only. Implementation is a part of its own.
