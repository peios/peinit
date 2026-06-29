use sha1::{Digest, Sha1};

pub fn service_sid(service_name: &str) -> String {
    let mut encoded = Vec::with_capacity(service_name.len() * 2);
    for unit in service_name.to_uppercase().encode_utf16() {
        encoded.extend_from_slice(&unit.to_le_bytes());
    }
    let digest = Sha1::digest(&encoded);
    let mut parts = Vec::with_capacity(5);
    for chunk in digest.chunks_exact(4) {
        parts.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    format!(
        "S-1-5-80-{}-{}-{}-{}-{}",
        parts[0], parts[1], parts[2], parts[3], parts[4]
    )
}

#[cfg(test)]
mod tests {
    use super::service_sid;

    #[test]
    fn service_sid_is_case_insensitive() {
        assert_eq!(service_sid("app"), service_sid("APP"));
    }

    #[test]
    fn service_sid_uses_sha1_sub_authorities() {
        assert_eq!(
            service_sid("app"),
            "S-1-5-80-2426739453-2501902915-3009591593-922485235-2122754908"
        );
    }
}
