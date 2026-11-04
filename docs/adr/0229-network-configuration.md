# ADR-0229: Network configuration: `net.*` settings declared by `netd`, state shown by `net status`

- **Status:** accepted
- **Date:** 2026-11-04
- **Depends on:** ADR-0107, ADR-0153, ADR-0155, ADR-0156, ADR-0157, ADR-0169, ADR-0171, ADR-0172, ADR-0220, ADR-0221, ADR-0222, ADR-0223, ADR-0224, ADR-0225, ADR-0228
- **Evidence:** CP-21, CP-23, DM-085, DM-087, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#genode, network/survey.md#fuchsias-socket-provider, network/survey.md#genodes-nic-router, network/survey.md#qemu-networking-for-development-and-ci, network/survey.md#smoltcp, shell/survey.md#freebsd-sh-and-base-system

## Context

`netd` needs an address, a gateway, DNS servers and local names. ADR-0157 made configuration one file,
`/config/settings`, of typed `name = value` lines declared by their owners, edited by `settings` or by hand, with no
configuration through the environment (CP-21, DM-085) and errors caught when made (DM-087, CP-23); ADR-0171 added
lists; ADR-0169 put the desktop's `gui.*` keys there with the compositor and the panel as declarers; ADR-0157 said
"Networking adds `net.*` names, nothing else." Under QEMU user-mode networking DHCP gives `10.0.2.15`, the gateway
`10.0.2.2` and the DNS server `10.0.2.3` (`network/survey.md#qemu-networking-for-development-and-ci`), and smoltcp has a
DHCP client that asks for exactly those (`network/survey.md#smoltcp`). This ADR decides where the network's
configuration lives, who declares and reads it, and how state that is not configuration (a lease) is shown.

