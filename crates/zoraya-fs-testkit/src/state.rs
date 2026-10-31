//! The observable state of a volume: every named object by path, with its
//! attributes and content. Extracted from the engine through its public
//! request API only, and from the model directly.

use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use zoraya_fs::{BlockDevice, Clock, FsError, ObjKind, OpenOptions, Timestamp, Volume};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjState {
    pub id: u64,
    pub kind: ObjKind,
    pub mode: u16,
    pub mtime: Timestamp,
    pub ctime: Timestamp,
    pub size: u64,
    /// File data or link target; empty for a directory.
    pub content: Vec<u8>,
}

/// Path (`""` for the root) to object.
pub type FsState = BTreeMap<String, ObjState>;

pub fn hash_state(s: &FsState) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// Reads the whole namespace through the request API.
pub fn engine_state<D: BlockDevice, C: Clock>(v: &mut Volume<D, C>) -> Result<FsState, FsError> {
    let mut out = FsState::new();
    let root = v.root();
    let r = collect(v, root, String::new(), &mut out);
    // On a `Failed` volume close reports `StorageFailed` after releasing the
    // handle; reading is still allowed, so that is not an error here.
    let _ = v.close(root);
    r?;
    Ok(out)
}

fn collect<D: BlockDevice, C: Clock>(
    v: &mut Volume<D, C>,
    h: zoraya_fs::Handle,
    path: String,
    out: &mut FsState,
) -> Result<(), FsError> {
    let a = v.stat(h)?;
    let content = match a.kind {
        ObjKind::File => {
            let mut buf = vec![0u8; a.size as usize];
            let n = v.read(h, 0, &mut buf)?;
            buf.truncate(n);
            buf
        }
        ObjKind::Link => v.readlink(h)?.into_bytes(),
        ObjKind::Directory => Vec::new(),
    };
    out.insert(
        path.clone(),
        ObjState {
            id: a.id,
            kind: a.kind,
            mode: a.posix_mode,
            mtime: a.mtime,
            ctime: a.ctime,
            size: a.size,
            content,
        },
    );
    if a.kind != ObjKind::Directory {
        return Ok(());
    }
    let mut after: Option<String> = None;
    while let Some(e) = v.readdir(h, after.as_deref())? {
        let child = v.open(
            h,
            &e.name,
            OpenOptions {
                link_itself: true,
                ..OpenOptions::default()
            },
        )?;
        let p = if path.is_empty() {
            e.name.clone()
        } else {
            format!("{path}/{}", e.name)
        };
        let r = collect(v, child, p, out);
        let _ = v.close(child);
        r?;
        after = Some(e.name);
    }
    Ok(())
}
