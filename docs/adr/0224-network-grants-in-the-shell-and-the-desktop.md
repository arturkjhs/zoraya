# ADR-0224: Network grants in the shell and on the desktop: `+net HOST[:PORT]`, `+listen PORT`, and what is shown

- **Status:** accepted
- **Date:** 2026-11-04
- **Depends on:** ADR-0150, ADR-0153, ADR-0155, ADR-0156, ADR-0157, ADR-0167, ADR-0168, ADR-0172, ADR-0220, ADR-0223, ADR-0228, ADR-0229
- **Evidence:** CP-01, CP-05, CP-24, CP-28, DM-009, DM-014, DM-072, GR-016, X-12, alternatives-postmortem.md#genode, alternatives-postmortem.md#plan-9--inferno, desktop/survey.md#spoofing-and-the-trusted-path, network/survey.md#container-and-agent-sandboxes, network/survey.md#one-bit-for-the-whole-network-android-macos-flatpak, network/survey.md#what-a-grant-names, shell/survey.md#capability-shells-and-grant-syntax

## Context

ADR-0153 made the shell the place where a person grants: operands a description marks as paths designate, `+r PATH`
and `+w PATH` grant more, `grants` shows the whole truth, a refused path is reported and the shell prints the grant
that allows it. ADR-0172 made rooms records of such grants, shown by the compositor from its own records, with an
untrusted room that gets nothing but its line. Both name `+net HOST` as the extension (CP-28; ADR-0153's revisit triggers,
ADR-0172's). ADR-0220 defines what the grant is; this ADR decides the words, what a description may declare, what
`grants`, `jobs` and the compositor show, what an untrusted room may reach, and what a person sees when a program is
refused. The agent sandboxes are the precedent for the words people accept: a list of hosts with optional ports and a
refusal that names the host (`network/survey.md#container-and-agent-sandboxes`); Deno's `--allow-net=host` is the
runtime form (`shell/survey.md#capability-shells-and-grant-syntax`).

**What is known.** ADR-0153's rules for a grant: a literal token before the command, one argument, never from a value
(DM-072, DM-009). The compositor's labels come from its own records, never the client's
(`desktop/survey.md#spoofing-and-the-trusted-path`, GR-016). One bit per application was too coarse everywhere it was
tried (`network/survey.md#one-bit-for-the-whole-network-android-macos-flatpak`, DM-014).

**What is assumed.** That a host name with an optional port is as readable as a path (ADR-0153's assumption, carried
over), and that a `*.` wildcard is understood by people who have seen one in a certificate or an allow-list.

## Decision drivers

1. **Same words everywhere** (ADR-0153, ADR-0172, CP-24): a network grant is one token, the same on a line, in a room's
   record, on a window.
2. **Never from data** (DM-072, ADR-0153 rule 3): a grant comes only from a literal token the person wrote or from a
   description in the system image (INV-0153-6).
3. **Short and safe** (DM-014): `+net HOST` with no port must be enough for the common case, or people grant `any`.
4. **A refusal names the host** and the shell prints the grant (ADR-0153 rule 11; the agent sandboxes' lesson).
5. **An untrusted room reaches nothing but its line** (ADR-0172 point 6, CP-05, X-12).
6. **The compositor draws it from its records** (INV-0172-1).

## Considered options

### Option A: No grant words: every program has the network (the Unix baseline)
- **Description:** The shell gives every command the root network connection.
- **Evidence for:** Nothing to type; every program works.
- **Evidence against:** Ambient authority (DM-009, CP-01); an untrusted room could reach anything; against ADR-0153
  rule 1 and ADR-0220.
- **Cost:** *(estimate)* None now.
- **Precedent:** Unix shells.

### Option B: One bit: `+net` with no argument (Android, Flatpak)
- **Description:** `+net` before a command grants the whole network; nothing finer.
- **Evidence for:** Shortest possible; the one bit people already know (`network/survey.md#one-bit-for-the-whole-network-android-macos-flatpak`).
- **Evidence against:** It cannot say which host, which is the use the isolation direction needs; `grants` would print
  `network: everything` for every networked command, which teaches nothing (DM-014).
- **Cost:** *(estimate)* Smallest.
- **Precedent:** Android `INTERNET`, Flatpak `--share=network`.

### Option C: `+net HOST[:PORT]` and `+listen PORT` as ADR-0153's words, operand kinds `host` and `url`, shown as the paths are
- **Description:** Below.
- **Evidence for:** Drivers 1 to 6. It is Deno's `--allow-net=host:port` and Claude Code's `allowedDomains` entries in
  ADR-0153's syntax (`network/survey.md#container-and-agent-sandboxes`, `shell/survey.md#capability-shells-and-grant-syntax`),
  and the survey's table says these are what a useful grant names (`network/survey.md#what-a-grant-names`).
- **Evidence against:** More words to learn; wildcards and ranges are policy; a URL operand designates a host *and* a
  write path for data, which is more than a `read` operand gives.
- **Cost:** *(estimate)* 400 lines in `zosh` (parsing, the scope builder, `grants`), the kinds in ADR-0156's format,
  the rooms crate, the compositor's pills.
- **Precedent:** Deno, the agent sandboxes (production); Plash's grants on the line
  (`shell/survey.md#capability-shells-and-grant-syntax`).

### Option D: Network as a namespace a room mounts (Plan 9's `/net`; Genode's domain)
- **Description:** A room's record names a network (`net home` or `net none`), and every program in the room has that
  network whole; the shell has no per-command word.
- **Evidence for:** Plan 9's model (`alternatives-postmortem.md#plan-9--inferno`) and Genode's domains
  (`alternatives-postmortem.md#genode`) are per group, which is what rooms are; one word per room.
- **Evidence against:** The unit is wrong for ADR-0153's lines, where one command needs one host; a room with
  "the network" is Option B per room.
- **Cost:** *(estimate)* Smaller than C; the per-command case is lost.
- **Precedent:** Plan 9 namespaces, Genode's NIC router domains.

## Recommendation

### Decision

Accepted as recommended (architect, 2026-11-04), with the answers: `+net HOST` without a port means any port; one `+net` covers TCP and UDP, there is no separate word for UDP; automatic designation by `url` and `host` operands for base-system commands stays, bounded by the line's and the room's rules of item 2. Two fixes: the hint of item 5 offers the smallest grant that would have allowed the request, the host *and the port* the program named (`+net example.org:443 ...`), never `any`, and the examples agree; a wildcard does not cover its own suffix, `*.example.org` grants `a.example.org` and not `example.org`, as in TLS wildcards, stated in item 1 with an example.

The original recommendation follows.

**Recommendation (not a decision): Option C.**

1. **The words.** Before the command name, with ADR-0153 rule 3's rules (a literal token, one argument, the value of a
   variable or substitution is never a grant):
   - `+net TARGET[:PORTS]` grants `reach` to `TARGET`, which is a host name (`example.org`), a wildcard (`*.example.org`,
     quoted as ADR-0150 requires since `*` globs), an address (`192.0.2.10`) or a prefix (`192.0.2.0/24`); `PORTS` is a
     port (`443`), a range (`8000-8010`) or absent, which means any port; TCP and UDP both. The shell lowers the case
     of a name (ADR-0220 item 2). A wildcard covers the names under its suffix and not the suffix itself:
     `+net '*.example.org'` grants `api.example.org`, not `example.org`, as a TLS wildcard certificate does not.
   - `+listen PORTS` grants `listen` on those ports, TCP and UDP.
   - `+net any` grants everything (`reach` any address and name, any port); it is a word, not a glob, and `grants`
     prints it in full so that it stays rare (driver 3).
   Several grants on one line are one connection whose scope is their union (ADR-0220 item 7). Every grant and
   designation is a `narrow` of the shell's *own* connection (as a path grant can name only what the shell holds,
   ADR-0153 rule 3): a shell running inside a room holds the room's scope and can give no more, which is what bounds
   an agent that runs a shell in an untrusted room. A line with any network rule also receives the read-only roots
   entry `/system/tls/roots` (ADR-0228 item 3; the path from the `tls.roots` setting, ADR-0229), shown by `grants` as
   `/system/tls/roots    read (with the network)`; a `status`-only scope does not.
