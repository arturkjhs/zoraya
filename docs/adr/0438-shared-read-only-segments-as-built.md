# ADR-0438: Shared read-only segments as built (plan part F6): an image keeps its program's pages, the creator pays, a program spawns its own image, and one name

- **Status:** proposed
- **Date:** 2026-11-16
- **Depends on:** ADR-0033, ADR-0051, ADR-0102, ADR-0107, ADR-0108, ADR-0261, ADR-0270, ADR-0415, ADR-0431, ADR-0433, ADR-0437
- **Evidence:** CP-14, CP-16

## Context

ADR-0415 item 2 (accepted) says "an image object (ADR-0108) keeps the frames of each segment that is not writable, filled once at
`image_create`; every `spawn` of that image maps those frames read-only instead of copying them", that the image's creator pays
(ADR-0261 point 8), and that a program spawning its own executable "spawns from the image it was started from" (`/proc/self/exe`).
**Read in the code before F6:** `image_create` parsed the headers and kept an `Arc` of the creator's memory object; each `spawn`
copied every page of every segment from that object into fresh frames (`load_segment`), so N children of one image took N copies
of its text and each child's job paid for its copy; the object stayed writable by its creator, so a rewrite of it changed the
children spawned afterwards (ADR-0108's "harms only the child it spawns", INV-0108's test `rewriting_an_image_harms_only_its_children`);
`/proc/self/exe` could not be opened (`EACCES`) and `libposix`'s `posix_spawn` and `execve` read the file again for every spawn.
The ADR leaves open what the sharing is keyed by, who pays when the image's creator and its spawners are different jobs, how the
text stays unwritable and unchanged, what is freed when, and how `top` shows it. The kernel's mapping records (ADR-0437) and the
`SHARED` mapping record that keeps a `fork` child on the same frames (ADR-0431) are already there.

## Decision drivers

1. **Read-only pages are the same bytes in every process of an image, and one copy of them serves all** (ADR-0415 driver 3).
2. **W^X and unwritable text hold for every process** (CP-16): no process can write a shared frame, whatever it holds; and the
   bytes of a running image never change under it, whatever happens to the file or the object it was made from.
3. **Failure at the call, and one job cannot exhaust another** (CP-14, ADR-0261 point 6): the one who makes the copy is the one
   a refusal reaches; a spawner, a child and a `fork` of the child take nothing of the creator's room.
4. **Bounded work under masked locks** (ADR-0270, INV-0270-1): an image of tens of megabytes is made and mapped in steps.
5. **No new authority.** A spawn gives the child what the spawn lists (ADR-0433); sharing text gives it nothing else.

## Considered options

### Option A: the image owns a private copy of its pages; the processes of that image map them

`image_create` copies the program's pages once into one object that no handle names. `spawn` maps the frames of every segment
that is not writable as it is, and copies the pages of the writable ones. Sharing is by the image: its identity is the image
object, and a program reuses it by spawning from the handle it was started from.

### Option B: the Linux baseline, a page cache keyed by file

Shared text is the cache of the file's pages, keyed by (volume, file, version). The kernel would know files (ADR-0002) and keep
the cache's accounting (who pays for a page two jobs map) and its coherence (a changed file) in the kernel.

### Option C: a kernel cache of images keyed by content

`image_create` hashes the bytes and returns the existing image for a match, so any two spawns of equal bytes share. A hash of the
whole program in a masked interval is unbounded work; a collision is a process running another's text; the cached image is paid by
whoever came first, for all.

### Option D: keep the creator's object; share only the frames of the read-only segments *in it*

No copy at `image_create`. The creator's handle may still write the object, so a "read-only" frame can change: it fails driver 2.

### Option E: Zircon's launcher with VMOs: a service holds the images, every spawn maps a read-only clone

The shape ADR-0212's broker cache would take. Not in the kernel and not built; Option A is what such a cache would hand out.

## Recommendation

**Recommendation (not a decision): Option A**, with what follows.

1. **The image's pages.** `image_create(memory, length)` checks the headers as before (INV-0108-1), then makes one memory object of
   `Program::kept()` pages (`zoraya-elf`) in the creator's job and copies into it: for a segment that is not writable *every* page,
   the zero tail past `filesz` included; for a writable one the pages that hold file bytes and none for its zero tail. Segments
   follow one another with no gap. The object is in no handle table; the image holds it, with the validated headers. The copy is
   made in steps of 64 pages with a step point after each (ADR-0270), text pages are made visible to instruction fetches, and the
   call refuses with `OverQuota` or `NoMemory` having taken nothing. **A change from ADR-0108:** what the source object or the file
   holds after the call changes no process started from the image, those already running nor those spawned later; the creator may
   close its object at once, which it had to keep before. *At its peak the call holds the file's object and the copy.*
2. **The spawn.** A segment that is not writable is one mapping record, `SHARED` (ADR-0431), over the image's frames at the
   segment's address, never writable (the kernel refuses a writable record, `map_kept`), executable for text, with one page-table
   entry per page; the child's job pays for the record and the tables, not for a frame. A writable segment gets a page of its own
   for each of its pages, filled from the kept page where the file has bytes. A `fork` child (`snapshot_process`) maps the same
   frames as its parent does, by ADR-0431. The records count toward the 4096 of ADR-0437 (at most 8, one per segment).
3. **Who pays, and what is freed when.** The job that called `image_create` pays the kept object, as a memory object's creator does
   (ADR-0261 point 8, INV-0413-4), for as long as the image handle, any process started from the image or any mapping of it
   lives; the frames go with the last of them, the charge with the frames. Neither the spawner nor a child nor a `fork` child
   is charged for them; a child's job is charged for its own pages and tables alone. A creator at its limit is refused at
   `image_create`; a child at its own limit is refused at the spawn with the creator's room unchanged.
   **What the person sees:** `top` shows a room's memory, which includes the images its jobs made once, however many processes
   run from them. A line of its own for an image, as ADR-0415 item 6's example shows, is **not built**: `top` has rows for
   rooms, and the kernel does not name images.
