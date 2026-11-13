//! The wire format of service protocols (ADR-0052), and the [`protocol!`]
//! macro that declares them.
//!
//! A message is a 16-byte header, then fixed fields in order with natural
//! alignment and zero padding, then bounded variable parts. Handles travel
//! out of band, in the channel message's handle list. A decoder checks the
//! whole message, once, before any effect: whatever it accepts is valid, and
//! everything else, including nonzero padding and trailing bytes, is refused.
//! `WIRE.md`, next to this crate's manifest, states the bytes without Rust.

#![no_std]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

use core::marker::PhantomData;

pub mod testing;

/// `ZP`, then the header's version.
pub const MAGIC_VERSION: u32 = 0x5a50_0001;
pub const HEADER_SIZE: usize = 16;
/// The largest message a channel carries (ADR-0050).
pub const MAX_WIRE_MESSAGE: usize = 64 * 1024;
/// The ordinal of the one generic error reply of every protocol.
pub const ERROR_ORDINAL: u32 = 0;
/// The transaction id of a message that expects no reply.
pub const NO_REPLY: u32 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireError {
    /// Fewer bytes than the message says.
    Truncated,
    BadMagic,
    /// The header's body size is not the body's.
    BadSize,
    /// Padding or a reserved field is not zero.
    BadPadding,
    /// A value out of its declared range.
    BadValue,
    /// A variable part over its declared maximum.
    TooLong,
    /// Bytes after the last field.
    Trailing,
    /// A different message than asked for.
    WrongOrdinal,
    /// Extra or missing handles.
    WrongHandles,
    /// The output does not fit.
    NoRoom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub ordinal: u32,
    pub transaction: u32,
}

impl Header {
    /// Checks the header, and gives it with the body.
    pub fn decode(bytes: &[u8]) -> Result<(Header, &[u8]), WireError> {
        if bytes.len() > MAX_WIRE_MESSAGE {
            return Err(WireError::TooLong);
        }
        let (head, body) = bytes
            .split_at_checked(HEADER_SIZE)
            .ok_or(WireError::Truncated)?;
        let word =
            |at: usize| u32::from_le_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]);
        if word(0) != MAGIC_VERSION {
            return Err(WireError::BadMagic);
        }
        if word(12) as usize != body.len() {
            return Err(WireError::BadSize);
        }
        Ok((
            Header {
                ordinal: word(4),
                transaction: word(8),
            },
            body,
        ))
    }
}

/// Reads a body once, forward.
pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub const fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader { bytes, at: 0 }
    }

    /// Skips zero padding up to `align`, which must be a power of two.
    pub fn align(&mut self, align: usize) -> Result<(), WireError> {
        let to = self.at.next_multiple_of(align);
        let padding = self.bytes.get(self.at..to).ok_or(WireError::Truncated)?;
        if padding.iter().any(|b| *b != 0) {
            return Err(WireError::BadPadding);
        }
        self.at = to;
        Ok(())
    }

    pub fn take(&mut self, count: usize) -> Result<&'a [u8], WireError> {
        let end = self.at.checked_add(count).ok_or(WireError::Truncated)?;
        let bytes = self.bytes.get(self.at..end).ok_or(WireError::Truncated)?;
        self.at = end;
        Ok(bytes)
    }

    /// True if every byte has been read.
    pub const fn is_done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

/// Writes a message. Every overrun is an error, so a caller checks once.
pub struct Writer<'b> {
    out: &'b mut [u8],
    at: usize,
}

