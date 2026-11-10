# ADR-0266: Defaults and syntax: trusted rooms unlimited, untrusted rooms limited by default, five words in the room's line

- **Status:** accepted
- **Date:** 2026-11-10
- **Depends on:** ADR-0056, ADR-0107, ADR-0155, ADR-0157, ADR-0172, ADR-0224, ADR-0242, ADR-0260, ADR-0261, ADR-0262, ADR-0264, ADR-0265, ADR-0267, ADR-0269
- **Evidence:** CP-23, shell/survey.md#capability-shells-and-grant-syntax, quotas/survey.md#linux-cgroups-v2, quotas/survey.md#windows-job-objects, quotas/survey.md#apples-memory-limits-and-jetsam, quotas/survey.md#storage

## Context
ADR-0260 to ADR-0265 give a job, a directory and an account limits. This ADR says what a room gets when the person
says nothing, how a person states and changes limits, and how `grants`, `room` and the compositor show them as text
(ADR-0172's rule that everything has its text). Today a room's line is `NAME [--untrusted] [--colour N] +r PATH +w
PATH...` in `/config/rooms` (ADR-0172 point 1), and the guest has 128 MiB (`tools/run-qemu.sh`), 512 MiB for the
POSIX session's tests (`docs/backlog.md`, stage 6e).

## Decision drivers
1. Safe by default: an untrusted room the person creates without numbers must still be unable to take the machine.
2. Least surprise for trusted work: `home` and trusted rooms behave as today (the general-purpose base).
3. Short: the task's example line is the target length, and a person types one word per limit.
4. The line is the truth (CP-23): what the room holds is written in its line, so the person reads it without
   knowing the defaults.
5. The machine varies (128 MiB to more): defaults scale with it.

## Considered options

### Option A: No limits unless asked, set per unit (Linux with systemd, the baseline)
- **Description:** Everything runs unlimited; a person who wants a bound sets a property such as `MemoryMax=` or
  `CPUQuota=` on a unit, which maps to cgroups' files (`quotas/survey.md#linux-cgroups-v2`; systemd itself is `[outside evidence: systemd documentation, not fetched]`).
- **Evidence for:** Nothing changes for anyone who does not ask; the knobs are there.
- **Evidence against:** Driver 1 fails: an untrusted room created in a hurry is unbounded.
- **Cost:** *(estimate)* None.
- **Precedent:** Linux desktops; Windows jobs are unlimited until a limit flag is set (`quotas/survey.md#windows-job-objects`).

### Option B: Fixed limits the platform sets and the person does not see (Apple's per-process limits)
- **Description:** Every untrusted program gets the platform's limit, chosen per device
  (`quotas/survey.md#apples-memory-limits-and-jetsam`).
- **Evidence for:** Nothing to type; tuned by the platform's makers.
- **Evidence against:** Driver 4: the person cannot read the limit; an entitlement or a hidden table changes it.
- **Cost:** *(estimate)* A table.
- **Precedent:** iOS, with an entitlement for more.

### Option C: Every limit explicit, no defaults (ADR-0056's `job_create`: "0 means 0")
- **Description:** `room new --untrusted` refuses without `--mem`, `--cpu`, `--procs` and `--disk`.
- **Evidence for:** The person always chose every number; nothing hidden.
- **Evidence against:** Driver 3: four options every time; people copy numbers they do not understand.
- **Cost:** *(estimate)* None.
- **Precedent:** ADR-0056.

### Option D: Defaults for untrusted rooms, scaled to the session, written into the line at creation
- **Description:** `room new NAME --untrusted` fills each limit the person did not give from the session's size and
  writes every value into the line; trusted rooms have none unless given.
- **Evidence for:** Drivers 1 to 5: safe without typing, the line shows every number, and the numbers scale.
  Capability shells write a grant as the thing itself (`shell/survey.md#capability-shells-and-grant-syntax`); the
  limits are written the same way.
- **Evidence against:** The defaults are guesses until measured; two machines produce different lines for one
  command.
- **Cost:** *(estimate)* 150 lines in the rooms crate and `zosh`.
- **Precedent:** tmpfs's `size=` defaulting to half of RAM (`quotas/survey.md#storage`).

## Recommendation
**Recommendation (not a decision): Option D.**

1. **The words.** Five limits in the room's line and in `room new` and `room set`, in ADR-0155's option syntax, and a
   sixth for an untrusted room's `/tmp`:

   | Word | Value | Means | Limit of |
   |---|---|---|---|
   | `--mem SIZE` | `512M`, `2G` (K, M, G are 1024-based) | memory of everything in the room | ADR-0261 |
   | `--cpu PCT` | `25%` (1 to 100) | at most this share of the machine per 20 ms period | ADR-0264 |
   | `--procs N` | `32` | live processes | ADR-0261 |
   | `--disk SIZE` | `2G` | on each `+w` directory of the room | ADR-0265 |
   | `--conns N` | `16` | network connections | ADR-0262 |
   | `--tmp SIZE` | `16M` | the `/tmp` of each shell in an untrusted room, inside `--mem` | ADR-0265 |

2. **Trusted rooms and `home`** have no limits of their own: they are bounded by the session's ceiling (ADR-0261
   point 10), as all of today's work is by the machine. A trusted room may be given any of the five (a build room
   with `--cpu 50%`); `--tmp` is for untrusted rooms only.
