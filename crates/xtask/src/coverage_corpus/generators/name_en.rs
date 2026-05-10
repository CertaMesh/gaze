use rand::rngs::StdRng;
use rand::SeedableRng;

use super::common::pick;
use super::Generator;

pub struct NameEnGenerator;

impl Generator for NameEnGenerator {
    fn id(&self) -> &'static str {
        "name_en"
    }

    fn class_id(&self) -> &'static str {
        "Name"
    }

    fn locale(&self) -> Option<&'static str> {
        Some("en-US")
    }

    fn generate(&self, seed: u64) -> String {
        let mut rng = StdRng::seed_from_u64(seed);
        format!("{} {}", pick(&mut rng, FIRST), pick(&mut rng, LAST))
    }
}

const FIRST: &[&str] = &["Alice", "Blair", "Casey", "Dana", "Evan", "Morgan"];
const LAST: &[&str] = &["Taylor", "Jordan", "Morgan", "Parker", "Reed", "Hayes"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_value_passes_class_validator() {
        let generator = NameEnGenerator;
        for seed in 0..100 {
            let value = generator.generate(seed);
            assert!(value
                .split_whitespace()
                .all(|part| part.starts_with(char::is_uppercase)));
            assert_eq!(value.split_whitespace().count(), 2);
        }
    }
}
