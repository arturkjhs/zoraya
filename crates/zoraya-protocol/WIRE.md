# The wire format (ADR-0052)

All integers are little-endian. A message is at most 65,536 bytes.

**Header, 16 bytes.** `u32` magic and version `0x5a500001`, `u32` ordinal, `u32` transaction id (0: no reply is
expected), `u32` size of the body. Ordinal 0 is the generic error reply: one `u32` status.

**Body.** Fields in declared order, each at its natural alignment from the start of the body. Padding is zero, and is
checked.

| Kind | Bytes |
|---|---|
| `u8`, `u16`, `u32`, `u64` | themselves |
| `bool` | one byte, 0 or 1 |
| `Flags<MASK>` | `u32`, with no bit outside `MASK` |
| `Below<N>` | `u32`, less than `N`: an enumeration |
| `Data<N>` | `u32` length of at most `N`, then the bytes |
| `Text<N>` | as `Data`, valid UTF-8 |
| `Path<N, EMPTY>` | as `Text`, components split at `/` each a `Name`, and empty only if `EMPTY` |
| `Name` | as `Text<255>`, a path component as ADR-0013 allows |
| `Seq<T, N>` | `u32` count of at most `N`, padding to 8, `u64` size in bytes, then the records |

Each record starts at 8-byte alignment from the start of the records, with its fields as a body's. The records end
exactly at the size.

No bytes follow the last field. Handles travel in the channel message's list, not in these bytes.