impl<'b> Writer<'b> {
    pub const fn new(out: &'b mut [u8]) -> Writer<'b> {
        Writer { out, at: 0 }
    }

    pub fn align(&mut self, align: usize) -> Result<(), WireError> {
        let to = self.at.next_multiple_of(align);
        self.out
            .get_mut(self.at..to)
            .ok_or(WireError::NoRoom)?
            .fill(0);
        self.at = to;
        Ok(())
    }

    pub fn put(&mut self, bytes: &[u8]) -> Result<(), WireError> {
        let end = self.at.checked_add(bytes.len()).ok_or(WireError::NoRoom)?;
        self.out
            .get_mut(self.at..end)
            .ok_or(WireError::NoRoom)?
            .copy_from_slice(bytes);
        self.at = end;
        Ok(())
    }

    pub const fn length(&self) -> usize {
        self.at
    }
}

/// A declared kind: the only things a field can be (INV-0052-3).
pub trait Field<'a>: Sized {
    /// True for a part whose size comes from the message.
    const VARIABLE: bool;
    fn put(&self, w: &mut Writer<'_>) -> Result<(), WireError>;
    fn get(r: &mut Reader<'a>) -> Result<Self, WireError>;
}

macro_rules! integer {
    ($($type:ty),*) => {$(
        impl Field<'_> for $type {
            const VARIABLE: bool = false;

            fn put(&self, w: &mut Writer<'_>) -> Result<(), WireError> {
                w.align(size_of::<$type>())?;
                w.put(&self.to_le_bytes())
            }

            fn get(r: &mut Reader<'_>) -> Result<Self, WireError> {
                r.align(size_of::<$type>())?;
                let mut raw = [0; size_of::<$type>()];
                raw.copy_from_slice(r.take(size_of::<$type>())?);
                Ok(<$type>::from_le_bytes(raw))
            }
        }
    )*};
}

integer!(u8, u16, u32, u64, i32);

impl Field<'_> for bool {
    const VARIABLE: bool = false;

    fn put(&self, w: &mut Writer<'_>) -> Result<(), WireError> {
        (*self as u8).put(w)
    }

    fn get(r: &mut Reader<'_>) -> Result<Self, WireError> {
        match u8::get(r)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(WireError::BadValue),
        }
    }
}

/// Bits, checked against `MASK`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flags<const MASK: u32>(pub u32);

impl<const MASK: u32> Field<'_> for Flags<MASK> {
    const VARIABLE: bool = false;

    fn put(&self, w: &mut Writer<'_>) -> Result<(), WireError> {
        self.0.put(w)
    }

    fn get(r: &mut Reader<'_>) -> Result<Self, WireError> {
        let raw = u32::get(r)?;
        if raw & !MASK != 0 {
            return Err(WireError::BadValue);
        }
        Ok(Flags(raw))
    }
}

fn length<const N: usize>(r: &mut Reader<'_>) -> Result<usize, WireError> {
    let count = u32::get(r)? as usize;
    if count > N {
        return Err(WireError::TooLong);
    }
    Ok(count)
}

fn put_length<const N: usize>(count: usize, w: &mut Writer<'_>) -> Result<(), WireError> {
    if count > N {
        return Err(WireError::TooLong);
    }
    (count as u32).put(w)
}

/// At most `N` raw bytes: file contents, never anything with structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Data<'a, const N: usize>(pub &'a [u8]);

impl<'a, const N: usize> Field<'a> for Data<'a, N> {
    const VARIABLE: bool = true;

    fn put(&self, w: &mut Writer<'_>) -> Result<(), WireError> {
        put_length::<N>(self.0.len(), w)?;
        w.put(self.0)
    }

    fn get(r: &mut Reader<'a>) -> Result<Self, WireError> {
        let count = length::<N>(r)?;
        Ok(Data(r.take(count)?))
    }
}

/// UTF-8 of at most `N` bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Text<'a, const N: usize>(pub &'a str);

impl<'a, const N: usize> Field<'a> for Text<'a, N> {
    const VARIABLE: bool = true;

    fn put(&self, w: &mut Writer<'_>) -> Result<(), WireError> {
        put_length::<N>(self.0.len(), w)?;
        w.put(self.0.as_bytes())
    }

    fn get(r: &mut Reader<'a>) -> Result<Self, WireError> {
        let count = length::<N>(r)?;
        let text = core::str::from_utf8(r.take(count)?).map_err(|_| WireError::BadValue)?;
        Ok(Text(text))
    }
}

/// A value below `N`: an enumeration, checked (ADR-0052).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Below<const N: u32>(pub u32);