2. **Designation.** ADR-0156's operand kinds gain `host` (a host name or address, with an optional `:PORT`) and `url`
   (a URL whose scheme gives the default port, `http` 80, `https` 443). An operand of either kind written literally
   designates `reach` to that host and port, automatically **only for base-system commands** (ADR-0153's Decision: a
   connection carries data both ways, so it is a write kind); for any other program the person writes `+net`. Two
   bounds, the network form of ADR-0153 rule 4: a `host` or `url` operand that came from a variable or a substitution
   designates only inside a `+net` rule on the same line or in the current room, else the line is refused before it
   runs (`zosh: http://other.example/: from $u, outside the line's network grants`); and in a room, literal or not,
   an operand designates only inside the room's network words, since the shell holds no more (item 1). So in an
   untrusted room with `+net api.example.org:443`, `fetch http://192.0.2.10/` is refused whoever typed it. A `url`
   operand designates no path. A redirection is never a network grant. One declaration beside the kinds: a
   base-system command's description may say `needs net-status`, and the shell then gives it a `status`-only
   narrowing of its own connection (ADR-0229's `net`); it designates nothing reachable and is listed in the
   tensions below, since ADR-0153 rule 2 lets a description name no fixed authority.
3. **Showing it.** `grants` prints one line per rule after the file entries, in the form
   `net: example.org:443    reach (operand URL)` or `(grant +net)`, `net: port 8080    listen (grant +listen)`, and
   `net: any    reach everything (grant +net any)`; `jobs` lists them with the job's grants; a refused request's hint
   is rule 5. Resolution is not printed separately: it is part of `reach` (ADR-0220 item 4).
4. **Rooms.** A room's line in `/config/rooms` may carry `+net` and `+listen` words as it carries `+r` and `+w`
   (ADR-0172 point 1); `room new NAME ... +net HOST` writes them; they are resolved against the creator's own scope,
   so a room holds only what its creator could reach (ADR-0172 driver 4, applied to the network: the shell's root
   connection or its own narrower one). An untrusted room's programs get the room's network words and nothing else
   (ADR-0172 point 6); a drop never carries a network grant. The compositor shows each rule as a pill in these words
   (`+net api.example.org:443`) beside the file pills, from its own records (INV-0172-1); the hover card lists them
   with their source (`room agent-1`, `operand URL`).
5. **Refusals.** When `netd` answers `NotGranted` to a program's `connect`, `datagram`, `listen` or `resolve`, the
   program's runtime (`zoraya-rt`, `libposix`) fails the call as ADR-0225 says and writes a report on the `REPORT`
   channel (ADR-0153 rule 11), `{ kind: Net, target, port, access: reach | listen }`, once per distinct target and
   port, within ADR-0153's 16 per process. After the job ends `zosh` prints, below the program's output:

   ```
     hint: curl tried to reach example.org:443, which it was not given
     to allow it: +net example.org:443 curl -O https://example.org/index.html
   ```
   The offered grant is the smallest that would have allowed the request: the host and the port the program named,
   or `+listen PORT`; never a wider wildcard, never any port, never `any`. A name refused because
   its answer is a local address (ADR-0223 item 4) is reported with the address and the hint offers `+net ADDRESS`
   (`hint: fetch's host intranet.example resolves to 10.0.2.2, the gateway, which it was not given`). A request refused by the far
   end or the network (`Refused`, `Unreachable`, `TimedOut`) is never reported: it is not a grant problem. The panel
   shows the same hint in the window's hover card with "+net HOST" as the line to run in `zosh` (ADR-0167 point 3's
   form).
