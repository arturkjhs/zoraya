//! ADR-0163's layout tables: `us`, `ua` (xkeyboard-config's "unicode"),
//! `ru` ("winkeys") and `cz` (QWERTZ "basic"), written from the reference
//! files as `docs/research/desktop/survey.md#keyboard-layouts` describes
//! them: per key code four levels (plain, Shift, AltGr, Shift+AltGr), each a
//! character, a dead key or nothing. The level 3 and 4 symbols other than
//! Czech's and the Ukrainian ґ and backslash are left out (`None`): nothing
//! in stage 7 needs them, and a table that is shorter than
//! xkeyboard-config's is more honest than a guessed one (the task report
//! lists them). Caps Lock acts on case pairs only, so it does not capitalise
//! Czech's number-row letters as XKB's semi-alphabetic type does.

use crate::Accent::{self, *};

/// What one level of one key gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Out {
    Char(char),
    Dead(Accent),
    None,
}

/// A layout: its name in the settings (ADR-0169), the short form the bar
/// shows, and its keys.
#[derive(Debug)]
pub struct Layout {
    pub name: &'static str,
    pub short: &'static str,
    pub keys: &'static [(u32, [Out; 4])],
}

impl Layout {
    /// The four levels of a key code, `None` for a key with no row.
    pub fn levels(&self, code: u32) -> [Out; 4] {
        self.keys
            .iter()
            .find(|(c, _)| *c == code)
            .map_or([Out::None; 4], |(_, l)| *l)
    }

    /// Whether any key has a level 3 or 4: then the right Alt is AltGr.
    pub fn has_altgr(&self) -> bool {
        self.keys
            .iter()
            .any(|(_, l)| l[2] != Out::None || l[3] != Out::None)
    }
}

const N: Out = Out::None;

const fn c(ch: char) -> Out {
    Out::Char(ch)
}

const fn d(a: Accent) -> Out {
    Out::Dead(a)
}

/// A key of two levels.
const fn k2(code: u32, a: char, b: char) -> (u32, [Out; 4]) {
    (code, [c(a), c(b), N, N])
}

/// A key of four levels.
const fn k4(code: u32, l: [Out; 4]) -> (u32, [Out; 4]) {
    (code, l)
}

/// Every layout, in the order of their names.
pub static LAYOUTS: [Layout; 4] = [US, UA, RU, CZ];

/// The layout called `name`.
pub fn by_name(name: &str) -> Option<usize> {
    LAYOUTS.iter().position(|l| l.name == name)
}

pub const US: Layout = Layout {
    name: "us",
    short: "US",
    keys: &[
        k2(41, '`', '~'),
        k2(2, '1', '!'),
        k2(3, '2', '@'),
        k2(4, '3', '#'),
        k2(5, '4', '$'),
        k2(6, '5', '%'),
        k2(7, '6', '^'),
        k2(8, '7', '&'),
        k2(9, '8', '*'),
        k2(10, '9', '('),
        k2(11, '0', ')'),
        k2(12, '-', '_'),
        k2(13, '=', '+'),
        k2(16, 'q', 'Q'),
        k2(17, 'w', 'W'),
        k2(18, 'e', 'E'),
        k2(19, 'r', 'R'),
        k2(20, 't', 'T'),
        k2(21, 'y', 'Y'),
        k2(22, 'u', 'U'),
        k2(23, 'i', 'I'),
        k2(24, 'o', 'O'),
        k2(25, 'p', 'P'),
        k2(26, '[', '{'),
        k2(27, ']', '}'),
        k2(30, 'a', 'A'),
        k2(31, 's', 'S'),
        k2(32, 'd', 'D'),
        k2(33, 'f', 'F'),
        k2(34, 'g', 'G'),
        k2(35, 'h', 'H'),
        k2(36, 'j', 'J'),
        k2(37, 'k', 'K'),
        k2(38, 'l', 'L'),
        k2(39, ';', ':'),
        k2(40, '\'', '"'),
        k2(43, '\\', '|'),
        k2(44, 'z', 'Z'),
        k2(45, 'x', 'X'),
        k2(46, 'c', 'C'),
        k2(47, 'v', 'V'),
        k2(48, 'b', 'B'),
        k2(49, 'n', 'N'),
        k2(50, 'm', 'M'),
        k2(51, ',', '<'),
        k2(52, '.', '>'),
        k2(53, '/', '?'),
    ],
};

