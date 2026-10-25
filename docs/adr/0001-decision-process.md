# ADR-0001: How decisions are made and used

- **Status:** proposed
- **Date:** 2026-10-25
- **Depends on:** none
- **Evidence:** CP-13, CP-27, X-10, DM-036, DM-055, DM-057, DM-058, DM-084, GR-010, alternatives-postmortem.md#barrelfish, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#hubris, alternatives-postmortem.md#keykos--eros--capros-and-coyotos, alternatives-postmortem.md#l4-family-and-sel4, alternatives-postmortem.md#plan-9--inferno, alternatives-postmortem.md#sprite, research-log.md#process, research-log.md#verification-phase-findings

## Context

The project has one human architect and several agents that research, draft and implement. TASK-01 to TASK-02b produced an evidence base (DM, GR, CP and X IDs, plus measured data in `task-02/` and `task-02b/`). Nothing in that base is a decision: every file says so in its header (`candidate-principles.md` "candidates for the architect, not decisions"; `task-02/conflict-impact.md` "It makes no decisions").

Forces:

- Decisions on the ABI become permanent once anything depends on them. Linux's "don't break userspace" rule turned early mistakes such as a.out and ignored flag bits into things that could not be removed for decades (GR-010, DM-055). This is X-10 and CP-13: the ABI must be versioned before anyone depends on it, which requires knowing which decisions are in force.
- Agents work in parallel and from a cold start. Without a written record of what is decided, each agent re-derives or silently re-decides. TASK-01's own process found that unverified first drafts contained claims the cited sources did not support in every slice (`research-log.md#verification-phase-findings`); a decision record needs the same discipline: evidence IDs that can be checked.
- Consensus processes are slow. wayland-protocols needed years for basic protocols, which produced a parallel repository (DM-084, *contested*). The project has one architect, so it does not need consensus machinery, but it does need the architect's time to be spent on choosing, not on reconstructing options.
- Scope creep is the documented killer of small systems (CP-27: Plan 9's 90–95% on external standards; Sprite chasing commercial Unix). A decision log is where scope is held fixed.

What is measured: nothing in this ADR is measured; it is a process decision. The evidence is historical.

## Decision drivers

1. Early ABI mistakes become permanent (GR-010 a.out history; DM-055 reused clone flag bits; DM-057 syscall-stable boundary). A record of which decisions are in force is a precondition for CP-13.
2. Claims must be checkable. The TASK-01 verifiers found unsupported citations in every slice (`research-log.md#verification-phase-findings`, problem class 1). ADRs cite IDs so an independent reader can check them.
3. Options must survive. The architect may later choose an option the agent rejected; the postmortem shows several systems whose "obvious" design premise expired (Barrelfish's core-count forecast, `alternatives-postmortem.md#barrelfish`). Rejected options must be documented well enough to be picked later.
4. Scope must be held by decision, not by drift (CP-27, `alternatives-postmortem.md#sprite`).
5. Small team, single owner. L4's abandoned features (timeouts, long IPC, clans) show that mechanisms added without a clear decision owner accrete and are removed later at cost (`alternatives-postmortem.md#l4-family-and-sel4`).

## Considered options

### Option A: Kernel-community process (the Linux way)

- **Description:** No decision records. Design is settled in code review and mailing-list discussion; the only written rule is "no regressions" (GR-010). Maintainers remember why things are as they are.
- **Evidence for:** It has scaled to the largest OS project. The rule is simple and enforced by one person (GR-010). It avoids process overhead for a small team.
- **Evidence against:** Rationale is lost; later developers cannot tell a deliberate choice from an accident (DM-036: POSIX lock close semantics were "codified from an early implementation rather than designed"; DM-058: nobody could say what `lock_kernel()` protected). Mistakes become permanent because nothing records their scope or an exit path (GR-010, DM-055). For agents starting cold, unwritten rationale does not exist.
- **Cost:** *(estimate)* Zero process cost up front; high cost later in re-derivation and in frozen mistakes. For agent-driven work the re-derivation cost is paid on every task.
- **Precedent:** Linux. It worked with a stable human maintainer group over 30 years; it did not prevent the retrofit catalogue in `design-mistakes.md`.

### Option B: Formal RFC process with multi-party sign-off (Fuchsia RFCs, wayland-protocols governance)

- **Description:** Every change goes through a numbered design document with reviewers and sign-off. Fuchsia introduced Starnix through an RFC (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`; the mechanics of Fuchsia's RFC process are `[outside evidence: fuchsia.dev RFC process]`); wayland-protocols requires 2–3 ACKs and one NACK blocks (DM-084).
- **Evidence for:** Rationale, alternatives and risks are recorded up front and stay findable. The Starnix RFC named its hard parts (signals, futexes, inotify) in advance, and five years later the release notes show exactly that work (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`). Multi-party review catches unsupported claims, as TASK-01 verification did (`research-log.md#verification-phase-findings`).
- **Evidence against:** With a veto and ACK quorum, basic protocols waited "months, to years" (DM-084). We have one architect, so the quorum is one; the machinery adds latency without adding reviewers.
- **Cost:** *(estimate)* Moderate writing cost per decision (the same as Option C), plus coordination cost that a one-architect project cannot use.
- **Precedent:** Fuchsia (active, outcome not final), wayland-protocols (fragmentation and a parallel repository until the 2024 experimental tier; DM-084, *contested*).

### Option C: Lightweight ADRs with a single deciding architect (non-Unix precedent: Plan 9's and Hubris's single-team ownership)

- **Description:** Numbered ADRs in `docs/adr/`, written by agents from the evidence base, each with options, a recommendation, invariants and an empty "Architect's decision" section. The architect alone changes status. Implementation agents treat accepted ADRs as binding. Plan 9 was built by one small team that wrote most of its software new rather than porting it (`alternatives-postmortem.md#plan-9--inferno`); Hubris was built by one company for its own hardware, deliberately "without excess generality" (`alternatives-postmortem.md#hubris`).
- **Evidence for:** Keeps the record of Option B and the speed of Option A. Hubris's design is documented by its authors and co-designed with its tooling (`alternatives-postmortem.md#hubris`). The TASK-01 process (independent verification of claims, `research-log.md#process`) is already in place and can be reused for ADR review.
- **Evidence against:** A single-owner process is fragile: Coyotos and BitC ended when their leader changed jobs (`alternatives-postmortem.md#keykos--eros--capros-and-coyotos`). An ADR is only as good as its evidence links; without a checking step it degrades into Option A with extra files.
- **Cost:** *(estimate)* One document per decision, a review pass per batch, and an index. Agents pay reading cost per task, which is bounded by `index.md`.
- **Precedent:** Single-team ownership in Plan 9 (ideas spread, system did not), Hubris (ships, niche by design), Oberon (teaching system). ADRs as a format are common industry practice `[outside evidence: Nygard, "Documenting Architecture Decisions", 2011]`.

## Recommendation

**Recommendation (not a decision): Option C.**

Rules, kept short:

1. **Statuses.** `proposed`: written by an agent, not binding. `accepted`: binding on all implementation work. `rejected`: not to be implemented; the record stays. `superseded by ADR-XXXX`: was accepted, now replaced; the new ADR must say what changes and why.
2. **Who sets status.** Only the architect sets `accepted`, `rejected` or `superseded`. Agents create ADRs as `proposed` and may propose a superseding ADR. An agent never edits the "Architect's decision" section.
3. **Changed decisions.** If the architect accepts an option other than the recommended one, or changes a detail, the architect records that in "Architect's decision". An agent then adds a `### Decision` sub-heading at the top of the "Recommendation" section stating the accepted decision, and keeps the original recommendation below it, so the record shows both. The template's `##` headings are never renamed (INV-0001-1).
4. **How implementation agents use ADRs.** Before changing the `abi` crate, the kernel ABI, the personality boundary or process/event semantics, an agent reads `index.md` and the accepted ADRs it lists for that area. Accepted ADRs are followed. Any deviation, however small, requires a new or superseding `proposed` ADR before the code is merged. A silent deviation is a defect to be reverted.
5. **Evidence rules.** Every claim in Context, Decision drivers and option analysis cites a DM/GR/CP/X ID or a `task-02`/`task-02b` file section. Anything else is `[outside evidence]` with a source or `[assumption]`. Each ADR has at least three options, including the Linux/POSIX baseline and at least one non-Unix precedent, and steelmans the options it does not recommend.
6. **Invariants become checks.** Each invariant gets an ID `INV-NNNN-k` (ADR number, running index) and is listed in `invariants.md` with its checking method. When an ADR is accepted, the first implementation task that touches its area must implement each invariant's check (test, lint, CI script or `abi` type) or record in the ADR's open questions why it cannot yet be checked. CI treats an accepted ADR's invariant without a check as a warning, and a failing check as a build failure.
7. **Review.** Each batch of ADRs is reviewed by an agent that did not draft them, for unsupported citations, strawmanned options, recommendations that contradict evidence, and uncheckable invariants. Findings go to `review-notes.md`.

## Consequences

- **Positive.** Decisions and their evidence are findable by agents starting cold. Rejected options stay available. Invariants have a defined path to becoming CI checks, which is what makes CP-13's "version it from the first release" possible.
- **Negative.** Writing cost per decision. Risk of ADRs becoming stale if code changes without a superseding ADR; rule 4 and the review step are the only defence.
- **Follow-up work.** `index.md` and `invariants.md` (this batch). A CI script that lists accepted ADRs' invariants without checks (after the first acceptance). A convention for ADR numbering in parallel agent work (reserve blocks, as TASK-01 did for source IDs, `research-log.md#process`).

## Invariants

- **INV-0001-1.** Every file `docs/adr/NNNN-*.md` other than the template has the template's section headings in order. *Check:* a CI script compares the `##` headings of each ADR with `0000-template.md`.
- **INV-0001-2.** Every ADR's `Status` line is one of the four values, and `accepted`/`rejected`/`superseded` appear only with a non-empty "Architect's decision" section. *Check:* CI script.
- **INV-0001-3.** Every evidence citation anywhere in an ADR (`DM-`, `GR-`, `CP-`, `X-` IDs and `<file>#<section>` references) exists in `design-mistakes.md`, `held-up-well.md`, `candidate-principles.md` or the named research file, and the `Evidence` line is exactly the set of citations used in the body. *Check:* a CI script that extracts every citation token from the whole file, greps the research files for each ID and resolves each section slug against the file's headings, then compares the body's set with the `Evidence` line.
- **INV-0001-4.** Every invariant in an accepted ADR appears in `invariants.md` with a checking method. *Check:* CI script that diffs the `## Invariants` sections against `invariants.md`.
- **INV-0001-5.** `index.md` lists every ADR file with its status. *Check:* CI script.

## Open questions

- How are ADR numbers allocated when several agents draft in parallel? Proposed: the task brief reserves a block, as TASK-01 did for source IDs. To be settled when the second batch is commissioned.
- Does the architect want a time limit on `proposed`? Not settled by any evidence here.

## Revisit triggers

- A second human decision-maker joins: rule 2 needs a quorum definition.
- An accepted ADR is found to have been silently deviated from in merged code more than once: rule 4 needs tooling, not text.
- The ADR count exceeds what `index.md` can summarise in one screen: the index needs areas or tags.

## Architect's decision

