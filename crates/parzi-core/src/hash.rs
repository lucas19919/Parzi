
const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

#[must_use]
pub fn fnv1a_from(mut h: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(PRIME);
    }
    h
}

#[must_use]
pub fn fnv1a(bytes: &[u8]) -> u64 {
    fnv1a_from(OFFSET, bytes)
}

#[must_use]
pub fn fnv_hex(bytes: &[u8]) -> String {
    format!("{:016x}", fnv1a(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_fnv1a_64() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv_hex(b"foobar"), "85944171f73967e8");
        assert_eq!(fnv1a_from(fnv1a(b"foo"), b"bar"), fnv1a(b"foobar"));
    }
}
