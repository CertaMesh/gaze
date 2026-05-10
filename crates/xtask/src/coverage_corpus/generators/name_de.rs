use rand::rngs::StdRng;
use rand::SeedableRng;

use super::common::pick;
use super::Generator;

pub struct NameDeGenerator;

impl Generator for NameDeGenerator {
    fn id(&self) -> &'static str {
        "name_de"
    }

    fn class_id(&self) -> &'static str {
        "Name"
    }

    fn locale(&self) -> Option<&'static str> {
        Some("de-DE")
    }

    fn generate(&self, seed: u64) -> String {
        let mut rng = StdRng::seed_from_u64(seed);
        format!("{} {}", pick(&mut rng, FIRST), pick(&mut rng, LAST))
    }
}

const FIRST: &[&str] = &["Max", "Mara", "Nina", "Jonas", "Laura", "Felix"];
const LAST: &[&str] = &["Schmidt", "Meyer", "Fischer", "Weber", "Wagner", "Hoffmann"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_value_passes_class_validator() {
        let generator = NameDeGenerator;
        for seed in 0..100 {
            let value = generator.generate(seed);
            assert!(value
                .split_whitespace()
                .all(|part| part.starts_with(char::is_uppercase)));
            assert_eq!(value.split_whitespace().count(), 2);
        }
    }
}
