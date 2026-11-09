//! Stage 7e: the chooser's sheet (ADR-0160, ADR-0167, proposed ADR-0245)
//! against the fake of `tests.rs`: only a client the driver manager marked
//! creates a sheet or reports a grant (INV-0160-7, INV-0167-4); the sheet
//! hangs under its owner's title bar, moves and closes with it, takes the
//! keyboard, and keeps the owner's body from the pointer; its strip and the
//! bar's middle carry the system emblem while it is open.

use super::rooms::{axis, done};
use super::*;

/// A client of `app` added by the panel with `tag`.
pub(super) fn tagged(s: &mut Setup, panel: u32, tag: u32, app: &str) -> u32 {
    let end = s.f().channel(64);
    let m = gui::AddRoomClient {
        untrusted: 0,
        colour: 0,
        tag,
        room: zoraya_protocol::Data(b""),
        app: zoraya_protocol::Data(app.as_bytes()),
        entries: zoraya_protocol::Data(b"+w ~/notes"),
        line: zoraya_protocol::Data(app.as_bytes()),
    };
    s.f().send(panel, message(&m, 7), vec![end]);
    s.c.step();
    assert_eq!(done(s, panel), status::OK);
    end
}

/// The chooser: a client the driver manager marks.
pub(super) fn system(s: &mut Setup) -> u32 {
    let end = s.f().channel(64);
    let m = s.manager;
    s.f()
        .send(m, message(&gui::AddSystemClient {}, 9), vec![end]);
    s.c.step();
    assert_eq!(done(s, m), status::OK);
    end
}

pub(super) fn sheet(s: &mut Setup, end: u32, tag: u32, w: u32, h: u32) -> (u32, u32) {
    let k = s.c.scale().factor();
    let n = (w * k * h * k) as usize;
    let memory = s.f().memory(vec![0x00ff_00ff; n], n as u64 * 4, false);
    let m = gui::CreateSheet {
        tag,
        kind: Below(0),
        width: w,
        height: h,
    };
    s.f().send(end, message(&m, 11), vec![memory]);
    s.c.step();
    let out = s.f().take(end);
    let Some((_, r)) = out
        .iter()
        .find_map(|r| decode::<gui::SheetCreated>(r, 0).ok())
    else {
        // Refused as unknown: the protocol's error reply.
        let e = out
            .iter()
            .find_map(|r| decode::<ErrorReply>(r, 0).ok())
            .expect("a reply");
        return (e.1.status, memory);
    };
    if r.status.0 == status::OK {
        let c = gui::CommitTo {
            window: r.window,
            x: 0,
            y: 0,
            width: w * k,
            height: h * k,
        };
        s.f().send(end, message(&c, 7), vec![]);
        s.c.step();
        s.f().take(end);
    }
    (r.status.0, memory)
}

fn bar_has(s: &mut Setup, colour: u32) -> bool {
    let bar = i64::from(space::BAR_HEIGHT.physical(Scale::ONE));
    (0..640).any(|x| s.pixel(x, bar / 2) == colour)
}

/// INV-0160-7, INV-0167-4: a client the driver manager did not mark gets
/// no sheet and changes no record with `Granted`; the panel cannot mark one.
#[test]
fn only_a_marked_client_creates_sheets_and_reports_grants() {
    let mut s = setup();
    let panel = s.panel();
    let app = tagged(&mut s, panel, 4, "edit");
    s.window_of(app, 300, 200, 0x0040_4040);
    let windows = s.c.stack().count();
    let (st, memory) = sheet(&mut s, app, 4, 200, 100);
    assert_eq!(st, status::UNSUPPORTED);
    assert!(s.f().closed.contains(&memory));
    let g = gui::Granted {
        tag: 4,
        words: zoraya_protocol::Data(b"+w /"),
    };
    s.f().send(app, message(&g, 12), vec![]);
    s.c.step();
    s.f().take(app);
    assert_eq!(s.c.stack().count(), windows);
    assert_eq!(s.c.record(0).unwrap().entries, ["+w ~/notes"]);
    // AddSystemClient on the panel's channel is refused.
    let end = s.f().channel(64);
    s.f()
        .send(panel, message(&gui::AddSystemClient {}, 9), vec![end]);
    s.c.step();
    assert_eq!(done(&mut s, panel), status::UNSUPPORTED);
    assert!(s.f().closed.contains(&end));
}

