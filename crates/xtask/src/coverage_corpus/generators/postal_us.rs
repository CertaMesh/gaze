use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use super::Generator;

pub struct PostalUsGenerator;

impl Generator for PostalUsGenerator {
    fn id(&self) -> &'static str {
        "postal_us"
    }

    fn class_id(&self) -> &'static str {
        "custom:postal_code"
    }

    fn locale(&self) -> Option<&'static str> {
        Some("en-US")
    }

    fn generate(&self, seed: u64) -> String {
        let mut rng = StdRng::seed_from_u64(seed);
        format!(
            "{:05}-{:04}",
            rng.gen_range(10_000..=99_999),
            rng.gen_range(1_000..=9_999)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_value_passes_class_validator() {
        let generator = PostalUsGenerator;
        for seed in 0..100 {
            let value = generator.generate(seed);
            assert_eq!(value.len(), 10);
            assert_eq!(value.as_bytes()[5], b'-');
            assert!(value.bytes().filter(|byte| byte.is_ascii_digit()).count() == 9);
        }
    }
}