3. **Untrusted rooms always have all six.** What the person omits is filled at creation `[assumption, every value]`:

   | Word | Default | On 128 MiB (session about 86 MiB) | On 512 MiB (session about 356 MiB) |
   |---|---|---|---|
   | `--mem` | a quarter of the session's ceiling, in whole MiB | `21M` | `89M` |
   | `--cpu` | `25%` | `25%` | `25%` |
   | `--procs` | `32` | `32` | `32` |
   | `--disk` | `1G`, or an eighth of the volume's free space if less | volume-dependent | volume-dependent |
   | `--conns` | `16` | `16` | `16` |
   | `--tmp` | `16M`, or a quarter of `--mem` if less | `5M` | `16M` |

   (128 MiB leaves 125,636 KiB free after boot, the boot log's figure in ADR-0056; less the kernel's 1/16, three
   quarters of the rest is about 86 MiB.) The line is written with every value, so `room` shows what the room holds
   whatever the defaults later become. An untrusted room refuses `none` for any word, and every value above its
   parent's (point 4).
4. **The session, the untrusted rooms' job and the services** (ADR-0261 point 10), set by `init` and the room keeper
   at start `[assumption, every value]`:
   - the kernel keeps 1/16 of the free frames; `init`'s job gets the rest;
   - the *session* job, which holds `home`, every room and the console's first shell, gets three quarters of `init`'s
     memory and 200 processes (of the 255 address space identifiers, leaving 55 for `init`, the drivers and the
     services and their restarts);
   - the *untrusted* job, the parent of every untrusted room (ADR-0269), gets half of the session's memory, 96
     processes and `--cpu 50%`; so the untrusted rooms together leave `home` and trusted rooms at least half of the
     session's memory, 104 processes and half of the CPU, however many rooms there are and whatever runs in them
     (INV-0261-8, INV-0264-4). Two untrusted rooms at their default `--mem` fill it;
   - the remaining quarter of `init`'s memory is for `init`, the drivers, `fsd`, the compositor and `netd`, which have
     no ceilings of their own; their per-client state is carved as ADR-0262's table says.
   `top --session` prints these numbers.
5. **No room above its parent.** A limit above the untrusted job's (for an untrusted room) or the session's (for a
   trusted one) is refused: `room: agent-1: --mem 512M: above the untrusted rooms' 178M`.