**What is known.** FreeBSD keeps interface configuration in `rc.conf` as `ifconfig_vtnet0="DHCP"`-style strings
(`shell/survey.md#freebsd-sh-and-base-system`); Linux spreads it over several files (DM-085's environment is one of
them; `/etc/resolv.conf` and `/etc/hosts` are two more, which musl reads); Genode's router is configured by a
component's `<config>` with domains and DHCP servers (`network/survey.md#genodes-nic-router`,
`alternatives-postmortem.md#genode`); Fuchsia sets the DNS servers through a routed protocol, `LookupAdmin`
(`network/survey.md#fuchsias-socket-provider`), in a system of components a parent wires
(`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).

**What is assumed.** One interface. That DHCP is the common case and a static address the exception.

## Decision drivers

1. **One source of truth**: the settings file for configuration; DHCP's lease is state, shown, never written back.
2. **Typed and declared** (ADR-0157 rule 3, CP-23, DM-087): an address that is not an address is refused when typed.
3. **Read by its owner, given by its launcher** (ADR-0157 rule 7, ADR-0169): `netd` reads `/config/settings` because
   `init` gives it that file, and no program else needs to.
4. **No `/etc/resolv.conf`, no `/etc/hosts`** in any program's table (ADR-0223, CP-21).
5. **Everything has its text** (ADR-0172): what `net status` shows, `settings` can set.

## Considered options

### Option A: Unix's files (`/etc/network/interfaces`, `/etc/resolv.conf`, `/etc/hosts`, a network manager)
- **Description:** Each concern its own file in its own syntax, read by whoever needs it; a daemon rewrites
  `resolv.conf` from DHCP.
- **Evidence for:** Every tool knows them; `getaddrinfo` reads them unchanged.
- **Evidence against:** DM-087's late errors and per-program syntaxes; DHCP overwrites a configuration file, so there
  is no source of truth; every program reads the resolver's files (DM-085's pattern; against driver 4 and ADR-0157).
- **Cost:** *(estimate)* None now; the mess later.
- **Precedent:** Linux.

### Option B: `rc.conf` strings (FreeBSD)
- **Description:** `net.interface = 'DHCP'` or `'inet 10.0.2.15 netmask 255.255.255.0'` as one string a program parses.
- **Evidence for:** One file (`shell/survey.md#freebsd-sh-and-base-system`); terse.
- **Evidence against:** The value is a string with a grammar of its own, checked only when `netd` starts (driver 2).
- **Cost:** *(estimate)* Small.
- **Precedent:** FreeBSD.

### Option C: Declared, typed `net.*` settings in ADR-0157's file, DHCP as state, a `net` command for state
- **Description:** Below.
- **Evidence for:** Drivers 1 to 5; it is what ADR-0169 did for the desktop and what ADR-0157 foresaw.
- **Evidence against:** A network setting cannot express a second interface without a list of records (ADR-0157's
  revisit trigger); `netd` must be restarted for a change to apply unless it watches the file (ADR-0157's open
  question).
- **Cost:** *(estimate)* The declarations in `netd`'s description, 100 lines of reading, the `net` command.
- **Precedent:** ADR-0169; the typed stores ADR-0157 weighed.

### Option D: A configuration service with policy (Fuchsia's `netcfg`, Genode's router config)
- **Description:** A component holds the policy and pushes it to the stack over a protocol, as Fuchsia's
  `LookupAdmin` sets the resolver's servers (`network/survey.md#fuchsias-socket-provider`) and Genode's router takes
  its `<config>` (`network/survey.md#genodes-nic-router`; the two component trees are
  `alternatives-postmortem.md#fuchsia--zircon-incl-starnix` and `alternatives-postmortem.md#genode`).
- **Evidence for:** Live reconfiguration; several interfaces; policy separate from the stack.
- **Evidence against:** A service and a protocol for a file of five lines; ADR-0157 rejected a settings service for
  the same reason.
- **Cost:** *(estimate)* A service.
- **Precedent:** Fuchsia, Genode.

## Recommendation

**Recommendation (not a decision): Option C.**

1. **The settings**, declared in `netd`'s description (ADR-0156's `setting NAME TYPE DEFAULT TEXT`):

   | Name | Type | Default | Meaning |
   |---|---|---|---|
   | `net.address` | choice `dhcp\|static` | `dhcp` | how the interface gets its address |
   | `net.ip` | text | `''` | the address and prefix length with `static`, `a.b.c.d/len` |
   | `net.gateway` | text | `''` | the gateway with `static` |
   | `net.dns` | list of text | `''` | DNS servers in order; empty means the ones DHCP gave |
   | `net.hosts` | list of text, unique | `''` | local names, `name=address` (ADR-0223 item 5) |
   | `net.hostname` | text | `zoraya` | the name DHCP announces and `uname` reports |

   And one the shell declares, since the shell reads it when it builds a table (ADR-0157 rule 3: declared by its
   owner): `tls.roots`, type `path`, default `/system/tls/roots`, the root store (ADR-0228 item 3).

   The `text` values that are addresses are checked by `netd`'s own parser when read and by `settings` when written
   (ADR-0157 rule 4), the way a `path` is: a bad value is reported with its line and the default used (rule 5). A
   `type` for addresses (`address`, `prefix`) is a change to ADR-0157's type list this ADR proposes, so that `settings`
   refuses `net.gateway = 10.0.2` at the prompt rather than `netd` at start. A condition between keys (`static` needs
   `net.ip` and `net.gateway`) is `netd`'s to report at start (ADR-0157 rule 5), since `settings` checks one value
   against one type.
2. **Who reads.** `init` gives `netd` a read-only connection to `/config` (as the compositor and the panel get it,
   ADR-0169); `netd` reads the file at start and on a `reload` request a launcher may send (a new ordinal). No other
   program reads `net.*`: a program that needs the DNS servers asks `status`.
3. **State is not configuration.** The DHCP lease (address, gateway, servers, expiry), the link state, the MAC and
   the counters live in `netd` and are read with `status` (ADR-0222 item 1); nothing writes them to `/config`.
