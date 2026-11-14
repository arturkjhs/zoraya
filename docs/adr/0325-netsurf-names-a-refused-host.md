# ADR-0325: NetSurf shows a page naming a refused host and the line that would allow it

- **Status:** accepted
- **Date:** 2026-11-14
- **Depends on:** ADR-0220, ADR-0224, ADR-0225, ADR-0312, ADR-0317, ADR-0318, ADR-0324
- **Evidence:** CP-12, DM-034, network/survey.md#container-and-agent-sandboxes

## Context
When `netd` refuses a connection or a lookup that the grant does not cover (`NotGranted`), `libposix` fails `connect`
with `EACCES` and `getaddrinfo` with `EAI_SYSTEM` and `EACCES` (ADR-0225 item 6). curl turns that into "Couldn't
connect to server" or "Couldn't resolve host name", and NetSurf shows that string on its error page: the person cannot
tell a refusal of the grant from a network that is down, nor learn what to type. ADR-0224 item 5 decided the shape of
the answer (the smallest grant, the host and the port the program named, as a line to type), through a `REPORT` channel
and the shell's hint, neither of which is built, and the shell's hint would come only after the window closes. The
architect decided (2026-11-14) that NetSurf itself shows a page naming the host and the exact line that would allow it.

What is known: curl records the `errno` of a failed `connect` (`CURLINFO_OS_ERRNO`, set in `cf-socket.c` on an
immediate failure); it records nothing for a failed lookup. NetSurf's curl fetcher can send its own response (header,
data, finished) as its `about:` fetchers do.

## Decision drivers
1. A refusal says what it refused and how to allow it (CP-12; ADR-0224 item 5's hint): the host, the port, and one line.
2. Nothing moves unasked (DM-034): the page offers the line, it grants nothing, and the smallest grant is offered,
   never `any` or a wildcard.
3. A refusal is told apart from the network's own failures (refused, unreachable, timed out are not grant problems,
   ADR-0224 item 5).
4. The page is drawn inside the window's body, which a site also draws: it must not claim an authority the trusted UI
   alone has (ADR-0317's pills are the grant; agent sandboxes' denials show the rule to add,
   `network/survey.md#container-and-agent-sandboxes`).

## Considered options
### Option A: curl's message (what 9d built, the POSIX baseline)
- **Description:** `EACCES` becomes curl's error string on NetSurf's error page.
- **Evidence for:** Nothing to build.
- **Evidence against:** Drivers 1 and 3: no host, no line, and "couldn't connect" reads as a network failure.
- **Cost:** None.
- **Precedent:** Every Unix program under a firewall that answers `EACCES`.

### Option B: the shell's hint after the program ends (ADR-0224 item 5 as written)
- **Description:** `libposix` reports on `REPORT`; `zosh` prints the hint when NetSurf exits.
- **Evidence for:** One mechanism for every program.
- **Evidence against:** `REPORT` is not built; a browser runs for a long time and the hint would come after the window
  is closed.
- **Cost:** *(estimate)* The report channel and its accounting.
- **Precedent:** ADR-0153 rule 11's file reports.

### Option C: NetSurf's curl fetcher answers a refused fetch with a page of its own (the architect's decision)
- **Description:** A patch of NetSurf's curl fetcher: when a fetch fails because `netd` refused it, the fetcher sends a
  page of its own for that URL (status 403, `text/html`) naming the host, the port and the line.
- **Evidence for:** Drivers 1 to 4; the page is where the person is looking.
- **Evidence against:** A patch to carry; the page is in the body, which a site could imitate.
- **Cost:** *(estimate)* 150 lines of C and a host test.
- **Precedent:** Browsers' own error pages (certificate errors, blocked by policy).

## Recommendation
**Option C**, as the architect decided:

1. **When.** A fetch that ends with `CURLE_COULDNT_CONNECT` and `CURLINFO_OS_ERRNO` `EACCES` (`netd` answered
   `NotGranted` to the `connect`), or with `CURLE_COULDNT_RESOLVE_HOST` when a second `getaddrinfo` of the host fails with
   `EAI_SYSTEM` and `errno` `EACCES` (the lookup was refused: under the web grant of ADR-0324, a name whose answers are
   all local or private). Any other failure keeps NetSurf's error page (driver 3).
2. **What.** The fetcher sends status 403 and `Content-Type: text/html; charset=utf-8`, then a page with the title
   `Not granted: HOST:PORT`, a heading `NetSurf may not reach HOST:PORT`, a sentence that the window's network grant does
   not cover that host and port and that no connection was made to it, the line, and a sentence on what the window's
   own grant is (`web`: public hosts on ports 80 and 443). The page's background is `#ffe4b5` (the GUI test reads it).
3. **The line.** `+net HOST:PORT netsurf URL`, with HOST as the URL has it (a name in lower case or a dotted
   address), PORT the URL's or the scheme's (80, 443), and URL the refused one, in single quotes when it holds a
   character outside `A-Za-z0-9` and `. _ ~ : / % + , = @ -` (a `'` in it written `%27`, which names the same resource).
   It is the smallest grant (ADR-0224 item 5's architect's fix: the host and the port, never `any`). A host the shell
   cannot take as a `+net` word (not a valid name or dotted address) gets the page without a line.
4. **Escaping.** Every part of the URL and host put into the page is HTML-escaped (`& < > " '`); the page is made by
   `zoraya_refusal_page` in `ports/netsurf/zoraya_refusal.c`, a function of its own with a host test
   (`make check-netsurf-refusal`, part of `make check`) for the line, the quoting and the escaping.
5. **No authority.** The page grants nothing and has no button or link that changes a grant: the person types the line
   in a terminal, where the shell checks it as any line. A site can draw a page that looks like it; it can offer a line
   that the person would read in full before typing it, and the window's pills still show what the program holds.

## Consequences
- **Positive.** A refused link or redirect says which host and port and what to type; the network's own failures still
  read as failures.
- **Negative.** A seventh NetSurf patch; a refused lookup is asked twice (the second answer comes from `netd`'s refusal
  at once, or its cache).
- **Follow-up.** The same for curl and wget is ADR-0224 item 5's hint (the `REPORT` channel), not built.

## Invariants
- **INV-0325-1.** A fetch refused by the grant (`connect` `EACCES`, or a lookup's `EAI_SYSTEM`/`EACCES`) gets the page
  with the host, the port and the line `+net HOST:PORT netsurf URL`; every other failure gets NetSurf's own error
  page. *Check:* `make test-netsurf` (the page's colour after links to 10.0.2.2, 127.0.0.1 and the gateway, and no
  request at the server), `make check-netsurf-refusal` (the page's text).
- **INV-0325-2.** Every byte of the URL and the host on the page is HTML-escaped, and the line quotes a URL with any
  character outside the safe set. *Check:* `make check-netsurf-refusal`.

## Open questions
- Whether a refusal because a name's answer is local under a name rule (`Local`, `getaddrinfo`'s `EAI_NONAME`) should
  get the page too, with `+net ADDRESS` (ADR-0224 item 5's second form); `libposix` cannot tell it from a missing name.

## Revisit triggers
- The `REPORT` channel and the shell's hint are built: the page and the hint should say the same line.

## Architect's decision

Accepted 2026-11-14. Decided by the architect; recorded by an agent at the architect's request.

The architect's decision (2026-11-14): when a host is refused, NetSurf shows a page naming the host and the exact line
that would allow it, not a bare error.