/// The number row of `ua` and `ru`: digits, and the Windows arrangement of
/// punctuation with Shift.
macro_rules! cyrillic_digits {
    () => {
        [
            k2(2, '1', '!'),
            k2(3, '2', '"'),
            k2(4, '3', '№'),
            k2(5, '4', ';'),
            k2(6, '5', '%'),
            k2(7, '6', ':'),
            k2(8, '7', '?'),
            k2(9, '8', '*'),
            k2(10, '9', '('),
            k2(11, '0', ')'),
            k2(12, '-', '_'),
            k2(13, '=', '+'),
        ]
    };
}

const UA_DIGITS: [(u32, [Out; 4]); 12] = cyrillic_digits!();
const RU_DIGITS: [(u32, [Out; 4]); 12] = cyrillic_digits!();

const UA_KEYS: [(u32, [Out; 4]); 47] = {
    let mut k = [(0, [N; 4]); 47];
    let mut i = 0;
    while i < 12 {
        k[i] = UA_DIGITS[i];
        i += 1;
    }
    let rest: [(u32, [Out; 4]); 35] = [
        // The apostrophe left of 1, U+02BC with Shift.
        k2(41, '\'', 'ʼ'),
        k2(16, 'й', 'Й'),
        k2(17, 'ц', 'Ц'),
        k2(18, 'у', 'У'),
        k2(19, 'к', 'К'),
        k2(20, 'е', 'Е'),
        k2(21, 'н', 'Н'),
        // ґ also on AltGr with г.
        k4(22, [c('г'), c('Г'), c('ґ'), c('Ґ')]),
        k2(23, 'ш', 'Ш'),
        k2(24, 'щ', 'Щ'),
        k2(25, 'з', 'З'),
        k2(26, 'х', 'Х'),
        k2(27, 'ї', 'Ї'),
        k2(30, 'ф', 'Ф'),
        k2(31, 'і', 'І'),
        k2(32, 'в', 'В'),
        k2(33, 'а', 'А'),
        k2(34, 'п', 'П'),
        k2(35, 'р', 'Р'),
        k2(36, 'о', 'О'),
        k2(37, 'л', 'Л'),
        k2(38, 'д', 'Д'),
        k2(39, 'ж', 'Ж'),
        k2(40, 'є', 'Є'),
        // ґ on the key right of ], the backslash on AltGr.
        k4(43, [c('ґ'), c('Ґ'), c('\\'), c('|')]),
        k2(44, 'я', 'Я'),
        k2(45, 'ч', 'Ч'),
        k2(46, 'с', 'С'),
        k2(47, 'м', 'М'),
        k2(48, 'и', 'И'),
        k2(49, 'т', 'Т'),
        k2(50, 'ь', 'Ь'),
        k2(51, 'б', 'Б'),
        k2(52, 'ю', 'Ю'),
        k2(53, '.', ','),
    ];
    let mut j = 0;
    while j < 35 {
        k[12 + j] = rest[j];
        j += 1;
    }
    k
};

pub const UA: Layout = Layout {
    name: "ua",
    short: "UA",
    keys: &UA_KEYS,
};

const RU_KEYS: [(u32, [Out; 4]); 47] = {
    let mut k = [(0, [N; 4]); 47];
    let mut i = 0;
    while i < 12 {
        k[i] = RU_DIGITS[i];
        i += 1;
    }
    let rest: [(u32, [Out; 4]); 35] = [
        k2(41, 'ё', 'Ё'),
        k2(16, 'й', 'Й'),
        k2(17, 'ц', 'Ц'),
        k2(18, 'у', 'У'),
        k2(19, 'к', 'К'),
        k2(20, 'е', 'Е'),
        k2(21, 'н', 'Н'),
        k2(22, 'г', 'Г'),
        k2(23, 'ш', 'Ш'),
        k2(24, 'щ', 'Щ'),
        k2(25, 'з', 'З'),
        k2(26, 'х', 'Х'),
        k2(27, 'ъ', 'Ъ'),
        k2(30, 'ф', 'Ф'),
        k2(31, 'ы', 'Ы'),
        k2(32, 'в', 'В'),
        k2(33, 'а', 'А'),
        k2(34, 'п', 'П'),
        k2(35, 'р', 'Р'),
        k2(36, 'о', 'О'),
        k2(37, 'л', 'Л'),
        k2(38, 'д', 'Д'),
        k2(39, 'ж', 'Ж'),
        k2(40, 'э', 'Э'),
        k2(43, '\\', '/'),
        k2(44, 'я', 'Я'),
        k2(45, 'ч', 'Ч'),
        k2(46, 'с', 'С'),
        k2(47, 'м', 'М'),
        k2(48, 'и', 'И'),
        k2(49, 'т', 'Т'),
        k2(50, 'ь', 'Ь'),
        k2(51, 'б', 'Б'),
        k2(52, 'ю', 'Ю'),
        k2(53, '.', ','),
    ];
    let mut j = 0;
    while j < 35 {
        k[12 + j] = rest[j];
        j += 1;
    }
    k
};

