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

/// A sheet, and the clock past its guard (ADR-0245), as a person's next
/// key would find it.
pub(super) fn sheet(s: &mut Setup, end: u32, tag: u32, w: u32, h: u32) -> (u32, u32) {
    let r = sheet_at_once(s, end, tag, w, h);
    s.f().clock += SHEET_GUARD_NS;
    r
}

/// A sheet, with the clock where it mapped.
pub(super) fn sheet_at_once(s: &mut Setup, end: u32, tag: u32, w: u32, h: u32) -> (u32, u32) {
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

/// The chooser's sheet, if one is open: its key and its owner.
fn open_sheet(s: &Setup, chooser_slot: usize) -> Option<(Key, (usize, u32))> {
    let key = s.c.stack().find(|k| k.client == chooser_slot)?;
    Some((key, s.c.window(key)?.owner()?))
}

fn popup(s: &mut Setup, end: u32, parent: u32) -> u32 {
    let m = s.f().memory(vec![7; 20 * 10], 800, false);
    let out = s.request(
        end,
        &gui::CreatePopup {
            parent,
            x: 10,
            y: 20,
            width: 20,
            height: 10,
        },
        vec![m],
    );
    let (_, p) = out
        .iter()
        .find_map(|m| decode::<gui::PopupCreated>(m, 0).ok())
        .unwrap();
    assert_eq!(p.status.0, status::OK);
    p.window
}

/// A sheet is refused when it has no width, when it is taller than the
/// screen below a strip, and when the chooser's buffers would pass four
/// displays' worth of pixels.
#[test]
fn a_sheet_is_refused_when_too_wide_too_tall_or_over_the_budget() {
    let mut s = setup();
    let panel = s.panel();
    let mut apps = Vec::new();
    for tag in 4..9 {
        let app = tagged(&mut s, panel, tag, "edit");
        s.window_of(app, 200, 100, 0x0040_4040);
        apps.push(app);
    }
    let chooser = system(&mut s);
    let room = H - s.c.frame().title as u32;
    assert_eq!(sheet(&mut s, chooser, 4, 0, 100).0, status::BAD_BUFFER);
    assert_eq!(
        sheet(&mut s, chooser, 4, 200, room + 1).0,
        status::BAD_BUFFER
    );
    assert_eq!(sheet(&mut s, chooser, 4, W + 1, 100).0, status::BAD_BUFFER);
    // Four of the largest fit the budget, a fifth does not.
    for tag in 4..8 {
        assert_eq!(sheet(&mut s, chooser, tag, W, room).0, status::OK, "{tag}");
    }
    assert_eq!(sheet(&mut s, chooser, 8, W, room).0, status::BAD_BUFFER);
}

/// The sheet hangs under the focused window of the tagged client, else
/// under its topmost toplevel; never under another client's window.
#[test]
fn a_sheet_hangs_under_the_focused_window_of_its_tag() {
    // Another client focused: the tagged client's topmost window.
    let mut s = setup();
    let panel = s.panel();
    let a = tagged(&mut s, panel, 4, "edit");
    let ida = s.window_of(a, 300, 200, 0x0040_4040);
    let b = tagged(&mut s, panel, 5, "files");
    let idb = s.window_of(b, 300, 200, 0x0060_6060);
    s.c.activate(Key { client: 1, id: idb });
    let chooser = system(&mut s);
    assert_eq!(sheet(&mut s, chooser, 4, 160, 80).0, status::OK);
    assert_eq!(open_sheet(&s, 2).map(|(_, o)| o), Some((0, ida)));
    // The tagged client's focused window, though another of its own is
    // above it (a new window takes no focus).
    let mut s = setup();
    let panel = s.panel();
    let a = tagged(&mut s, panel, 4, "edit");
    let first = s.window_of(a, 300, 200, 0x0040_4040);
    let second = s.window_of(a, 300, 200, 0x0050_5050);
    assert_ne!(first, second);
    assert_eq!(
        s.c.focused(),
        Some(Key {
            client: 0,
            id: first
        })
    );
    let chooser = system(&mut s);
    assert_eq!(sheet(&mut s, chooser, 4, 160, 80).0, status::OK);
    assert_eq!(open_sheet(&s, 1).map(|(_, o)| o), Some((0, first)));
}

/// A new sheet takes the keyboard from its owner or the owner's popup,
/// never from another client's window.
#[test]
fn a_sheet_takes_the_focus_only_from_its_owners_client() {
    let mut s = setup();
    let panel = s.panel();
    let a = tagged(&mut s, panel, 4, "edit");
    s.window_of(a, 300, 200, 0x0040_4040);
    let b = tagged(&mut s, panel, 5, "files");
    let idb = s.window_of(b, 300, 200, 0x0060_6060);
    let other = Key { client: 1, id: idb };
    s.c.activate(other);
    let chooser = system(&mut s);
    assert_eq!(sheet(&mut s, chooser, 4, 160, 80).0, status::OK);
    assert_eq!(s.c.focused(), Some(other));
    // The owner's popup focused: the sheet takes it.
    let mut s = setup();
    let panel = s.panel();
    let a = tagged(&mut s, panel, 4, "edit");
    let id = s.window_of(a, 300, 200, 0x0040_4040);
    let p = popup(&mut s, a, id);
    let body = s.c.window(Key { client: 0, id: p }).unwrap().body_rect();
    s.press_at(body.x + 2, body.y + 2);
    assert_eq!(s.c.focused(), Some(Key { client: 0, id: p }));
    let chooser = system(&mut s);
    assert_eq!(sheet(&mut s, chooser, 4, 160, 80).0, status::OK);
    let (key, _) = open_sheet(&s, 1).unwrap();
    assert_eq!(s.c.focused(), Some(key));
}

/// The sheet goes directly above its owner's popups.
#[test]
fn a_sheet_stacks_above_its_owners_popups() {
    let mut s = setup();
    let panel = s.panel();
    let a = tagged(&mut s, panel, 4, "edit");
    let id = s.window_of(a, 300, 200, 0x0040_4040);
    let p = popup(&mut s, a, id);
    let chooser = system(&mut s);
    assert_eq!(sheet(&mut s, chooser, 4, 160, 80).0, status::OK);
    let (key, _) = open_sheet(&s, 1).unwrap();
    let order: Vec<Key> = s.c.stack().collect();
    assert_eq!(
        order,
        [Key { client: 0, id }, Key { client: 0, id: p }, key]
    );
}

/// The sheet's strip moves nothing, and an owner with a sheet open takes
/// no press but on its title bar: its close button does nothing.
#[test]
fn a_sheet_strip_takes_no_press_and_an_owner_with_a_sheet_takes_only_its_title() {
    let mut s = setup();
    let panel = s.panel();
    let app = tagged(&mut s, panel, 4, "edit");
    let id = s.window_of(app, 400, 300, 0x0040_4040);
    let chooser = system(&mut s);
    assert_eq!(sheet(&mut s, chooser, 4, 160, 80).0, status::OK);
    let (key, _) = open_sheet(&s, 1).unwrap();
    let before = s.c.window(key).unwrap().body_rect();
    let f = s.c.frame();
    let (w, h) = s.f().display_size;
    let (x, y) = (before.x + 40, before.y - f.title / 2);
    s.point(axis(x, w), axis(y, h), Some(1));
    s.point(axis(x + 40, w), axis(y + 30, h), None);
    s.point(axis(x + 40, w), axis(y + 30, h), Some(0));
    assert_eq!(
        s.c.window(key).unwrap().body_rect(),
        before,
        "the strip moves nothing"
    );
    s.f().take(app);
    let o = s.outer(0, id);
    let cy = o.y + f.border + f.title / 2;
    s.press_at(o.x + i64::from(window::CLOSE_X), cy);
    let told = s.f().take(app);
    assert!(
        !told
            .iter()
            .any(|m| decode::<gui::CloseRequested>(m, 0).is_ok()),
        "the owner's close button is not pressed while its sheet is open"
    );
    assert!(s.c.window(Key { client: 0, id }).is_some());
}

/// The sheet's outline is its strip and its body: a press just above the
/// body's bottom is the sheet's, and below its bottom it is not.
#[test]
fn a_press_at_the_sheet_edge_is_the_sheets_and_just_below_it_not() {
    let mut s = setup();
    let panel = s.panel();
    let app = tagged(&mut s, panel, 4, "edit");
    let ida = s.window_of(app, 300, 200, 0x0040_4040);
    let other = tagged(&mut s, panel, 5, "files");
    let idc = s.window_of(other, 300, 200, 0x0060_6060);
    let chooser = system(&mut s);
    assert_eq!(sheet(&mut s, chooser, 4, 160, 80).0, status::OK);
    let (key, _) = open_sheet(&s, 2).unwrap();
    // The owner and its sheet raised above the other window.
    s.c.activate(Key { client: 0, id: ida });
    let body = s.c.window(key).unwrap().body_rect();
    s.f().take(chooser);
    let (w, h) = s.f().display_size;
    s.point(axis(body.x + 10, w), axis(body.bottom() - 5, h), None);
    assert!(
        events(&s.f().take(chooser)).iter().any(|(o, _)| *o == 23),
        "the sheet's body takes the pointer"
    );
    // Far below the sheet, where no window is, a press changes nothing.
    let c = Key { client: 1, id: idc };
    s.c.activate(c);
    let below = s.outer(1, idc).bottom() + 40;
    assert!(below < i64::from(H) - 10);
    s.press_at(body.x + 5, below);
    assert_eq!(s.c.focused(), Some(c));
}

/// Closing a focused sheet gives the focus back to its owner, though
/// another window is above them.
#[test]
fn closing_a_sheet_focuses_its_owner_under_another_window() {
    let mut s = setup();
    let panel = s.panel();
    let a = tagged(&mut s, panel, 4, "edit");
    let id = s.window_of(a, 300, 200, 0x0040_4040);
    let b = tagged(&mut s, panel, 5, "files");
    s.window_of(b, 300, 200, 0x0060_6060);
    let chooser = system(&mut s);
    assert_eq!(sheet(&mut s, chooser, 4, 160, 80).0, status::OK);
    let (key, _) = open_sheet(&s, 2).unwrap();
    assert_eq!(s.c.focused(), Some(key));
    assert_eq!(s.c.stack().last().map(|k| k.client), Some(1));
    s.f()
        .send(chooser, message(&gui::Close { window: key.id }, 10), vec![]);
    s.c.step();
    assert_eq!(s.c.focused(), Some(Key { client: 0, id }));
}

/// A press on a sheet raises its owner with it, above another window.
#[test]
fn a_press_on_a_sheet_raises_its_owner_with_it() {
    let mut s = setup();
    let panel = s.panel();
    let a = tagged(&mut s, panel, 4, "edit");
    let id = s.window_of(a, 500, 300, 0x0040_4040);
    let chooser = system(&mut s);
    assert_eq!(sheet(&mut s, chooser, 4, 160, 80).0, status::OK);
    let b = tagged(&mut s, panel, 5, "files");
    let idb = s.window_of(b, 100, 60, 0x0060_6060);
    let (key, _) = open_sheet(&s, 1).unwrap();
    let ob = s.outer(2, idb);
    let body = s.c.window(key).unwrap().body_rect();
    let (x, y) = (body.x + 3, body.y + 3);
    assert!(!ob.contains(x, y), "{body:?} {ob:?}");
    s.press_at(x, y);
    assert_eq!(
        s.c.stack().collect::<Vec<_>>(),
        [Key { client: 2, id: idb }, Key { client: 0, id }, key]
    );
}

/// ADR-0245's condition (1): for 500 ms after it maps a sheet takes no key
/// and no button, so a key timed to meet the request does not confirm it.
#[test]
fn a_new_sheet_ignores_input_for_500_ms() {
    let mut s = setup();
    let panel = s.panel();
    let app = tagged(&mut s, panel, 4, "edit");
    s.window_of(app, 400, 300, 0x0040_4040);
    let chooser = system(&mut s);
    assert_eq!(sheet_at_once(&mut s, chooser, 4, 160, 80).0, status::OK);
    s.f().take(chooser);
    s.f().take(app);
    // Enter at once: to no one.
    s.keys(&[(code::ENTER, 1), (code::ENTER, 0)]);
    assert!(keys_of(&s.f().take(chooser)).is_empty());
    assert!(keys_of(&s.f().take(app)).is_empty());
    // A press on the sheet's body: no button reaches it.
    let sheet = s.c.stack().find(|k| k.client == 1).unwrap();
    let body = s.c.window(sheet).unwrap().body_rect();
    s.press_at(body.x + 20, body.y + 20);
    let presses: Vec<u32> = s
        .f()
        .take(chooser)
        .iter()
        .filter_map(|m| decode::<gui::WindowPointer>(m, 0).ok())
        .map(|(_, p)| p.buttons.0)
        .collect();
    assert!(presses.iter().all(|b| *b == 0), "{presses:?}");
    // Just before the end, still nothing; after it, keys arrive.
    s.f().clock += SHEET_GUARD_NS - 50_000_000;
    s.keys(&[(code::ENTER, 1), (code::ENTER, 0)]);
    assert!(keys_of(&s.f().take(chooser)).is_empty());
    s.f().clock += 60_000_000;
    s.keys(&[(code::ENTER, 1), (code::ENTER, 0)]);
    assert!(!keys_of(&s.f().take(chooser)).is_empty());
}

/// ADR-0245's condition (1): a key held down before the sheet mapped
/// (its repeats included) never reaches it; once released, the key is the
/// sheet's again. A button held from before likewise.
#[test]
fn a_key_held_from_before_the_sheet_never_reaches_it() {
    let mut s = setup();
    let panel = s.panel();
    let app = tagged(&mut s, panel, 4, "edit");
    s.window_of(app, 400, 300, 0x0040_4040);
    let chooser = system(&mut s);
    // Enter goes down in the editor; the sheet maps under it.
    s.keys(&[(code::ENTER, 1)]);
    assert_eq!(sheet_at_once(&mut s, chooser, 4, 160, 80).0, status::OK);
    s.f().take(chooser);
    s.f().clock += SHEET_GUARD_NS * 2;
    // Repeats of the held Enter, long after: not the sheet's.
    s.keys(&[(code::ENTER, 2), (code::ENTER, 2)]);
    assert!(keys_of(&s.f().take(chooser)).is_empty());
    // Released, then pressed again: the sheet's.
    s.keys(&[(code::ENTER, 0)]);
    s.keys(&[(code::ENTER, 1), (code::ENTER, 0)]);
    let keys = keys_of(&s.f().take(chooser));
    assert_eq!(
        keys.iter().filter(|(_, pressed, _)| *pressed).count(),
        1,
        "{keys:?}"
    );
    // Another key that was not held is the sheet's at once.
    s.keys(&[(30, 1), (30, 0)]);
    assert!(!keys_of(&s.f().take(chooser)).is_empty());
}

#[test]
fn a_button_held_from_before_the_sheet_reaches_it_only_after_release() {
    let mut s = setup();
    let panel = s.panel();
    let app = tagged(&mut s, panel, 4, "edit");
    s.window_of(app, 400, 300, 0x0040_4040);
    let chooser = system(&mut s);
    assert_eq!(sheet_at_once(&mut s, chooser, 4, 160, 80).0, status::OK);
    let sheet = s.c.stack().find(|k| k.client == 1).unwrap();
    let body = s.c.window(sheet).unwrap().body_rect();
    let (w, h) = s.f().display_size;
    let (x, y) = (axis(body.x + 20, w), axis(body.y + 20, h));
    // Pressed within the guard and held past it: never a press.
    s.point(x, y, Some(1));
    s.f().clock += SHEET_GUARD_NS * 2;
    s.point(axis(body.x + 22, w), y, None);
    let buttons = |s: &mut Setup| -> Vec<u32> {
        s.f()
            .take(chooser)
            .iter()
            .filter_map(|m| decode::<gui::WindowPointer>(m, 0).ok())
            .map(|(_, p)| p.buttons.0)
            .collect()
    };
    let held = buttons(&mut s);
    assert!(!held.is_empty() && held.iter().all(|b| *b == 0), "{held:?}");
    // Released and pressed again: the sheet's press.
    s.point(x, y, Some(0));
    s.point(x, y, Some(1));
    assert!(buttons(&mut s).contains(&gui::buttons::LEFT));
}

/// ADR-0245's condition (2): System+? tells the chooser, and only system
/// clients, that the show-commands mode is on or off.
#[test]
fn the_chooser_hears_the_show_commands_mode() {
    let mut s = setup();
    let panel = s.panel();
    let app = tagged(&mut s, panel, 4, "edit");
    s.window_of(app, 400, 300, 0x0040_4040);
    let chooser = system(&mut s);
    s.f().take(app);
    s.f().take(chooser);
    let chord = [
        (code::RIGHT_META, 1),
        (code::SLASH, 1),
        (code::SLASH, 0),
        (code::RIGHT_META, 0),
    ];
    let told = |s: &mut Setup, end: u32| -> Vec<bool> {
        s.f()
            .take(end)
            .iter()
            .filter_map(|m| decode::<gui::Commands>(m, 0).ok())
            .map(|(_, c)| c.on)
            .collect()
    };
    s.keys(&chord);
    assert_eq!(told(&mut s, chooser), [true]);
    assert!(told(&mut s, app).is_empty(), "not an application");
    // A system client added while the mode is on hears it at once.
    let late = system(&mut s);
    assert_eq!(told(&mut s, late), [true]);
    s.keys(&chord);
    assert_eq!(told(&mut s, chooser), [false]);
}
