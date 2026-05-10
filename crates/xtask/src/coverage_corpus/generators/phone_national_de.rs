use rand::rngs::StdRng;
use rand::SeedableRng;

use super::common::pick;
use super::Generator;

pub struct PhoneNationalDeGenerator;

impl Generator for PhoneNationalDeGenerator {
    fn id(&self) -> &'static str {
        "phone_national_de"
    }

    fn class_id(&self) -> &'static str {
        "custom:phone"
    }

    fn locale(&self) -> Option<&'static str> {
        Some("de-DE")
    }

    fn generate(&self, seed: u64) -> String {
        let mut rng = StdRng::seed_from_u64(seed);
        pick(&mut rng, VALUES).to_string()
    }
}

const VALUES: &[&str] = &[
    "+49 1555 0112233",
    "015550112233",
    "+49 30 00000000",
    "01710000000",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_value_passes_class_validator() {
        let generator = PhoneNationalDeGenerator;
        for seed in 0..100 {
            let phone =
                phonenumber::parse(Some(phonenumber::country::DE), generator.generate(seed))
                    .expect("parse DE phone");
            assert_eq!(phone.country().code(), 49);
        }
    }
}