6. **Native programs** say `not granted` in their own message (ADR-0155 rule 5); the hint is the shell's.

**Examples.**

```
~/proj> fetch https://example.org/ -o page.html
~/proj> grants fetch https://example.org/ -o page.html
fetch gets:
  in, out, err             the terminal
  random                   the random service
  ~/proj                   read (current directory)
  net: example.org:443     reach (operand URL)
  /system/tls/roots        read (with the network)
  ~/proj                   create or replace page.html (operand FILE)
~/proj> set u http://other.example/
~/proj> fetch $u
zosh: http://other.example/: from $u, outside the line's network grants
  hint: if that is meant: +net other.example:80 fetch $u
~/proj> curl -O https://example.org/index.html
curl: (6) Could not resolve host: example.org
  hint: curl tried to reach example.org:443, which it was not given
  to allow it: +net example.org:443 curl -O https://example.org/index.html
~/proj> +net example.org:443 curl -O https://example.org/index.html
~/proj> +net '*.example.org' curl -O https://example.org/index.html
curl: (6) Could not resolve host: example.org
  hint: curl tried to reach example.org:443, which it was not given (*.example.org covers the names under it)
  to allow it: +net example.org:443 curl -O https://example.org/index.html
~/proj> set h example.org
~/proj> +net $h curl -O https://example.org/index.html
~/proj> grants +net $h curl -O https://example.org/index.html
curl gets:
  ...
  net: example.org         reach, any port (grant +net)
~/proj> set g '+net any'
~/proj> $g curl -O https://example.org/index.html
zosh: +net any: no such command
~/proj> room new agent-1 --untrusted +w ~/sandbox/agent-1 +net api.example.org:443
~/proj> room agent-1 agent --task plan.md
```

The word after `+net` may come from a variable, as the word after `+r` may (ADR-0153 rule 3: the *token* `+net` is
literal, its argument expands to exactly one word); `$g` is a command name, not a grant. On the desktop, `agent`'s
window carries the badge `agent-1 untrusted` and the pills `+w ~/sandbox/agent-1` and `+net api.example.org:443`; in
the show-commands mode the launch reads `room agent-1 agent --task plan.md`.

