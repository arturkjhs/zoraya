# ADR-0171: A list type for settings (a change to ADR-0157)

- **Status:** accepted
- **Date:** 2026-11-03
- **Depends on:** ADR-0150, ADR-0157, ADR-0163, ADR-0169
- **Evidence:** CP-23, DM-086, DM-087, desktop/survey.md#settings, desktop/survey.md#keyboard-layouts

## Context
ADR-0157 (accepted) declares every setting with one of five types: `yes-no`, `number MIN MAX`, `text`, `path`, `choice A|B|C`, and names "a setting that needs a structure (a list of records)" as its revisit trigger, with the answer "a typed list syntax, not a second file format". The desktop's first setting that is a list is the enabled keyboard layouts (ADR-0163, ADR-0169): an ordered set of names from ADR-0163's table, of which one is active. ADR-0169 declared it as `text` and had the compositor split it at spaces, which puts the checking ADR-0157 driver 2 asks for ("mistakes are caught when made", DM-087) into the owner instead of the `settings` command, so `settings gui.keyboard-layouts 'us uk'` would be accepted and reported only when the compositor reads it. The architect asked for the type instead. The survey's precedents: `sysrc`'s `+=` and `-=` edit list-valued `rc.conf` variables by item with a separator the value's first character sets, and refuse a duplicate (`desktop/survey.md#settings`); XKB keeps layouts as an ordered group list (`desktop/survey.md#keyboard-layouts`). Measured: nothing. Assumed: lists of words are enough for stage 7; a list of records is not needed yet.

## Decision drivers
1. A list's items are checked by the `settings` command and by the reader against the item's type, so a wrong item is refused when made (ADR-0157 driver 2, CP-23, DM-087).
2. One file format: a list is a value on one `NAME = VALUE` line in ADR-0157's syntax (ADR-0157's revisit trigger, driver 1).
3. Lists are edited by item, as `sysrc` does, without retyping the whole value (`desktop/survey.md#settings`).
4. Order is kept, since the layouts' order is the switching order (ADR-0163).

## Considered options

### Option A: `text` split by the owner (ADR-0169 as first written; `rc.conf`'s untyped strings)
- **Description:** The setting is `text`; the owner splits it at spaces and checks the items when it reads the file.
- **Evidence for:** No change to ADR-0157; `rc.conf` works this way (`desktop/survey.md#settings`).
- **Evidence against:** Driver 1 fails: `settings` accepts anything, and the mistake is seen only when the owner reads the file (DM-087); every owner writes its own splitter.
- **Cost:** None now; a check per owner later.
- **Precedent:** FreeBSD's `rc.conf`.

### Option B: One setting per item (`gui.keyboard-layout-1`, `gui.keyboard-layout-2`, ...)
- **Description:** A list becomes numbered settings of the item's type.
- **Evidence for:** No new type; each item is typed and checked.
- **Evidence against:** The count is not declared, order is by name, inserting an item renumbers the rest, and `settings --all` lists a hundred names for a long list (driver 3 fails).
- **Cost:** *(estimate)* Small in the crate; large for people.
- **Precedent:** Windows' numbered registry values `[outside evidence]`.

### Option C: `list of TYPE`, one line, items in ADR-0150's words, edited by item
- **Description:** ADR-0157 rule 3 gains the type `list of TYPE` where `TYPE` is one of the other five; the value is ADR-0150's word list on one line (`gui.keyboard-layouts = us ua cz`, items that need quoting quoted as ADR-0150 quotes them); the declaration may give `MIN MAX` for the count and `unique`; `settings` gains `settings NAME +ITEM` (append, refused if `unique` and present), `settings NAME -ITEM` (remove, refused if absent) and `settings NAME ITEM...` (replace the whole list), each item checked against `TYPE`; a bad item refuses the whole command and leaves the file unchanged; the reader applies the same check and, on a bad item, reports the line and uses the default list, as rule 5 says for a bad value.
- **Evidence for:** Drivers 1 to 4: items typed and checked at the command, one line, `sysrc`'s item editing, order kept; ADR-0150's word syntax already defines splitting and quoting, so the parser is the shell's.
- **Evidence against:** A list of records (pairs) is still not expressible; a long list is one long line.
- **Cost:** *(estimate)* 100 lines in ADR-0157's crate and `settings`, and the declaration syntax.
- **Precedent:** `sysrc`'s `+=`/`-=` (`desktop/survey.md#settings`); Nushell's and fish's list-valued variables, which ADR-0150 already follows for the shell.

