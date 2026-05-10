use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use sha3::{Digest, Keccak256};

use super::Generator;

pub struct EthEip55Generator;

impl Generator for EthEip55Generator {
    fn id(&self) -> &'static str {
        "eth_eip55"
    }

    fn class_id(&self) -> &'static str {
        "custom:eth_address"
    }

    fn locale(&self) -> Option<&'static str> {
        None
    }

    fn generate(&self, seed: u64) -> String {
        let mut rng = StdRng::seed_from_u64(seed);
        let lowercase = (0..40)
            .map(|_| format!("{:x}", rng.gen_range(0..16)))
            .collect::<String>();
        format!("0x{}", checksum(&lowercase))
    }
}

fn checksum(lowercase_address: &str) -> String {
    let hash = Keccak256::digest(lowercase_address.as_bytes());
    lowercase_address
        .bytes()
        .enumerate()
        .map(|(index, byte)| {
            if byte.is_ascii_digit() {
                return char::from(byte);
            }
            let hash_nibble = if index % 2 == 0 {
                hash[index / 2] >> 4
            } else {
                hash[index / 2] & 0x0f
            };
            if hash_nibble > 7 {
                char::from(byte).to_ascii_uppercase()
            } else {
                char::from(byte)
            }
        })
        .collect()
}

#[cfg(test)]
fn valid_eip55(input: &str) -> bool {
    let Some(address) = input.strip_prefix("0x") else {
        return false;
    };
    address.len() == 40
        && address.bytes().all(|byte| byte.is_ascii_hexdigit())
        && checksum(&address.to_ascii_lowercase()) == address
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_value_passes_class_validator() {
        let generator = EthEip55Generator;
        for seed in 0..100 {
            assert!(valid_eip55(&generator.generate(seed)));
        }
    }
}
