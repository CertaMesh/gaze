use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use super::Generator;

pub struct PhoneStructuralGenerator;

impl Generator for PhoneStructuralGenerator {
    fn id(&self) -> &'static str {
        "phone_structural"
    }

    fn class_id(&self) -> &'static str {
        "custom:phone"
    }

    fn locale(&self) -> Option<&'static str> {
        None
    }

    fn generate(&self, seed: u64) -> String {
        let mut rng = StdRng::seed_from_u64(seed);
        format!("+120255501{:02}", rng.gen_range(0..=99))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_value_passes_class_validator() {
        let generator = PhoneStructuralGenerator;
        for seed in 0..100 {
            let value = generator.generate(seed);
            let phone = phonenumber::parse(None, &value).expect("parse E.164 phone");
            assert!(phonenumber::is_valid(&phone));
        }
    }
}
