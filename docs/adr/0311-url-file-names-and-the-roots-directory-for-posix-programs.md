# ADR-0311: The `names` key for `curl -O`, and the roots directory a POSIX program with the network is given

- **Status:** accepted
- **Date:** 2026-11-13
- **Depends on:** ADR-0153, ADR-0156, ADR-0224, ADR-0228, ADR-0297, ADR-0310
- **Evidence:** CP-28, DM-072

## Context
TASK-27 9a asks that `grants curl -O https://x/y` show exactly the network and the file grants. Two things are missing
from the accepted designs:

1. `curl -O` saves "under the remote name" in the current directory. ADR-0153 designates the files that operands and
   option arguments name; here nothing on the line names the file, the program derives it from the URL. The current
   directory is read-only to a program (rule 1), so without a grant the program fails, and with `+w .` it may create
   anything in the directory, which is more than the person meant.
2. ADR-0228 item 3: the shell adds `/system/tls/roots` read-only to any program with a network grant, shown as `read
   (with the network)`. A POSIX program is given directories only (ADR-0153 rule 10), and the shell adds the roots to
   native programs only.

## Decision drivers
1. A grant is what the person wrote or what the image's description says, nothing the program can widen (ADR-0153
   rules 1 and 3, INV-0153-1).
2. The shell, not the program, decides which file is created (CP-28, DM-072).
3. When the shell is unsure what the program will create, the line is refused, with the way to say it.
4. A POSIX program with a network can verify a certificate (ADR-0228).