impl<const N: u32> Field<'_> for Below<N> {
    const VARIABLE: bool = false;

    fn put(&self, w: &mut Writer<'_>) -> Result<(), WireError> {
        if self.0 >= N {
            return Err(WireError::BadValue);
        }
        self.0.put(w)
    }

    fn get(r: &mut Reader<'_>) -> Result<Self, WireError> {
        let raw = u32::get(r)?;
        if raw >= N {
            return Err(WireError::BadValue);
        }
        Ok(Below(raw))
    }
}

/// A relative path (ADR-0055, ADR-0013): components that are each a [`Name`],
/// so no leading `/`. `EMPTY` allows the empty path, which only a reply about the end of a path can be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Path<'a, const N: usize, const EMPTY: bool>(pub &'a str);

impl<const N: usize, const EMPTY: bool> Path<'_, N, EMPTY> {
    /// Every component is a [`Name`], so no `.` or `..` (INV-0013-5).
    pub fn is_valid(path: &str) -> bool {
        (path.is_empty() && EMPTY) || (!path.is_empty() && path.split('/').all(Name::is_valid))
    }
}

impl<'a, const N: usize, const EMPTY: bool> Field<'a> for Path<'a, N, EMPTY> {
    const VARIABLE: bool = true;

    fn put(&self, w: &mut Writer<'_>) -> Result<(), WireError> {
        if !Self::is_valid(self.0) {
            return Err(WireError::BadValue);
        }
        Text::<'a, N>(self.0).put(w)
    }

    fn get(r: &mut Reader<'a>) -> Result<Self, WireError> {
        let Text::<N>(path) = Text::get(r)?;
        if !Self::is_valid(path) {
            return Err(WireError::BadValue);
        }
        Ok(Path(path))
    }
}

/// One path component, as ADR-0013 allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Name<'a>(pub &'a str);

impl<'a> Name<'a> {
    /// ADR-0013 rules 1, 2 and 4.
    pub fn is_valid(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= 255
            && name != "."
            && name != ".."
            && !name.chars().any(|c| c == '/' || c.is_control())
    }
}

impl<'a> Field<'a> for Name<'a> {
    const VARIABLE: bool = true;

    fn put(&self, w: &mut Writer<'_>) -> Result<(), WireError> {
        if !Name::is_valid(self.0) {
            return Err(WireError::BadValue);
        }
        Text::<'a, 255>(self.0).put(w)
    }

    fn get(r: &mut Reader<'a>) -> Result<Self, WireError> {
        let Text::<255>(name) = Text::get(r)?;
        if !Name::is_valid(name) {
            return Err(WireError::BadValue);
        }
        Ok(Name(name))
    }
}

/// At most `N` records, decoded whole when the message is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seq<'a, T, const N: usize> {
    /// To send.
    Of(&'a [T]),
    /// As it arrived, already checked.
    Wire {
        bytes: &'a [u8],
        count: usize,
        kind: PhantomData<T>,
    },
}

impl<'a, T: Field<'a> + Copy + 'a, const N: usize> Seq<'a, T, N> {
    pub fn len(&self) -> usize {
        match self {
            Seq::Of(items) => items.len(),
            Seq::Wire { count, .. } => *count,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = T> + 'a {
        let (mut items, mut reader, mut left) = match self {
            Seq::Of(items) => (items.iter(), Reader::new(&[]), 0),
            Seq::Wire { bytes, count, .. } => ([].iter(), Reader::new(bytes), *count),
        };
        core::iter::from_fn(move || {
            if let Some(item) = items.next() {
                return Some(*item);
            }
            if left == 0 {
                return None;
            }
            left -= 1;
            // `get` accepted these bytes already.
            reader.align(8).ok()?;
            T::get(&mut reader).ok()
        })
    }
}

impl<'a, T: Field<'a> + Copy + 'a, const N: usize> Field<'a> for Seq<'a, T, N> {
    const VARIABLE: bool = true;

    fn put(&self, w: &mut Writer<'_>) -> Result<(), WireError> {
        put_length::<N>(self.len(), w)?;
        w.align(8)?;
        let size_at = w.length();
        0u64.put(w)?;
        let start = w.length();
        for item in self.iter() {
            w.align(8)?;
            item.put(w)?;
        }
        let size = (w.length() - start) as u64;
        w.out[size_at..start].copy_from_slice(&size.to_le_bytes());
        Ok(())
    }