#[test]
fn a_sheet_hangs_under_its_owners_title_bar_and_carries_the_mark() {
    let mut s = setup();
    let panel = s.panel();
    let p = *s.c.appearance().palette();
    let app = tagged(&mut s, panel, 4, "edit");
    let id = s.window_of(app, 400, 260, 0x0040_4040);
    let chooser = system(&mut s);
    let mark = p.bg_frame_system.pixel();
    assert!(!bar_has(&mut s, mark), "no mark before a sheet");
    // An unknown tag, or none: no owner.
    assert_eq!(sheet(&mut s, chooser, 9, 200, 100).0, status::NO_SURFACE);
    assert_eq!(sheet(&mut s, chooser, 0, 200, 100).0, status::NO_SURFACE);
    assert_eq!(sheet(&mut s, chooser, 4, 200, 100).0, status::OK);
    assert_eq!(sheet(&mut s, chooser, 4, 200, 100).0, status::EXISTS);
    let f = s.c.frame();
    let owner = s.c.window(Key { client: 0, id }).unwrap().body_rect();
    let key = s.c.stack().find(|k| k.client == 1).unwrap();
    let w = s.c.window(key).unwrap();
    let body = w.body_rect();
    assert_eq!(body.x, owner.x + (owner.width - 200) / 2);
    assert_eq!(body.y, owner.y - f.border + f.title);
    assert_eq!(w.title, "Open a file for edit");
    // Directly above its owner, with the keyboard.
    let order: Vec<Key> = s.c.stack().collect();
    assert_eq!(order.last(), Some(&key));
    assert_eq!(s.c.focused(), Some(key));
    // The strip's emblem and the bar's.
    let strip_y = body.y - f.title / 2;
    assert_eq!(s.pixel(body.x + 12 + 3, strip_y), mark);
    assert!(bar_has(&mut s, mark), "the bar's middle carries the mark");
    // The strip is above the owner's body line: in its title bar's row.
    assert!(body.y - f.title < owner.y);
    // Closing the sheet takes the mark away and gives the owner focus back.
    s.f()
        .send(chooser, message(&gui::Close { window: key.id }, 10), vec![]);
    s.c.step();
    assert!(!bar_has(&mut s, mark));
    assert_eq!(s.c.focused(), Some(Key { client: 0, id }));
}

#[test]
fn the_owner_gets_no_input_while_its_sheet_is_open_and_moves_it() {
    let mut s = setup();
    let panel = s.panel();
    let app = tagged(&mut s, panel, 4, "edit");
    let id = s.window_of(app, 400, 300, 0x0040_4040);
    let chooser = system(&mut s);
    assert_eq!(sheet(&mut s, chooser, 4, 160, 80).0, status::OK);
    s.f().take(app);
    s.f().take(chooser);
    // A press on the owner's body, beside the sheet: nothing reaches it,
    // and the keyboard stays the sheet's.
    let owner = s.c.window(Key { client: 0, id }).unwrap().body_rect();
    s.press_at(owner.x + 10, owner.bottom() - 10);
    s.keys(&[(30, 1), (30, 0)]);
    let to_app = s.f().take(app);
    assert!(
        events(&to_app).iter().all(|(o, _)| *o != 23 && *o != 16),
        "{:?}",
        events(&to_app)
    );
    assert!(
        !keys_of(&s.f().take(chooser)).is_empty(),
        "keys go to the sheet"
    );
    // System+M hides the owner and its sheet with it.
    s.keys(&[
        (code::RIGHT_META, 1),
        (code::M, 1),
        (code::M, 0),
        (code::RIGHT_META, 0),
    ]);
    assert!(s.c.stack().all(|k| s.c.window(k).unwrap().hidden));
    // Shown again and dragged by its title bar, the sheet follows.
    let key = s.c.stack().find(|k| k.client == 1).unwrap();
    s.c.activate(Key { client: 0, id });
    assert!(!s.c.window(key).unwrap().hidden);
    let before = s.c.window(key).unwrap().body_rect();
    let o = s.outer(0, id);
    let (w, h) = s.f().display_size;
    s.point(axis(o.x + 100, w), axis(o.y + 10, h), Some(1));
    s.point(axis(o.x + 140, w), axis(o.y + 30, h), None);
    s.point(axis(o.x + 140, w), axis(o.y + 30, h), Some(0));
    let after = s.c.window(key).unwrap().body_rect();
    assert_eq!((after.x - before.x, after.y - before.y), (40, 20));
    // Closing the owner closes the sheet and tells the chooser.
    s.f()
        .send(app, message(&gui::Close { window: id }, 10), vec![]);
    s.c.step();
    assert!(s.c.window(key).is_none());
    let told = s.f().take(chooser);
    assert!(
        told.iter()
            .any(|m| decode::<gui::CloseRequested>(m, 0).is_ok_and(|(_, c)| c.window == key.id))
    );
}

#[test]
fn a_grant_from_the_chooser_joins_the_owners_pills() {
    let mut s = setup();
    let panel = s.panel();
    let app = tagged(&mut s, panel, 4, "edit");
    s.window_of(app, 300, 200, 0x0040_4040);
    let chooser = system(&mut s);
    let grant = |s: &mut Setup, tag: u32, words: &str| {
        let g = gui::Granted {
            tag,
            words: zoraya_protocol::Data(words.as_bytes()),
        };
        s.f().send(chooser, message(&g, 12), vec![]);
        s.c.step();
        let out = s.f().take(chooser);
        decode::<gui::GrantedDone>(out.last().unwrap(), 0)
            .unwrap()
            .1
            .status
            .0
    };
    assert_eq!(grant(&mut s, 4, "+r ~/project/plán.txt"), status::OK);
    assert_eq!(grant(&mut s, 4, "+r ~/project/plán.txt"), status::OK);
    assert_eq!(grant(&mut s, 5, "+r ~/x"), status::NO_SURFACE);
    assert_eq!(grant(&mut s, 4, "+r \u{1b}[2J"), status::BAD_RANGE);
    assert_eq!(grant(&mut s, 0, "+r ~/x"), status::BAD_RANGE);
    assert_eq!(
        s.c.record(0).unwrap().entries,
        ["+w ~/notes", "+r ~/project/plán.txt"]
    );
    let pills = &s.c.win_views()[0].pills;
    assert!(pills.iter().any(|p| p == "+r ~/project/plán.txt"));
}
