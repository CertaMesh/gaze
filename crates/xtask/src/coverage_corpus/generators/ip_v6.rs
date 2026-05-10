use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use super::Generator;

pub struct Ipv6Generator;

impl Generator for Ipv6Generator {
    fn id(&self) -> &'static str {
        "ip_v6"
    }

    fn class_id(&self) -> &'static str {
        "custom:ip_address"
    }

    fn locale(&self) -> Option<&'static str> {
        None
    }

    fn generate(&self, seed: u64) -> String {
        let mut rng = StdRng::seed_from_u64(seed);
        if seed.is_multiple_of(2) {
            format!(
                "2001:db8:{:x}:{:x}::{:x}",
                rng.gen_range(1..=0xffff_u16),
                rng.gen_range(1..=0xffff_u16),
                rng.gen_range(1..=0xffff_u16)
            )
        } else {
            format!(
                "2001:db8::{:x}:192.0.2.{}",
                rng.gen_range(1..=0xffff_u16),
                rng.gen_range(1..=254)
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv6Addr;

    #[test]
    fn generated_value_passes_class_validator() {
        let generator = Ipv6Generator;
        for seed in 0..100 {
            assert!(generator.generate(seed).parse::<Ipv6Addr>().is_ok());
        }
    }
}
