//! Stable persisted identifiers, not a cryptographic security primitive.
pub fn stable_id(parts: &[&str]) -> String {
    let mut first = 0xcbf29ce484222325u64;
    let mut second = 0x84222325cbf29ce4u64;
    for part in parts {
        for byte in (part.len() as u64)
            .to_le_bytes()
            .into_iter()
            .chain(part.bytes())
        {
            first = (first ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            second = (second ^ u64::from(byte)).wrapping_mul(0x100000001e7);
        }
    }
    format!("{first:016x}{second:016x}")
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn length_framing_prevents_part_boundary_aliases(){
        assert_ne!(stable_id(&["ab","c"]),stable_id(&["a","bc"]));
        assert_eq!(stable_id(&["한글","file"]),stable_id(&["한글","file"]));
        assert_ne!(stable_id(&[]),stable_id(&[""]));
    }
}