pub const RU: Layout = Layout {
    name: "ru",
    short: "RU",
    keys: &RU_KEYS,
};

/// Czech QWERTZ, "basic": the accented letters on the number row, the
/// digits with Shift, dead acute and caron on `=`, `ú` on `[`, `ů` on `;`,
/// `y` and `z` swapped; AltGr's symbols and the fourth level's dead keys.
pub const CZ: Layout = Layout {
    name: "cz",
    short: "CZ",
    keys: &[
        k4(41, [c(';'), d(Ring), c('`'), c('~')]),
        k4(2, [c('+'), c('1'), c('!'), d(Tilde)]),
        k4(3, [c('ě'), c('2'), c('@'), d(Caron)]),
        k4(4, [c('š'), c('3'), c('#'), d(Circumflex)]),
        k4(5, [c('č'), c('4'), c('$'), d(Breve)]),
        k4(6, [c('ř'), c('5'), c('%'), d(Ring)]),
        k4(7, [c('ž'), c('6'), c('^'), d(Ogonek)]),
        k4(8, [c('ý'), c('7'), c('&'), d(Grave)]),
        k4(9, [c('á'), c('8'), c('*'), d(Dot)]),
        k4(10, [c('í'), c('9'), c('{'), d(Acute)]),
        k4(11, [c('é'), c('0'), c('}'), d(DoubleAcute)]),
        k4(12, [c('='), c('%'), N, d(Diaeresis)]),
        k4(13, [d(Acute), d(Caron), d(Macron), d(Cedilla)]),
        k4(16, [c('q'), c('Q'), c('\\'), N]),
        k4(17, [c('w'), c('W'), c('|'), N]),
        k4(18, [c('e'), c('E'), c('€'), N]),
        k2(19, 'r', 'R'),
        k2(20, 't', 'T'),
        k2(21, 'z', 'Z'),
        k2(22, 'u', 'U'),
        k2(23, 'i', 'I'),
        k2(24, 'o', 'O'),
        k2(25, 'p', 'P'),
        k4(26, [c('ú'), c('/'), c('['), c('÷')]),
        k4(27, [c(')'), c('('), c(']'), c('×')]),
        k4(30, [c('a'), c('A'), c('~'), N]),
        k4(31, [c('s'), c('S'), c('đ'), N]),
        k4(32, [c('d'), c('D'), c('Đ'), N]),
        k4(33, [c('f'), c('F'), c('['), N]),
        k4(34, [c('g'), c('G'), c(']'), N]),
        k4(35, [c('h'), c('H'), c('`'), N]),
        k4(36, [c('j'), c('J'), c('\''), N]),
        k4(37, [c('k'), c('K'), c('ł'), N]),
        k4(38, [c('l'), c('L'), c('Ł'), N]),
        k4(39, [c('ů'), c('"'), c('$'), d(DoubleAcute)]),
        k4(40, [c('§'), c('!'), c('\''), c('ß')]),
        k4(43, [d(Diaeresis), c('\''), c('\\'), c('|')]),
        k4(44, [c('y'), c('Y'), c('°'), N]),
        k4(45, [c('x'), c('X'), c('#'), N]),
        k4(46, [c('c'), c('C'), c('&'), N]),
        k4(47, [c('v'), c('V'), c('@'), N]),
        k4(48, [c('b'), c('B'), c('{'), N]),
        k4(49, [c('n'), c('N'), c('}'), N]),
        k4(50, [c('m'), c('M'), c('^'), N]),
        k4(51, [c(','), c('?'), c('<'), N]),
        k4(52, [c('.'), c(':'), c('>'), N]),
        k4(53, [c('-'), c('_'), c('*'), N]),
        k4(86, [c('\\'), c('|'), c('/'), N]),
    ],
};
