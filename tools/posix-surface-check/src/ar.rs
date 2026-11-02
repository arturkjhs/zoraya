//! A reader of GNU `ar` archives, this tool's own (ADR-0100 item 7): it shares
//! no code with `zoraya-ar`, which writes them, so that the check does not
//! inherit the writer's mistakes. Total: any input gives a result.

const MAGIC: &[u8] = b"!<arch>\n";
const HEADER: usize = 60;

/// One ordinary member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member<'a> {
    pub name: String,
    pub data: &'a [u8],
}

fn text(b: &[u8]) -> Result<&str, String> {
    core::str::from_utf8(b)
        .map(|s| s.trim_end_matches(' '))
        .map_err(|_| "a header field is not text".to_string())
}

/// The members of `archive`, without the index (`/`, `/SYM64/`) and the name
/// table (`//`), in archive order.
pub fn members(archive: &[u8]) -> Result<Vec<Member<'_>>, String> {
    if !archive.starts_with(MAGIC) {
        return Err("not an ar archive".into());
    }
    let mut at = MAGIC.len();
    let mut table: &[u8] = &[];
    let mut out = Vec::new();
    while at < archive.len() {
        let h = archive
            .get(at..at.checked_add(HEADER).ok_or("overflow")?)
            .ok_or(format!("truncated header at {at}"))?;
        if &h[58..60] != b"`\n" {
            return Err(format!("bad header at {at}"));
        }
        let size: usize = text(&h[48..58])?
            .parse()
            .map_err(|_| format!("bad size at {at}"))?;
        let start = at + HEADER;
        let end = start.checked_add(size).ok_or("overflow")?;
        let data = archive
            .get(start..end)
            .ok_or(format!("member at {at} runs past the end"))?;
        let raw = text(&h[..16])?;
        match raw {
            "/" | "/SYM64/" => {}
            "//" => table = data,
            _ => {
                let name = match raw.strip_prefix('/') {
                    Some(n) => {
                        let off: usize = n.parse().map_err(|_| format!("bad name at {at}"))?;
                        let rest = table.get(off..).ok_or(format!("bad name offset at {at}"))?;
                        let len = rest
                            .windows(2)
                            .position(|w| w == b"/\n")
                            .ok_or(format!("unterminated long name at {at}"))?;
                        String::from_utf8(rest[..len].to_vec())
                            .map_err(|_| format!("long name at {at} is not text"))?
                    }
                    None => raw.strip_suffix('/').unwrap_or(raw).to_string(),
                };
                out.push(Member { name, data });
            }
        }
        at = end + (size & 1);
    }
    Ok(out)
}
