use std::net::Ipv4Addr;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use super::Generator;

pub struct Ipv4Generator;

impl Generator for Ipv4Generator {
    fn id(&self) -> &'static str {
        "ip_v4"
    }

    fn class_id(&self) -> &'static str {
        "custom:ip_address"
    }

    fn locale(&self) -> Option<&'static str> {
        None
    }

    fn generate(&self, seed: u64) -> String {
        let mut rng = StdRng::seed_from_u64(seed);
        Ipv4Addr::new(198, 51, 100, rng.gen_range(1..=254)).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_value_passes_class_validator() {
        let generator = Ipv4Generator;
        for seed in 0..100 {
            assert!(generator.generate(seed).parse::<Ipv4Addr>().is_ok());
        }
    }
}
