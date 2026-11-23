//! Cursors over a message's bytes, so a message is read and written
//! field by field in its manual's order: little-endian for UBX
//! (UBX-13003221 R28, "UBX Protocol"), big-endian for the Oncore (VP
//! reference, "Binary Format").
//!
//! Like `bytes::Buf`, a read or write past the end panics: a decoder
//! checks a message's length before it reads, and an encoder writes into
//! a buffer of the message's length.

use core::marker::PhantomData;

/// The fields' widths, bytes: two, three (the Oncore's and the module's
/// 24-bit fields), four and eight.
const U16_BYTES: usize = size_of::<u16>();
const U24_BYTES: usize = 3;
const U32_BYTES: usize = size_of::<u32>();
const U64_BYTES: usize = size_of::<u64>();

/// A byte order.
pub trait ByteOrder {
    fn u16(b: [u8; U16_BYTES]) -> u16;
    fn u24(b: [u8; U24_BYTES]) -> u32;
    fn u32(b: [u8; U32_BYTES]) -> u32;
    fn u64(b: [u8; U64_BYTES]) -> u64;
    fn u16_bytes(v: u16) -> [u8; U16_BYTES];
    fn u32_bytes(v: u32) -> [u8; U32_BYTES];
    fn u64_bytes(v: u64) -> [u8; U64_BYTES];
}

/// UBX's byte order.
#[derive(Debug, Clone, Copy)]
pub struct Little;

/// The Oncore's byte order.
#[derive(Debug, Clone, Copy)]
pub struct Big;

impl ByteOrder for Little {
    fn u16(b: [u8; U16_BYTES]) -> u16 {
        u16::from_le_bytes(b)
    }
    fn u24([a, b, c]: [u8; U24_BYTES]) -> u32 {
        u32::from_le_bytes([a, b, c, 0])
    }
    fn u32(b: [u8; U32_BYTES]) -> u32 {
        u32::from_le_bytes(b)
    }
    fn u64(b: [u8; U64_BYTES]) -> u64 {
        u64::from_le_bytes(b)
    }
    fn u16_bytes(v: u16) -> [u8; U16_BYTES] {
        v.to_le_bytes()
    }
    fn u32_bytes(v: u32) -> [u8; U32_BYTES] {
        v.to_le_bytes()
    }
    fn u64_bytes(v: u64) -> [u8; U64_BYTES] {
        v.to_le_bytes()
    }
}

impl ByteOrder for Big {
    fn u16(b: [u8; U16_BYTES]) -> u16 {
        u16::from_be_bytes(b)
    }
    fn u24([a, b, c]: [u8; U24_BYTES]) -> u32 {
        u32::from_be_bytes([0, a, b, c])
    }
    fn u32(b: [u8; U32_BYTES]) -> u32 {
        u32::from_be_bytes(b)
    }
    fn u64(b: [u8; U64_BYTES]) -> u64 {
        u64::from_be_bytes(b)
    }
    fn u16_bytes(v: u16) -> [u8; U16_BYTES] {
        v.to_be_bytes()
    }
    fn u32_bytes(v: u32) -> [u8; U32_BYTES] {
        v.to_be_bytes()
    }
    fn u64_bytes(v: u64) -> [u8; U64_BYTES] {
        v.to_be_bytes()
    }
}

/// Reads fields from the front of a message's bytes.
#[derive(Debug)]
pub struct Reader<'a, O> {
    bytes: &'a [u8],
    order: PhantomData<O>,
}

/// A UBX payload's reader, and an Oncore message's.
pub type UbxReader<'a> = Reader<'a, Little>;
pub type OncoreReader<'a> = Reader<'a, Big>;

