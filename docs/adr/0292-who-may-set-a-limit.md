# ADR-0292: Who may set or read a storage limit on the file protocol

- **Status:** accepted
- **Date:** 2026-11-11
- **Depends on:** ADR-0055, ADR-0153, ADR-0265, ADR-0290
- **Evidence:** CP-14, quotas/survey.md#storage

## Context
ADR-0265 rule 4 says `set_limit(name, bytes)` needs `WRITE` on the connection and that a limit is "set from above". It
does not say what a connection narrowed to one name (ADR-0055's note, ADR-0153 rule 2) may do. A narrowed connection
carries create's rights, `WRITE` among them (the chooser's Save, a program's output operand). The review of Q5 showed
that such a connection, narrowed to `agent-1`, could remove or raise the limit of `agent-1` (CP-14: the disk filler's
way out). `get_limit` had no check at all, so a narrowed connection learned how much the whole directory holds
(`quotas/survey.md#storage`).

## Decision drivers
1. A limit set from above must not be changed by a grant meant for creating or replacing one name.
2. The smallest rule that closes the hole now, with no new right in the ABI before the architect decides.
3. A program that holds a limited directory itself keeps the outer limit whatever it does below it (ADR-0265 rule 4).

## Considered options

### Option A: `WRITE` on a full directory connection (built)
- **Description:** `SetLimit` is refused with `ACCESS_DENIED` on a narrowed connection and without the rights of a
  whole tree, `WRITE | CREATE | REMOVE | RENAME`: `WRITE` alone is not enough, because a `write` operand that names a
  directory, or two `create` operands in one directory, give a full connection with `WRITE` (ADR-0153's grants) and
  no `REMOVE`. `GetLimit` needs `READ` and is refused on a narrowed connection, as a listing is.
- **Evidence for:** Drivers 1 to 3; no new right.
- **Evidence against:** Every program holding a directory with `WRITE` can set limits on its subdirectories, repeating
  the subtree scan (bounded by ADR-0290's scan bound).
- **Cost:** *(estimate)* 10 lines in the service.
- **Precedent:** XFS project quotas need a privileged `quotactl`, not write access.

### Option B: a separate `LIMIT` right
- **Description:** `rights::LIMIT` (bit 8); only a trusted line's whole-tree access carries it, never create, change
  or save.
- **Evidence for:** Drivers 1 and 3 without trusting every `WRITE`; rooms' programs would not set limits at all.
- **Evidence against:** An ABI shape; the rights table, `grants`' words and the compositor's rights masks change.
- **Cost:** *(estimate)* 80 lines across the protocol, the service, the shell's grants.
- **Precedent:** Fuchsia's fine-grained rights for directory operations.

### Option C: only the keeper sets limits (a fsd request on the supervisor channel)
- **Description:** No `SetLimit` on file connections.
- **Evidence for:** The least surface.
- **Evidence against:** A second channel in `fsd`; the shell could not limit a directory by hand.
- **Cost:** *(estimate)* 120 lines.
- **Precedent:** none found.

## Recommendation
**Recommendation (not a decision): Option A now, Option B when rooms (Q6a) hand `WRITE` to untrusted programs on a
directory above their own.** A is built in Q5; the narrowed connection cannot touch a limit.

## Consequences
- **Positive.** The hole closes with no ABI change.
- **Negative.** Any holder of a whole tree's rights may set, raise, lower or remove any limit below its directory,
  the keeper's included (not the one on its own root or above: a name is one component). The outer limit always binds.
  Q6a must give a room's `/tmp` its own limited directory as the connection itself, never as a child of a directory
  the room holds. On a full volume a limit can be changed or removed but not set again (ADR-0290).
- **Follow-up.** Q6a decides between A and B before an untrusted room runs.

## Invariants
None.

## Open questions
- Whether a keeper that must reset a limit on a full volume needs the reserve extended to new records.
- Whether the keeper's connection should be the only one with a `LIMIT` right once untrusted rooms run.

## Revisit triggers
- An untrusted room's program holding a directory with `WRITE` that is not the room's own limited directory.

## Architect's decision

Accepted 2026-11-12. Decided by the architect; recorded by an agent at the architect's request.