**Tensions with accepted ADRs.**
- *ADR-0153 rule 1* ("nothing else"): a network connection is added only by a grant or a designating operand; INV-0153-1
  gains the `NET` slot in its enumeration (ADR-0220).
- *ADR-0153 rule 2*'s table of kinds gains `host` and `url`, whose "entry" is a scope rule, not a table entry; its
  Decision's rule that write kinds are automatic for base-system commands only is applied to them; its rule 4 gains
  the network form (item 2); and `needs net-status` (item 2) and the roots entry (item 1) are two things a line
  receives that no operand or `+` word names, against rule 2's "authority comes only from what the person typed":
  stated, both bounded (a right to read state, and a public file) and both shown by `grants`.
- *ADR-0153's Decision* keeps automatic write designation for base-system commands with a revisit trigger on the
  agent-isolation direction; item 2's room bound is what makes that safe for a `url` operand in an untrusted room.
- *ADR-0156*: two operand kinds in the description format.
- *ADR-0172 point 1*'s record grammar gains `+net` and `+listen` words; point 5's pills gain them; INV-0172-2's "the
  room's entries" includes them.
- *ADR-0167 point 2*'s GUI entries: a GUI launch adds no network; the room's or the operand's words do.

### Effect of the vision
This is where target 2 becomes visible: an untrusted room's window says `+net api.example.org:443` and nothing else, a
supervisor reads it, and the refusal hint is the audit trail of what the agent tried. Target 3: desktop programs get
their hosts from URL operands and drops of links (a later ADR); the chooser has no network form yet (open question).
Threads: none.

## Consequences
- **Positive.** One syntax for files and the network; the common fetch needs no grant beyond its URL; refusals teach
  the grant; rooms and windows show it.
- **Negative.** Two kinds and two words more to learn; `any` exists; a `url` operand gives a base-system command a
  two-way channel the person may not think of as a write.
- **Follow-up.** The parser and scope builder in `zoraya-shell`; the kinds in the description crate; the rooms crate;
  `grants` and `jobs`; the compositor's pills and hover card; the `REPORT` message kind.

## Invariants
- **INV-0224-1.** A network grant comes only from a literal `+net` or `+listen` token followed by one word, from a
  `host` or `url` operand of a base-system command's description, or as a `status`-only scope from `needs net-status`
  in such a description; a value never makes one, a non-base-system description gives none, an operand from a value
  designates only inside the line's or room's rules, and no scope exceeds the shell's own. *Check:* INV-0153-3's and
  INV-0153-9's host tests extended with `+net`, `+listen`, `url` and `host` cases, operands from values with and
  without a covering rule, and a shell whose own scope is one name.
- **INV-0224-2.** `grants` prints exactly the scope rules the command's connection would carry. *Check:* INV-0153-4's
  comparison test extended to the scope.
- **INV-0224-3.** A room holds no network rule its creator's scope did not cover. *Check:* INV-0172-4's test with
  `+net` words outside the creator's scope.
- **INV-0224-4.** A refused network request is reported once per target and port and printed with the grant that
  covers it, a local-address refusal with the address, and a request refused by the far end (connection refused,
  unreachable) is never reported as a grant problem. *Check:* INV-0153-7's host test of the reporter extended with
  network reports, and an in-QEMU test with a C program and a native program reaching a host outside their grant and
  the runner's server on a port nothing listens on (ADR-0230); the local-address report is INV-0223-3's host test.
- **INV-0224-5.** Every pixel stating a network grant on a window is drawn from the compositor's records. *Check:*
  INV-0172-1's test with a client whose buffer imitates a `+net` pill.

## Open questions
- Whether `+net HOST` should mean any port or a scheme's default port only: *settled by the Decision*, any port.
- Whether UDP should need its own word: *settled by the Decision*, no.
- A chooser for the network (a "connect to" dialog that grants a host) for desktop programs without a URL operand.
- The exact text curl prints for `EAI_SYSTEM` (ADR-0225); the examples assume "Could not resolve host".

## Revisit triggers
- People routinely type `+net any`: the kinds are missing or the hosts too many.
- A program in scope that follows redirects across hosts it cannot know in advance: a wildcard is not enough, and the
  question becomes a prompt (Deno's `Allow? [y/n/A]`) or a proxy policy.

## Architect's decision

Accepted 2026-11-04. Decided by the architect; recorded by an agent at the architect's request. `+net HOST` without a port means any port; one `+net` covers TCP and UDP, with no separate word for UDP; automatic designation by `url` and `host` operands for base-system commands stays, bounded by the line's and the room's rules as written. Two fixes: the hint offers the smallest grant that would have allowed the request, the host and the port the program named, never `any`, and rule 5 and the examples now agree; `*.example.org` does not cover `example.org` itself, as in TLS wildcards, stated in item 1 with an example.
