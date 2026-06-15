//! Minimal protobuf wire-format helpers shared by the `windsurf` (Connect) and
//! `grok` (gRPC-Web) providers, which speak protobuf rather than JSON.
//!
//! Only the wire types the two providers use are supported. Everything returns
//! `Option`/bounded results — malformed input yields `None` rather than panics.

/// Protobuf wire types.
pub const WIRE_VARINT: u8 = 0;
pub const WIRE_FIXED64: u8 = 1;
pub const WIRE_LEN: u8 = 2;
pub const WIRE_FIXED32: u8 = 5;

/// A streaming reader over a protobuf message body.
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn done(&self) -> bool {
        self.pos >= self.data.len()
    }

    /// Reads a base-128 varint. Returns `None` on truncation.
    pub fn read_varint(&mut self) -> Option<u64> {
        let mut result: u64 = 0;
        let mut shift = 0;
        loop {
            let byte = *self.data.get(self.pos)?;
            self.pos += 1;
            result |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                return Some(result);
            }
            shift += 7;
            if shift >= 64 {
                return None;
            }
        }
    }

    /// Reads a field key, returning `(field_number, wire_type)`.
    pub fn next_key(&mut self) -> Option<(u32, u8)> {
        let key = self.read_varint()?;
        let field = (key >> 3) as u32;
        let wire = (key & 0x07) as u8;
        if field == 0 {
            return None;
        }
        Some((field, wire))
    }

    /// Reads a length-delimited byte slice.
    pub fn read_len(&mut self) -> Option<&'a [u8]> {
        let len = self.read_varint()? as usize;
        let end = self.pos.checked_add(len)?;
        if end > self.data.len() {
            return None;
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Some(slice)
    }

    pub fn read_fixed32(&mut self) -> Option<u32> {
        let end = self.pos.checked_add(4)?;
        if end > self.data.len() {
            return None;
        }
        let b = &self.data[self.pos..end];
        self.pos = end;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn read_fixed64(&mut self) -> Option<u64> {
        let end = self.pos.checked_add(8)?;
        if end > self.data.len() {
            return None;
        }
        let b = &self.data[self.pos..end];
        self.pos = end;
        Some(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    /// Skips the body of a field with the given wire type. Returns `None` on a
    /// malformed/unsupported wire type.
    pub fn skip(&mut self, wire: u8) -> Option<()> {
        match wire {
            WIRE_VARINT => self.read_varint().map(|_| ()),
            WIRE_FIXED64 => self.read_fixed64().map(|_| ()),
            WIRE_LEN => self.read_len().map(|_| ()),
            WIRE_FIXED32 => self.read_fixed32().map(|_| ()),
            _ => None,
        }
    }
}

/// Appends a varint to `out`.
pub fn encode_varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push(((value & 0x7f) | 0x80) as u8);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Appends a field key `(field_number << 3) | wire_type`.
pub fn encode_key(field: u32, wire: u8, out: &mut Vec<u8>) {
    encode_varint(((field as u64) << 3) | wire as u64, out);
}

/// Appends a length-delimited string field.
pub fn encode_string_field(field: u32, value: &str, out: &mut Vec<u8>) {
    encode_key(field, WIRE_LEN, out);
    encode_varint(value.len() as u64, out);
    out.extend_from_slice(value.as_bytes());
}

/// Appends a varint field.
pub fn encode_varint_field(field: u32, value: u64, out: &mut Vec<u8>) {
    encode_key(field, WIRE_VARINT, out);
    encode_varint(value, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_varint_roundtrip() {
        for v in [0u64, 1, 127, 128, 300, 16384, u32::MAX as u64, u64::MAX] {
            let mut buf = Vec::new();
            encode_varint(v, &mut buf);
            let mut r = Reader::new(&buf);
            assert_eq!(r.read_varint(), Some(v));
            assert!(r.done());
        }
    }

    #[test]
    fn test_field_encode_decode() {
        let mut buf = Vec::new();
        encode_string_field(1, "tok", &mut buf);
        encode_varint_field(2, 1, &mut buf);

        let mut r = Reader::new(&buf);
        assert_eq!(r.next_key(), Some((1, WIRE_LEN)));
        assert_eq!(r.read_len(), Some(&b"tok"[..]));
        assert_eq!(r.next_key(), Some((2, WIRE_VARINT)));
        assert_eq!(r.read_varint(), Some(1));
        assert!(r.done());
    }

    #[test]
    fn test_skip() {
        let mut buf = Vec::new();
        encode_varint_field(1, 42, &mut buf);
        encode_string_field(2, "keep", &mut buf);
        let mut r = Reader::new(&buf);
        let (f, w) = r.next_key().unwrap();
        assert_eq!(f, 1);
        r.skip(w).unwrap();
        assert_eq!(r.next_key(), Some((2, WIRE_LEN)));
        assert_eq!(r.read_len(), Some(&b"keep"[..]));
    }

    #[test]
    fn test_truncated_varint_is_none() {
        let mut r = Reader::new(&[0x80]); // continuation bit set, no next byte
        assert_eq!(r.read_varint(), None);
    }

    #[test]
    fn test_read_len_out_of_bounds() {
        // length says 10 but only 2 bytes follow
        let mut r = Reader::new(&[0x0a, b'h', b'i']);
        let (_f, _w) = r.next_key().unwrap();
        assert_eq!(r.read_len(), None);
    }

    #[test]
    fn test_fixed32() {
        let bytes = 3.5f32.to_bits().to_le_bytes();
        let mut r = Reader::new(&bytes);
        assert_eq!(r.read_fixed32().map(f32::from_bits), Some(3.5));
    }
}
