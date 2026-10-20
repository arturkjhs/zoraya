# Verification brief (TASK-01, phase 3)

You are an **independent verifier**. You did not write the entries you check, and you should not trust them.
Read first: `docs/research/TASK-01-design-mistakes-research.md` (the "Critical biases", "Entry format" and
"Evidence standards" sections are the standard you hold entries to).

Inputs (read-only for you):
- DM entries: `docs/research/design-mistakes.md`; GR entries: `docs/research/held-up-well.md`;
  postmortem drafts: `docs/research/_staging/postmortem-merged.md`.
- Sources: `docs/research/sources.md`. Each source has `supports:` lines saying what it was cited for.

## What to check, for every item in your slice

1. **Citation support.** For each `[S<N>]` used in the item, fetch the URL (WebFetch; load with ToolSearch
   `select:WebFetch,WebSearch`; if WebFetch is rate-limited, `curl -sL` into your own scratch dir and read
   it — write downloads to a fresh subdirectory, never execute them) and check that the source actually
   says what the sentence claims. Numbers, dates, version numbers, CVE IDs, and attributions ("X argued Y")
   deserve the closest look. If a claim is not supported: correct it, find a supporting source, or mark it
   `[unverified]`. If a source does not support anything it is cited for, remove the citation.
2. **Source quality.** Is the type label right (primary/secondary/weak)? Blogs and forums are weak unless
   the author is the system's designer writing about their own system (then primary, but note it). Does each
   item still have ≥2 verified sources with ≥1 primary after your corrections? If not, downgrade confidence
   and say so.
3. **Classification honesty.** `mistake | tradeoff | contested | right-at-the-time-wrong-now`. Note: the
   draft catalog labels 42 of 105 entries `right-at-the-time-wrong-now` — that label is plausibly
   over-used. Use it only if the entry shows (with evidence) that the decision was reasonable under the
   constraints of its time *and* that those constraints changed. Contested entries must present both sides
   with sources. Is confidence justified by the sources?
4. **Steelman fairness and bias.** Is the original decision steelmanned with its real constraints? Is POSIX
   treated as a neutral baseline? Are cleaner alternatives credited without recording their real costs or
   failures? Fix wording where needed.
5. **Copying.** Quotes must be under 15 words; paraphrase otherwise.
6. **Format.** Exact field order from the task file (GR entries use the adapted names stated in the header
   of `held-up-well.md`; postmortem sections use the subsections stated in their drafts).

Be efficient: a source cited by several items in your slice only needs fetching once. You need not re-verify
a source's bibliographic details beyond checking the URL resolves to the stated work.

## Output (write only these files; do not edit anything else; do not run git)

1. `docs/research/_staging/verify/<V>-entries.md` — the **complete final text of every item in your slice**,
   in the same format, corrected where needed (unchanged items copied verbatim). Keep IDs and headings
   exactly. Write this file incrementally (append as you finish each item) so an interruption loses little.
2. `docs/research/_staging/verify/<V>-findings.md` with:
   - One line per item: `<ID>: PASS` or `<ID>: FIXED — <what changed, briefly>` or
     `<ID>: DOWNGRADED — <field: old → new, why>` or `<ID>: FLAG — <problem you could not resolve>`.
   - Source directives, one per line, for `sources.md`, exactly in these forms:
     `SRC S<N>: type=<primary|secondary|weak>` (relabel), `SRC S<N>: unverified — <reason>`,
     `SRC S<N>: note — <text to append>`, `SRC S<N>: new — [S<N>] <full line in sources.md format> | supports: <ID> (<claim>)`
     (new sources only from your block, given in your prompt).
   - A short `## Summary` with counts (pass/fixed/downgraded/flagged), the classification changes, and the
     most serious problems found.

Reply with a short summary (under 150 words). Do not paste entries into the reply.
