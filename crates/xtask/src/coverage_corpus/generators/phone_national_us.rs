use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use super::Generator;

pub struct PhoneNationalUsGenerator;

impl Generator for PhoneNationalUsGenerator {
    fn id(&self) -> &'static str {
        "phone_national_us"
    }

    fn class_id(&self) -> &'static str {
        "custom:phone"
    }

    fn locale(&self) -> Option<&'static str> {
        Some("en-US")
    }

    fn generate(&self, seed: u64) -> String {
        let mut rng = StdRng::seed_from_u64(seed);
        format!("555-01{:02}", rng.gen_range(0..=99))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_value_passes_class_validator() {
        let generator = PhoneNationalUsGenerator;
        for seed in 0..100 {
            let phone =
                phonenumber::parse(Some(phonenumber::country::US), generator.generate(seed))
                    .expect("parse US phone");
            assert_eq!(phone.country().code(), 1);
        }
    }
}