    fn get(r: &mut Reader<'a>) -> Result<Self, WireError> {
        let count = length::<N>(r)?;
        let size = u64::get(r)?;
        let size = usize::try_from(size).map_err(|_| WireError::TooLong)?;
        let bytes = r.take(size)?;
        let mut inner = Reader::new(bytes);
        for _ in 0..count {
            inner.align(8)?;
            T::get(&mut inner)?;
        }
        if !inner.is_done() {
            return Err(WireError::Trailing);
        }
        Ok(Seq::Wire {
            bytes,
            count,
            kind: PhantomData,
        })
    }
}

/// Panics, at compile time when used in a constant, if a fixed field follows
/// a variable part (ADR-0052).
pub const fn fixed_then_variable(fields: &[bool]) {
    let mut seen = false;
    let mut i = 0;
    while i < fields.len() {
        assert!(!seen || fields[i], "a fixed field after a variable part");
        seen = fields[i];
        i += 1;
    }
}

const fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Field names that would be an untyped operation (INV-0052-3).
const FORBIDDEN_FIELDS: [&str; 7] = [
    "op",
    "opcode",
    "operation",
    "command",
    "cmd",
    "ioctl",
    "request",
];

/// Panics, at compile time, on a field that names an operation. An operation
/// is a method, with an ordinal of its own (DM-053).
pub const fn no_operation_field(fields: &[&str]) {
    let mut i = 0;
    while i < fields.len() {
        let mut j = 0;
        while j < FORBIDDEN_FIELDS.len() {
            assert!(!same(fields[i], FORBIDDEN_FIELDS[j]), "an operation field");
            j += 1;
        }
        i += 1;
    }
}

/// One message of a protocol.
pub trait Message<'a>: Sized {
    const ORDINAL: u32;
    /// How many handles travel with it.
    const HANDLES: usize;
    fn body(&self, w: &mut Writer<'_>) -> Result<(), WireError>;
    fn parse(r: &mut Reader<'a>) -> Result<Self, WireError>;
}

/// Writes `message` to `out`, and gives its length.
pub fn encode<'a, M: Message<'a>>(
    message: &M,
    transaction: u32,
    out: &mut [u8],
) -> Result<usize, WireError> {
    if out.len() < HEADER_SIZE {
        return Err(WireError::NoRoom);
    }
    let room = out.len().min(MAX_WIRE_MESSAGE);
    let (head, body) = out[..room].split_at_mut(HEADER_SIZE);
    let mut w = Writer::new(body);
    message.body(&mut w)?;
    let size = w.length();
    head[0..4].copy_from_slice(&MAGIC_VERSION.to_le_bytes());
    head[4..8].copy_from_slice(&M::ORDINAL.to_le_bytes());
    head[8..12].copy_from_slice(&transaction.to_le_bytes());
    head[12..16].copy_from_slice(&(size as u32).to_le_bytes());
    Ok(HEADER_SIZE + size)
}

/// Checks all of a message that has `handles` handles, and gives it. Total: no
/// input panics (INV-0052-1).
pub fn decode<'a, M: Message<'a>>(
    bytes: &'a [u8],
    handles: usize,
) -> Result<(Header, M), WireError> {
    let (header, body) = Header::decode(bytes)?;
    if header.ordinal != M::ORDINAL {
        return Err(WireError::WrongOrdinal);
    }
    if handles != M::HANDLES {
        return Err(WireError::WrongHandles);
    }
    let mut r = Reader::new(body);
    let message = M::parse(&mut r)?;
    if !r.is_done() {
        return Err(WireError::Trailing);
    }
    Ok((header, message))
}

/// The one reply to an ordinal a service does not know, the same for every
/// protocol (ADR-0052). `status` is the protocol's own value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorReply {
    pub status: u32,
}

impl Message<'_> for ErrorReply {
    const ORDINAL: u32 = ERROR_ORDINAL;
    const HANDLES: usize = 0;

    fn body(&self, w: &mut Writer<'_>) -> Result<(), WireError> {
        self.status.put(w)
    }

    fn parse(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(ErrorReply {
            status: u32::get(r)?,
        })
    }
}