4. **The `net` command** (native, in the system image, description with `cwd none`): `net status` prints the state in
   words; `net` alone the same; it is given a `status`-only scope (ADR-0220 item 2) by the shell, which its description
   declares as the one thing it needs (`needs net-status`, the declaration ADR-0224 item 2 defines, honoured for
   base-system commands only). Setting is `settings net.NAME VALUE` (ADR-0157), never `net`'s business (ADR-0155's one-command rule).
5. **One interface.** `net0`, index 1 for `if_nametoindex` (ADR-0225); a second interface is the revisit trigger.

**Examples.**

```
~> net status
net0  up  52:54:00:12:34:56
  address   10.0.2.15/24 (dhcp, lease 1h 59m left)
  gateway   10.0.2.2
  dns       10.0.2.3 (dhcp)
  hosts     build=10.0.2.100
~> settings net.dns +1.1.1.1
~> settings net.gateway 10.0.2
settings: net.gateway: 10.0.2 is not an address
~> settings net.address static
~> net status
net0  up  52:54:00:12:34:56
  address   none (net.address = static, but net.ip is not set)
```

**Tensions with accepted ADRs.**
- *ADR-0157 rule 3*'s type list gains `address` and `prefix` (or the `text` values are checked by `netd` alone, at the
  cost of late errors); its statement "Networking adds `net.*` names, nothing else" changes: `tls.roots` is one name
  more, declared by the shell.
- *ADR-0107 item 11* fixes `uname`'s `nodename` as `"zoraya"`; `net.hostname` keeps that as its default and lets a
  person change it, which changes what `uname` reports: a change to ADR-0107 if accepted.
- *ADR-0155*: `net` is a command, not a builtin, since the shell does not own the state; its `status`-only scope comes
  from `needs net-status`, defined in ADR-0224 item 2 with its tension with ADR-0153 rule 2.
- *ADR-0169*: the same pattern, a second declarer and reader.

### Effect of the vision
For target 2 a sandbox's `/config` is its launcher's (ADR-0157 rule 1), so an agent cannot change the machine's DNS
servers, and `netd` reads the session's file, not the agent's. Target 3: a settings page for the network edits the same
keys (ADR-0169's pattern). Threads: none.

## Consequences
- **Positive.** One file, typed, declared; DHCP cannot overwrite it; no resolver files in any program.
- **Negative.** A change needs `netd` to reload; one interface only; two new setting types or late checking.
- **Follow-up.** The declarations; `netd`'s reader; `net`; the `address` and `prefix` types in the settings crate.

## Invariants
- **INV-0229-1.** `settings` refuses a `net.*` value outside its type and leaves the file unchanged; `netd` started on
  a file with a bad `net.*` line reports it once and runs on the default. *Check:* INV-0157-2's and INV-0157-3's host
  tests with `net.*` cases.
- **INV-0229-2.** No program but `netd` holds a connection through which `/config/settings` is readable for the
  network's sake, and `libposix` reads no `/etc/resolv.conf` or `/etc/hosts`. *Check:* INV-0225-3's counting fake
  `netd` and the file-only counter.
- **INV-0229-3.** `net status` prints the lease `netd` holds, and a DHCP renewal changes the output without any write
  to `/config`. *Check:* a host test of `netd`'s logic with a scripted DHCP server over the fake device, reading
  `status` before and after, and a file-write counter on the `/config` connection.

## Open questions
- Whether `netd` should watch `/config/settings` for changes (ADR-0157's open question) or wait for `reload`.
- Whether `net.hostname` should exist at all before a second machine can see it.

## Revisit triggers
- A second interface, or IPv6 configuration beyond "what SLAAC and DHCPv6 give": a list of records (ADR-0157's
  revisit trigger) or Option D.
- A measured need to reconfigure without a restart.

## Architect's decision

Accepted 2026-11-04. Decided by the architect; recorded by an agent at the architect's request.
