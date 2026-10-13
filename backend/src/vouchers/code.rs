use std::collections::HashSet;

use aes_gcm::aead::OsRng;
use aes_gcm::aead::rand_core::RngCore;

pub const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
const GROUP_LEN: usize = 4;
const GROUPS: usize = 3;

pub fn generate_code() -> String {
    let mut rng = OsRng;
    let mut code = String::with_capacity(GROUPS * GROUP_LEN + GROUPS - 1);
    for group in 0..GROUPS {
        if group > 0 {
            code.push('-');
        }
        for _ in 0..GROUP_LEN {
            code.push(ALPHABET[random_index(&mut rng)] as char);
        }
    }
    code
}

pub fn generate_unique_codes(count: usize) -> Vec<String> {
    let mut codes = Vec::with_capacity(count);
    let mut seen: HashSet<String> = HashSet::with_capacity(count);
    while codes.len() < count {
        let code = generate_code();
        if seen.insert(code.clone()) {
            codes.push(code);
        }
    }
    codes
}

fn random_index(rng: &mut impl RngCore) -> usize {
    // Rejection sampling keeps the distribution uniform (no modulo bias).
    let limit = (256 / ALPHABET.len()) * ALPHABET.len();
    loop {
        let mut byte = [0u8; 1];
        rng.fill_bytes(&mut byte);
        if usize::from(byte[0]) < limit {
            return usize::from(byte[0]) % ALPHABET.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{ALPHABET, GROUP_LEN, GROUPS, generate_code, generate_unique_codes};

    #[test]
    fn codes_have_expected_shape_and_alphabet() {
        let code = generate_code();
        assert_eq!(code.len(), GROUPS * GROUP_LEN + GROUPS - 1);
        for character in code.chars() {
            if character == '-' {
                continue;
            }
            assert!(
                ALPHABET.contains(&(character as u8)),
                "unexpected character {character}"
            );
        }
        assert_eq!(code.matches('-').count(), GROUPS - 1);
    }

    #[test]
    fn codes_are_unique_and_random() {
        let codes = generate_unique_codes(2_000);
        assert_eq!(codes.len(), 2_000);
        let unique: HashSet<&String> = codes.iter().collect();
        assert_eq!(unique.len(), 2_000);

        // Two independent draws must not be identical with overwhelming probability.
        assert_ne!(generate_code(), generate_code());
    }
}