/// Declares messages or records. A field's type must implement [`Field`], so
/// no field can be a free-form anything.
///
/// ```text
/// protocol! {
///     message Read = 3, handles 0 { offset: u64, length: u32 }
///     message Name<'a> = 4, handles 1 { name: Name<'a> }
///     record Entry<'a> { cookie: u64, name: Name<'a> }
/// }
/// ```
#[macro_export]
macro_rules! protocol {
    () => {};
    ($(#[$meta:meta])* message $name:ident<$lt:lifetime> = $ordinal:literal, handles $handles:literal {
        $($(#[$field_meta:meta])* $field:ident: $type:ty),* $(,)?
    } $($rest:tt)*) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name<$lt> { $($(#[$field_meta])* pub $field: $type),* }

        impl<$lt> $crate::Message<$lt> for $name<$lt> {
            const ORDINAL: u32 = $ordinal;
            const HANDLES: usize = $handles;
            $crate::protocol!(@body $lt $name; $($field: $type),*);
        }
        $crate::protocol!($($rest)*);
    };
    ($(#[$meta:meta])* message $name:ident = $ordinal:literal, handles $handles:literal {
        $($(#[$field_meta:meta])* $field:ident: $type:ty),* $(,)?
    } $($rest:tt)*) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name { $($(#[$field_meta])* pub $field: $type),* }

        impl<'a> $crate::Message<'a> for $name {
            const ORDINAL: u32 = $ordinal;
            const HANDLES: usize = $handles;
            $crate::protocol!(@body 'a $name; $($field: $type),*);
        }
        $crate::protocol!($($rest)*);
    };
    ($(#[$meta:meta])* record $name:ident<$lt:lifetime> {
        $($(#[$field_meta:meta])* $field:ident: $type:ty),* $(,)?
    } $($rest:tt)*) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name<$lt> { $($(#[$field_meta])* pub $field: $type),* }

        impl<$lt> $crate::Field<$lt> for $name<$lt> {
            const VARIABLE: bool = true;

            fn put(&self, w: &mut $crate::Writer<'_>) -> Result<(), $crate::WireError> {
                const { $crate::no_operation_field(&[$(stringify!($field)),*]) };
                $($crate::Field::put(&self.$field, w)?;)*
                Ok(())
            }

            fn get(r: &mut $crate::Reader<$lt>) -> Result<Self, $crate::WireError> {
                Ok($name { $($field: <$type as $crate::Field>::get(r)?),* })
            }
        }
        $crate::protocol!($($rest)*);
    };
    ($(#[$meta:meta])* record $name:ident {
        $($(#[$field_meta:meta])* $field:ident: $type:ty),* $(,)?
    } $($rest:tt)*) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name { $($(#[$field_meta])* pub $field: $type),* }

        impl<'a> $crate::Field<'a> for $name {
            const VARIABLE: bool = true;

            fn put(&self, w: &mut $crate::Writer<'_>) -> Result<(), $crate::WireError> {
                const { $crate::no_operation_field(&[$(stringify!($field)),*]) };
                $($crate::Field::put(&self.$field, w)?;)*
                Ok(())
            }

            fn get(r: &mut $crate::Reader<'a>) -> Result<Self, $crate::WireError> {
                Ok($name { $($field: <$type as $crate::Field>::get(r)?),* })
            }
        }
        $crate::protocol!($($rest)*);
    };
    (@body $lt:lifetime $name:ident; $($field:ident: $type:ty),*) => {
        // An empty message uses neither argument.
        #[allow(unused_variables)]
        fn body(&self, w: &mut $crate::Writer<'_>) -> Result<(), $crate::WireError> {
            const { $crate::fixed_then_variable(&[$(<$type as $crate::Field>::VARIABLE),*]) };
            const { $crate::no_operation_field(&[$(stringify!($field)),*]) };
            $($crate::Field::put(&self.$field, w)?;)*
            Ok(())
        }

        #[allow(unused_variables)]
        fn parse(r: &mut $crate::Reader<$lt>) -> Result<Self, $crate::WireError> {
            Ok($name { $($field: <$type as $crate::Field>::get(r)?),* })
        }
    };
}

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