6. **Changing.** `room set NAME WORD VALUE...` changes the line and, if the room is running, sends `Set` to the room
   keeper, which applies it to the room's job (and, for `--disk`, to its directories) at once (ADR-0261 point 3).
   Lowering below use is allowed and refuses growth until use falls. `room set` runs in `home` and in trusted rooms'
   shells; a room's own shell holds no `Set` (ADR-0269), so in it `room set` is refused with `from home: room set
   ...`. `room set NAME --mem none` is refused for an untrusted room. A new `--tmp` applies to shells started after it.
7. **What shows them, all in these words:**
   - `room` lists each room's limits after its trust and before its grants (the order of the line).
   - `grants` in a room prints one more line: `limits --mem 512M --cpu 25% --procs 32 --disk 2G --conns 16 --tmp 16M
     (room agent-1)`.
   - The compositor's records (`AddRoomClient`, ADR-0172) carry the limit words; the hover card of a window lists them
     with their use, and the rooms overview's `New room` form has a field per word with the line beside it.
   - Settings (ADR-0267) edits them through the room keeper's `Set`.
8. **In the line.** `NAME [--untrusted] [--colour N] [--mem SIZE] [--cpu PCT] [--procs N] [--disk SIZE] [--conns N]
   [--tmp SIZE] GRANT...`, read and written by the settings crate (ADR-0157); a bad value is reported once and the
   line is skipped (ADR-0157 rule 5), and an untrusted line missing a word gets the default at its first use and is
   rewritten.
9. **Not now:** limits per command line (`limit --mem 64M make`), which a trusted user may want; it needs no new
   mechanism (a sub-job with limits) and waits for a request.

**Tensions with accepted ADRs.** *ADR-0172 point 1:* the accepted line is `NAME [--untrusted] [--colour N] GRANT...`;
six words are added. *ADR-0172 point 3:* `room set` is a new subcommand beside `add`, `drop` and `remove`, and a room's
own shell may not run it. *ADR-0172 point 4* and *ADR-0242:* `AddRoomClient` gains the limit words. *ADR-0157:* the
settings crate reads and writes them. *ADR-0224:* `+net` words in a room line are unchanged; `--conns` sits beside
them. *ADR-0107:* an untrusted room's shells each start a `tmpd` of `--tmp` (ADR-0265).

**Examples.**

```
zosh ~> room new agent-1 --untrusted +w ~/sandbox/agent-1 +net api.example.com:443     # 512 MiB guest
zosh ~> room
* home
  project   +w ~/project +r ~/include
  agent-1   untrusted  --mem 89M --cpu 25% --procs 32 --disk 1G --conns 16 --tmp 16M  +w ~/sandbox/agent-1 +net api.example.com:443
zosh ~> room set agent-1 --mem 512M --cpu 10%
room: agent-1: --mem 512M: above the untrusted rooms' 178M
zosh ~> room set agent-1 --mem 128M --cpu 10%
zosh ~> room agent-1
agent-1 ~/sandbox/agent-1> grants
+w ~/sandbox/agent-1                         (room agent-1)
+net api.example.com:443                     (room agent-1)
limits --mem 128M --cpu 10% --procs 32 --disk 1G --conns 16 --tmp 16M  (room agent-1)
```

The task's line, `room new agent-1 --untrusted --mem 512M --cpu 25% +w ~/sandbox/agent-1 +net
api.example.com:443`, is valid as typed on a guest of 1.5 GiB or more, where the untrusted rooms' job holds at least 512 MiB.

### Effect of the vision
An agent's room is bounded without the person knowing any number, and its line says how. The general-purpose base is
unchanged: no limit appears on `home` or a trusted room unless typed. A larger machine gives larger defaults with no
change to any line already written.

## Consequences
- **Positive.** Safe default, short line, every number visible.
- **Negative.** The defaults are guesses; lines written on a small machine stay small after a memory upgrade.
- **Follow-up.** The rooms crate's words, `room set`, `grants`' line (Q6); `init`'s session job (Q1).

## Invariants
- **INV-0266-1.** Every untrusted room's line holds all six limit words after any `room new` or `room set`, and its
  job carries the five that are job limits. *Check:* host test of the rooms crate over `room new` and `room set` with and without each word;
  in-QEMU test reading the room job's `job_get_usage` limits after entering it.
- **INV-0266-2.** No room's limit is above its parent's (the untrusted rooms' job for an untrusted room, the session
  for a trusted one), and the session's process ceiling leaves at least 55 address space identifiers outside it.
  *Check:* host test of the shell's check; in-QEMU test that `job_set_limits` refuses it anyway (INV-0261-3) and
  that `init` starts the session job with its ceilings.
- **INV-0266-3.** `grants`, `room` and the compositor's hover card show the same limit words for a room. *Check:* host
  test comparing the three renderings of one record.

## Open questions
- Every default: measure an agent-like workload (a compiler, a fetcher, a test runner) in a room.
- Whether `--disk` should be one total over the room's directories instead of one limit per directory.

## Revisit triggers
- Measurements that show the defaults refuse ordinary work or leave the session too little.
- Threads or SMP (the `--cpu` meaning stays; `--procs` may want a sibling).

## Architect's decision

Accepted 2026-11-10. Decided by the architect; recorded by an agent at the architect's request. The numbers marked `[assumption]` are defaults until measured: each Q task of `docs/research/quotas/plan.md` measures the ones it implements and proposes changes with the numbers.