impl<'a, O: ByteOrder> Reader<'a, O> {
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, order: PhantomData }
    }

    /// The next `N` bytes.
    pub fn array<const N: usize>(&mut self) -> [u8; N] {
        let (head, rest) = self.bytes.split_first_chunk::<N>().expect("a field past the message's end");
        self.bytes = rest;
        *head
    }

    /// The bytes not yet read.
    pub fn rest(&self) -> &'a [u8] {
        self.bytes
    }

    /// Passes over `n` bytes: reserved, or not read.
    pub fn skip(&mut self, n: usize) -> &mut Self {
        self.bytes = &self.bytes[n..];
        self
    }

    pub fn u8(&mut self) -> u8 {
        let [b] = self.array();
        b
    }

    pub fn i8(&mut self) -> i8 {
        self.u8() as i8
    }

    pub fn u16(&mut self) -> u16 {
        O::u16(self.array())
    }

    /// Three bytes, unsigned.
    pub fn u24(&mut self) -> u32 {
        O::u24(self.array())
    }

    pub fn u32(&mut self) -> u32 {
        O::u32(self.array())
    }

    pub fn i32(&mut self) -> i32 {
        self.u32() as i32
    }

    pub fn f32(&mut self) -> f32 {
        f32::from_bits(self.u32())
    }

    pub fn f64(&mut self) -> f64 {
        f64::from_bits(O::u64(self.array()))
    }
}

/// Writes fields into a buffer from its front.
#[derive(Debug)]
pub struct Writer<'a, O> {
    out: &'a mut [u8],
    len: usize,
    order: PhantomData<O>,
}

/// A UBX payload's writer, and an Oncore message's.
pub type UbxWriter<'a> = Writer<'a, Little>;
pub type OncoreWriter<'a> = Writer<'a, Big>;

impl<'a, O: ByteOrder> Writer<'a, O> {
    pub const fn new(out: &'a mut [u8]) -> Self {
        Self { out, len: 0, order: PhantomData }
    }

    /// The bytes written so far.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.out[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
        self
    }

    /// Passes over `n` bytes, leaving them: fields not changed.
    pub fn skip(&mut self, n: usize) -> &mut Self {
        assert!(self.len + n <= self.out.len(), "a field past the message's end");
        self.len += n;
        self
    }

    /// `n` zeros: reserved, or not given.
    pub fn zeros(&mut self, n: usize) -> &mut Self {
        self.out[self.len..self.len + n].fill(0);
        self.len += n;
        self
    }

    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.bytes(&[v])
    }

    pub fn i8(&mut self, v: i8) -> &mut Self {
        self.u8(v as u8)
    }

    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.bytes(&O::u16_bytes(v))
    }

    pub fn i16(&mut self, v: i16) -> &mut Self {
        self.u16(v as u16)
    }

    pub(crate) fn u32(&mut self, v: u32) -> &mut Self {
        self.bytes(&O::u32_bytes(v))
    }

    pub fn i32(&mut self, v: i32) -> &mut Self {
        self.u32(v as u32)
    }

    pub fn f32(&mut self, v: f32) -> &mut Self {
        self.u32(v.to_bits())
    }

    pub fn f64(&mut self, v: f64) -> &mut Self {
        self.bytes(&O::u64_bytes(v.to_bits()))
    }
}

#[cfg(test)]
mod tests {
    use super::OncoreReader;
    use super::OncoreWriter;
    use super::U16_BYTES;
    use super::U32_BYTES;
    use super::U64_BYTES;
    use super::UbxReader;
    use super::UbxWriter;

    const WORD: u16 = 0x1234;
    const LONG: i32 = -2;
    const DOUBLE: f64 = 298.257_223_563;
    /// The byte left between the fields; the fields' and its length; where
    /// the Oncore writer's `i24` starts.
    const GAP: usize = 1;
    const LEN: usize = U16_BYTES + U32_BYTES + GAP + U64_BYTES;

    #[test]
    fn fields_round_trip_in_either_order() {
        let mut b = [0u8; LEN];
        let n = UbxWriter::new(&mut b).u16(WORD).i32(LONG).zeros(GAP).f64(DOUBLE).len();
        assert_eq!(n, b.len());
        assert_eq!(b[..U16_BYTES], WORD.to_le_bytes());
        let mut r = UbxReader::new(&b);
        assert_eq!((r.u16(), r.i32(), r.skip(GAP).f64()), (WORD, LONG, DOUBLE));

        OncoreWriter::new(&mut b).u16(WORD).i32(LONG);
        assert_eq!(b[..U16_BYTES], WORD.to_be_bytes());
        let mut r = OncoreReader::new(&b);
        assert_eq!((r.u16(), r.i32()), (WORD, LONG));
    }

    #[test]
    #[should_panic(expected = "past the message's end")]
    fn a_read_past_the_end_panics() {
        UbxReader::new(&[0; U32_BYTES - 1]).u32();
    }
}