4. **The key is the image, not the content.** There is no cache in the kernel. Two `image_create`s of one file make two images
   and two copies; the processes of one image share. A program shares by spawning from the image it holds. A launcher or the
   broker's cache (ADR-0212, not built) shares across programs by handing one image handle to every spawn.
5. **A program spawns its own image.** In `libposix`, a `posix_spawn` or `execve` whose path resolves to the `/proc/self/exe`
   entry (`Posix::own_image`, by the resolver, so `//proc/./self/exe` too) uses a duplicate of the bootstrap's `Image` handle:
   no `image_create`, no file read, no file grant, and the child's executable path is the process's own (`readlink` of `/proc/self/exe` in the child gives it). The executable's own path as a file name is *not*
   recognised: the file may have changed since this process started, and it is loaded as before. The child gets what the spawn
   lists (ADR-0433): its table, its fds and its connections are those of any spawn; the image handle it receives is the one it is
   spawned from, in its `/proc/self/exe` entry if the table lists that entry. A table that lists no such entry has no image to
   reuse and falls back to the file.
6. **One name.** `int zoraya_child_memory(pid_t pid, unsigned long long *used)` (`<zoraya/posix.h>`) gives the memory charged to
   the job of the live child `pid`, read through the parent's handle to it (which carries `WAIT`, the right `job_get_usage`
   needs): nothing a parent could not read before, and no handle to a job it did not make. It is how `t_shared` shows the sharing
   from C. Its row is below.

```posix-surface-added
zoraya_child_memory t_shared
```

### Effect of the vision

Target 3 (big programs): a browser's three processes hold one copy of its text in the guest, charged to the room that started it
(which `top` shows once). Target 2: shared text is read-only frames of an image the process was given; it can neither be written
nor changed by anyone, and a spawn of itself needs no file.

### Examples

```
$ /bin/t_shared
t_shared: image keeps at least 548864 bytes; L's job: 1455040, +2176256 for a spawn of the file, +139264 +139264 for two of its image
t_shared: children (own image, file, own image, own image): 1315904 1315904 1315904 1315904 bytes
t_shared: passed
```

(one run under HVF; the test compares the numbers with each other, never with these. The spawn of the file costs the image's pages and a pipe; each spawn of the image costs the pipe alone, and the same each time.)

## Consequences

- **Positive.** N children of one program take one copy of its text; a child's start copies only its data; a running image
  cannot be changed by its file or its creator; the creator's room pays once and every refusal is at a call that holds nothing.
- **Negative.** `image_create` of a large program needs room for the file's object *and* the copy while it runs (twice the
  program for a moment), and takes the time of one copy; a read-only segment's zero tail is committed at `image_create` where it
  was committed at each spawn; two `execve`s of one file by a shell still make two images (item 4); a program that spawns itself
  by its file path, not `/proc/self/exe`, gets no sharing.
- **Follow-up.** The broker's image cache (ADR-0212) to share across programs; a line per image in `top` if the person needs
  it; a program that `fork`s many children (the browser's zygote, ADR-0411) shares through ADR-0431's records already.

## Invariants

- **INV-0438-1.** An image holds its own copy of its program's pages: what the object it was made from holds afterwards, or the
  file, changes no process of the image, one already running or one started later. *Check:* `t_image`'s `rewriting_the_object_after_the_image_changes_nothing`
  (`make test-qemu-one`) and the kernel test `images_share_their_read_only_pages`.
- **INV-0438-2.** The job that made an image pays for its kept pages and the image's record, once, however many processes of any
  jobs run from it; the job of each process pays for its own pages and tables only; a creator that cannot pay gets `OverQuota`
  and holds nothing; a child that cannot pay for its own pages is refused with the creator's use unchanged. *Check:* the kernel
  tests `images_share_their_read_only_pages` and `an_images_pages_are_paid_by_its_creator_alone` (`make test-qemu-one`), and
  `t_shared` (`make test-libposix`: the job of a launcher grows by the image's pages after one spawn of its file and by the same
  small amount after each spawn of its own image).
- **INV-0438-3.** The pages of an image live while its handle, any process started from it or any mapping of it does, and are
  freed, with their charge, when the last of them goes; no process's text is ever freed while it runs. *Check:* the kernel test
  `an_images_pages_outlive_its_handle_and_go_with_its_last_process`, and the frame counts of `t_image`'s leak loop.
- **INV-0438-4.** `image_create` of a program with a large read-only segment, and a `spawn` from the image, each hold no more than
  a step of work between two step points (INV-0270-1). *Check:* the kernel test `an_image_of_a_large_program_is_bounded`
  (`make test-qemu-one`: 2048 pages of shared zero tail).

## Open questions

- Whether `image_create`'s peak (the file's object plus the copy) is a problem for a program of the browser's size in a room's
  limit: the first engine part's measurement (plan E2).
- Whether a read-only segment's zero tail should be committed lazily: ADR-0051 (eager commit) says no.
- The measurement of a program's own pages against its image (what `top`'s line per image would show) is not made.

## Revisit triggers

- The broker's image cache (ADR-0212) is built: the key of item 4 is then its decision.
- A program whose text is mapped by hundreds of processes: the cost of a page-table entry per page and process (a 2 MiB block
  mapping would need aligned runs in the object).

## Architect's decision

(Left empty for the architect.)
