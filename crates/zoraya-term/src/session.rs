//! The terminal as the window runs it, without the IPC (ADR-0096, ADR-0332,
//! ADR-0385): output goes to the grid, and the person's gestures (a key, the
//! pointer, the wheel, focus, a paste) come out as what is to be sent to the
//! shell. Those are the only places the terminal makes anything to send, so
//! "no output produces input" (INV-0332-2) is a property of this type: the
//! outbox is touched by the gesture methods and by nothing else, and
//! [`Session::output`] cannot reach it.

use alloc::string::String;
use alloc::vec::Vec;

use zoraya_proto_term::paste::{self, MAX_CHUNK, MAX_PASTE_BYTES};

use crate::keys::{KeyModes, MAX_KEY_BYTES, Mods, key_bytes};
use crate::mouse::{Pointer, PointerEvent, focus_bytes};
use crate::term::Term;
use crate::{Look, WheelLines};
use zoraya_keymap::code;

/// What is to be sent to the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outgoing {
    /// Bytes for `Input`: keys, mouse and focus reports.
    Input(Vec<u8>),
    /// A part of a paste, for `Paste`; `last` ends it. Whole characters.
    Paste { bytes: Vec<u8>, last: bool },
}

/// The grid, the pointer and what waits to be sent.
pub struct Session {
    pub term: Term,
    pointer: Pointer,
    wheel: WheelLines,
    /// A wheel's turns not yet a notch, in 120ths.
    notch_rest: i32,
    outbox: Vec<Outgoing>,
}

impl Session {
    pub fn new(term: Term) -> Session {
        Session {
            term,
            pointer: Pointer::default(),
            wheel: WheelLines::default(),
            notch_rest: 0,
            outbox: Vec::new(),
        }
    }

    /// Output from the shell: the grid only. Nothing is sent in answer, ever.
    pub fn output(&mut self, bytes: &[u8]) {
        self.term.write(bytes);
    }

    fn input(&mut self, bytes: Vec<u8>) {
        if !bytes.is_empty() {
            self.outbox.push(Outgoing::Input(bytes));
        }
    }

    /// A key pressed (`zoraya-proto-gui`'s modifier bits). Shift with Page Up
    /// or Page Down moves the view and sends nothing.
    pub fn key(&mut self, key: u32, modifiers: u32, text: Option<char>) {
        let mods = Mods::from_bits(modifiers);
        let half = (self.term.size().1 / 2) as isize;
        let shift_only = mods.shift && !mods.alt && !mods.ctrl;
        match key {
            code::PAGE_UP if shift_only => self.term.scroll_view(half),
            code::PAGE_DOWN if shift_only => self.term.scroll_view(-half),
            _ => {
                let modes = KeyModes {
                    app_cursor: self.term.modes().app_cursor,
                    app_keypad: self.term.modes().app_keypad,
                };
                let mut out = [0u8; MAX_KEY_BYTES];
                let n = key_bytes(key, mods, text, modes, &mut out);
                self.input(out[..n].to_vec());
            }
        }
    }

    /// The pointer over the grid, at a cell, with the buttons held.
    pub fn pointer(&mut self, column: usize, row: usize, buttons: u32, modifiers: u32) {
        let mut out = Vec::new();
        let event = PointerEvent {
            column,
            row,
            buttons,
            mods: Mods::from_bits(modifiers),
        };
        self.pointer.event(&mut self.term, event, &mut out);
        self.input(out);
    }

    /// The wheel over a cell (`dy` in 120ths of a notch, above 0 toward the
    /// person): reported to a program that asked for the mouse, else it
    /// moves the view.
    pub fn wheel(&mut self, column: usize, row: usize, dy: i32, modifiers: u32) {
        let mods = Mods::from_bits(modifiers);
        if self.term.reports_mouse() && !mods.shift {
            let sum = self
                .notch_rest
                .saturating_add(dy)
                .clamp(-64 * 120, 64 * 120);
            self.notch_rest = sum % 120;
            let mut out = Vec::new();
            self.pointer
                .wheel(&self.term, column, row, sum / 120, mods, &mut out);
            self.input(out);
        } else {
            let lines = self.wheel.turn(dy);
            if lines != 0 {
                self.term.scroll_view(lines);
            }
        }
    }

    /// The window gained or lost the keyboard's focus.
    pub fn focus(&mut self, focused: bool) {
        let bytes = focus_bytes(&self.term, focused);
        self.input(bytes.to_vec());
    }

    /// The person pasted (`Paste` of the compositor): the text, in parts of at
    /// most [`MAX_CHUNK`] bytes each a whole number of characters, at most
    /// [`MAX_PASTE_BYTES`] in all; the shell cleans it. Nothing for an empty
    /// text.
    pub fn paste(&mut self, text: &[u8]) {
        let text = String::from_utf8_lossy(text);
        let mut rest = &text[..paste::boundary(&text, MAX_PASTE_BYTES)];
        while !rest.is_empty() {
            let mut at = paste::boundary(rest, MAX_CHUNK);
            if at == 0 {
                // Cannot be: a character is at most 4 bytes.
                at = rest.chars().next().map_or(0, char::len_utf8);
            }
            let (head, tail) = rest.split_at(at);
            self.outbox.push(Outgoing::Paste {
                bytes: head.as_bytes().to_vec(),
                last: tail.is_empty(),
            });
            rest = tail;
        }
    }

    /// The selected text, at most `limit` bytes of whole characters, for the
    /// clipboard (`Offer`); none when nothing is selected.
    pub fn copy(&self, look: &Look, limit: usize) -> Option<String> {
        let text = self.term.selected_text(look);
        if text.is_empty() {
            return None;
        }
        let at = paste::boundary(&text, limit);
        Some(String::from(&text[..at]))
    }

    /// What waits to be sent, in order.
    pub fn take_outbox(&mut self) -> Vec<Outgoing> {
        core::mem::take(&mut self.outbox)
    }

    pub fn outbox_is_empty(&self) -> bool {
        self.outbox.is_empty()
    }
}
