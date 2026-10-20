# Research-stream brief (TASK-01)

You are one of several parallel research streams. Read these first, fully:

1. `docs/research/TASK-01-design-mistakes-research.md` — the task. Its "Critical biases", "Entry format" and
   "Evidence standards" sections are binding.
2. `docs/research/index.md` — the candidate list. Your stream letter tells you which rows are yours.

## Rules

- **Write only your two staging files** (named in your prompt) under `docs/research/_staging/`. Do not edit any
  other file. Do not run git.
- **Entry format:** exactly the `### DM-NNN` / `### GR-NNN` format in the task file, every field present, in order.
  Keep each entry tight: roughly 250–500 words. Concrete failure modes beat general statements.
- **IDs:** use the IDs from the index. If a candidate does not survive research, still emit its heading with
  `- **Status:** dropped — <reason>` or `- **Status:** merged into DM-NNN` and nothing else. If you find a
  significant item not in the index, add it using an ID from your reserve block (given in your prompt).
- **Sources:** allocate IDs only from your source block (given in your prompt), sequentially. Format in your
  sources file, one per line:
  `[S<N>] Authors, Title, Venue/Publisher, Year, URL, <primary|secondary|weak>, checked 2026-10-19`
  Add `, unverified` if you could not fetch it (then it does not count toward the 2-source / 1-primary minimum).
  After the line, add one indented line: `  - supports: DM-NNN (<what claim, in a few words>)`, one per entry it
  is used for. This lets an independent verifier check you.
- **Every URL must actually be fetched by you** (WebFetch; load it with ToolSearch `select:WebFetch,WebSearch`
  if needed). Use WebSearch to find sources. Never cite title/author/year/URL from memory without fetching.
  ACM/IEEE paywalls: look for the author's copy, usenix.org open proceedings, or the system's own site.
  For LKML, lore.kernel.org links are preferred. LWN articles count as secondary.
- Each entry: at least 2 verified sources, at least 1 primary. If you can't reach that, lower the confidence
  and write `[unverified]` next to the unsupported claim — do not pad.
- Classification must be honest: `mistake | tradeoff | contested | right-at-the-time-wrong-now`. Contested
  entries present both positions, each with a source. Steelman the original decision with its historical
  constraints. Do not treat POSIX as the neutral baseline.
- Paraphrase. Quotes under 15 words only where wording matters.
- "Relevance to our OS" is for a new Rust, aarch64, capability-leaning-but-undecided OS with no compatibility
  obligations, a Wayland-like compositor, and a sandboxed POSIX compatibility layer for hosting a C compiler.
  Don't make the decision; say why it matters.

## At the end

Append to the bottom of your entries file a section `## Stream notes` with: what you searched, what you
dropped/merged and why, new entries added, open questions you could not resolve, and any source you
suspect is weak. Then reply with a **short** summary (under 200 words): counts of entries by classification,
number of sources by type, and anything the coordinator must know. Do not paste the entries into your reply.