## Considered options
### Option A: `names OPTION OPERAND`, a description key, and the shell derives the name from the URL (the recommendation)
In `user/describe/posix/curl.cmd`: `names --remote-name URL`. When the option is on the line the shell designates, for
each URL operand, one file to create in the current directory, named by the last path segment of the URL, if that is
plain.
- **Evidence for:** drivers 1 to 3; the grant is one `create NAME` entry (ADR-0153's table), shown by `grants`.
- **Evidence against:** the shell and the program must agree on the name: any difference is a grant that does not match
  what is created, so the rule is conservative (below).
- **Cost:** *(estimate)* 70 lines in `zoraya-describe`, 25 in `zoraya-shell`.
- **Precedent:** none in the survey; the nearest are Deno's `--allow-write=PATH` (the person names it) and Flatpak's
  portal (the program asks, a person chooses).
### Option B: do not offer `-O`; only `-o FILE`
- **Evidence against:** TASK-27's done-when says `curl -O https://...`; `-O` is the form in every guide.
### Option C: give the current directory write access for `-O`
- **Evidence against:** the program may create or replace any file in the directory, not the one the URL names;
  against driver 1.
### Option D: the program asks a chooser for the name (ADR-0170's portal)
- **Evidence against:** `curl` is a terminal program; a chooser is a graphical room's.

## Recommendation
**Recommendation (not a decision): Option A.**

1. **The key.** `names OPTION OPERAND`, any number of lines. OPTION is a declared option that takes no argument; OPERAND
   is a declared `url` operand; anything else is a parse error. `Description::names` holds them.
2. **The name.** `zoraya_describe::url_file_name(url)` is the last segment of the URL's path, with the query and
   the fragment cut off, if it is non-empty, at most 255 bytes, not `.` or `..`, and made of ASCII letters, digits and
   `. - _ + ~ @ , =` only. Anything else (an empty segment, a `%` escape, a space, a byte above 127, `;` or `:`) gives
   none, and the line is refused: `curl: URL: -O (--remote-name) would save under a name the shell cannot be sure of`,
   hint `to say where it goes, write -o FILE`. curl itself decodes `%` escapes and refuses some names; the shell does
   neither, so a name curl would change is never one the shell approved.
3. **The grant.** For each URL operand, a `create NAME` entry of the current directory with the reason `option -O`,
   through the same designation as an operand of kind `create`: a name derived from a value lies inside the current
   directory by construction, and a literal one needs no `+w`. `grants curl -O https://x/y` shows `~  create or replace y
   (option -O)` beside the network line, and no other line for the current directory (item 5). Only for the image's own
   descriptions (ADR-0153's Decision).
4. **The roots for a POSIX program.** When the shell gives a trusted POSIX program a connection that reaches a host, it adds
   the directory `/system/tls` to its entries, read-only, with the reason `with the network`. It is the directory
   because a POSIX program is given directories only; it holds the roots file and nothing else
   (`make check-tls-roots` checks that). `/system` is read-only to everyone (ADR-0289 item 8), so the program cannot
   change it. It is added for the program's own connection only, never to the shell's table.
5. **The rest of the current directory is not given** (review of 9a, finding 2). The `create NAME` entry reaches the
   program as a directory handle narrowed to that one name (`narrow(name, rights)`, as for a native program), at the
   current directory's prefix in the bootstrap table, and the table's current-directory entry is that one. The
   descriptions of `curl` and `wget` say `cwd none`, so the plan has no `read (current directory)` row to fold the
   narrowed entry into "create and change anything in" (`merge` folds a narrowed part with a whole one of the same path).
   Therefore the program can create or replace the one file, and cannot list, read or create another there (no `.curlrc`,
   no `-d @file` of the directory): a `+r` or `+w` of the line is what gives more. Two rules follow. A line that
   designates nothing in the current directory (`curl URL`, `wget -O -`) starts the program in `/bin`, read only: it has
   no relative path to resolve. A line that designates a path *below* the current directory (`-o sub/f`) and nothing at it
   gets a placeholder at the current directory's prefix whose connection is dead (a channel with its other end closed, since the bootstrap record wants a directory connection there; `zosh`, `posix_entries`): libposix resolves a
   relative path from the current directory's entry and places it by the longest prefix over the whole table
   (`zoraya_resolve::resolve`), so that entry has to supply a prefix, not a readable directory; the program can read
   nothing through it (`curl -o sub/f -d @x` fails without a `+r`; review of 9a, W1). `-` after `-O` of `wget`
   (`wget -O -`) is the standard output, so it designates nothing (`grants`, N5). The alternative, if the
   architect prefers it, is `cwd read` as before: the program then reads the whole directory and `-o`/`-O` give it write
   access to the whole directory; this is what 9a first shipped and what review found to be more than the line says.

## Consequences
- **Positive.** `curl -O` works with the smallest grant that would let it; the description language has one more key,
  for any later program that saves under a URL's name (NetSurf's downloads, CPython's `urllib.request.urlretrieve`).
- **Negative.** `-O` on an unusual URL is refused rather than approximated. A program reading the whole of `/system/tls`
  would see anything added there later; the check says it holds the roots alone.
- **Follow-up.** `url_file_name` cuts the query and fragment before it looks for the path's `/` (`http://h?q/x.txt` has the path `/`). `grants` words the roots as the file for native programs and as the directory for POSIX ones.

## Invariants
- **INV-0311-1.** With `-O` on the line the shell designates exactly the files `url_file_name` gives for the URL operands, in the current directory, and refuses the line when it gives none; the program is given those files and no other of the directory (a narrowed entry, not the directory). *Check:* `cargo test -p zoraya-describe` (`url_file_name`), `cargo test -p zoraya-shell` (`grants_tests`, `curl_o_*`, the narrowed entry), `make test-apps` (`grants curl -O`, a refused name, and `-d @file` failing for a file the line does not give).
- **INV-0311-2.** A trusted POSIX program is given `/system/tls` only together with a connection that reaches a host, and read-only. *Check:* `make test-apps` (`grants curl URL` and `grants curl --help`) and `make test-hand-run`.

## Open questions
- Whether `-J` (the server's `Content-Disposition` name) should ever be offered: the server would choose the file name;
  no.
- `wget`'s default name (no `-O`) is the same case; its description offers `-O FILE` only, and says to give `+w .`.

## Revisit triggers
- A program whose saved name is not derivable from one URL operand.

## Architect's decision
Accepted 2026-11-13. Decided by the architect; recorded by an agent at the architect's request.

Option A (the one-name create entry; `cwd none`). The `cwd read` alternative is refused.
