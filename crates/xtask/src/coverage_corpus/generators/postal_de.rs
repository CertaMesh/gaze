use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use super::Generator;

pub struct PostalDeGenerator;

impl Generator for PostalDeGenerator {
    fn id(&self) -> &'static str {
        "postal_de"
    }

    fn class_id(&self) -> &'static str {
        "custom:postal_code"
    }

    fn locale(&self) -> Option<&'static str> {
        Some("de-DE")
    }

    fn generate(&self, seed: u64) -> String {
        let mut rng = StdRng::seed_from_u64(seed);
        format!("{:05}", rng.gen_range(10_000..=99_999))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_value_passes_class_validator() {
        let generator = PostalDeGenerator;
        for seed in 0..100 {
            let value = generator.generate(seed);
            assert_eq!(value.len(), 5);
            assert!(value.bytes().all(|byte| byte.is_ascii_digit()));
        }
    }
}