### Option D: A second file format for structured settings (a record syntax, as the registry's keys and subkeys)
- **Description:** Lists and records in a nested syntax of their own, beside the flat file.
- **Evidence for:** Any structure.
- **Evidence against:** ADR-0157's revisit trigger rules out a second file format; DM-087's lesson on per-program syntaxes.
- **Cost:** *(estimate)* A parser, a writer and a command for the second format.
- **Precedent:** dconf's paths, the registry (DM-086 via `desktop/survey.md#settings`).

## Recommendation
Recommended: **Option C**. It is a recommendation, not a decision, and it is a change to accepted ADR-0157 in the sense of ADR-0001 rule 1; if accepted, ADR-0157 rule 3's type list and rule 4's command forms gain what follows, recorded there under rule 3 of ADR-0001.

- **Declaration.** `setting NAME list of TYPE [MIN MAX] [unique] DEFAULT TEXT`, with `TYPE` one of `yes-no`, `number MIN MAX`, `text`, `path`, `choice A|B|C`; `DEFAULT` is a quoted word list (`'us'`). `MIN MAX` bound the count (default 0 and 64 `[assumption]`); `unique` refuses a repeated item.
- **Value.** One line, `NAME = ITEM ITEM ...` in ADR-0150's word syntax: items split at spaces, an item with a space quoted; no expansion. An empty list is `NAME =`.
- **Command.** `settings NAME` shows the items one per line with their index; `settings NAME ITEM...` replaces; `settings NAME +ITEM` appends, `settings NAME -ITEM` removes the first equal item; `settings NAME +ITEM --at N` inserts before index N. Every item is checked against `TYPE`, the count against `MIN MAX`, `unique` against the result; any failure refuses the command with ADR-0155's message form (`settings: gui.keyboard-layouts: uk is not one of us, ua, ru, cz`) and the file is unchanged. `--check` and `--reset` work as for other types.
- **The reader** (ADR-0157's crate) returns a list of typed items, or reports the line and the first bad item and returns the default, as rule 5 says.
- **The desktop's use.** ADR-0163's enabled layouts become `gui.keyboard-layouts`, `list of choice us|ua|ru|cz`, count 1 to 4, `unique`, default `'us'`, in switching order; `gui.keyboard-layout` stays a `choice` and must name a member, which the compositor checks (a non-member is reported and the first member used). ADR-0169's key table changes accordingly, and its "the compositor splits at spaces" rule goes.

**Tensions with accepted ADRs.** *ADR-0157* rule 3 (the five types) and rule 4 (the command's forms) are extended as above; its revisit trigger is this ADR; nothing else in it changes. *ADR-0150*: its word syntax is reused for values, which rule 2 of ADR-0157 already does for one word.

### Effect of the vision
None on the choice; for target 2 a sandbox's lists are in its own `/config` (ADR-0157). Target 3's records (a list of shortcuts with keys and commands) are the next type and would be a list of quoted words paired by position until then.

### Deferred to stage 6
Nothing here depends on POSIX.

## Consequences
- **Positive.** Typed, checked, ordered lists on one line, edited by item.
- **Negative.** Still no records; one long line for a long list.
- **Follow-up.** The type in ADR-0157's crate and `settings`; the two desktop declarations.

## Invariants
- **INV-0171-1.** `settings NAME +ITEM`, `-ITEM` and a replacement each refuse any item outside `TYPE`, any count outside `MIN MAX` and any repeat under `unique`, and leave the file unchanged on refusal; on success the file's line holds exactly the resulting items in order. *Check:* a host test of the command over every item type, bounds and `unique`, comparing the file before and after.
- **INV-0171-2.** The reader returns the declared default when a line holds a bad item, after reporting it once, and never a partial list. *Check:* a deterministic random test of the reader over lines with bad items in every position.
- **INV-0171-3.** The value syntax round-trips: the writer's line, parsed by ADR-0150's word rules, gives back the items, for items with spaces and quotes included. *Check:* a host property test over random items.

## Open questions
- *(Check note, stage 7f.)* INV-0171-1: `zoraya-settings`' `lists_are_read_written_and_edited` (append, insert,
  remove, replace, each bound and `unique`, the file unchanged on refusal). INV-0171-2:
  `a_bad_item_anywhere_gives_the_default`. INV-0171-3: `list_lines_round_trip` (300 deterministic random lists over
  spaces, quotes, backslashes and `#`).
- Whether `--at N` is worth having in the first version.
- Whether a list of records should come before target 3 needs it.

## Revisit triggers
- A setting that needs records.
- A list that does not fit one line of a terminal.

## Architect's decision

Accepted 2026-11-03. Decided by the architect; recorded by an agent at the architect's request.
