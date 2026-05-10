use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use super::Generator;

pub struct IbanDeGenerator;

impl Generator for IbanDeGenerator {
    fn id(&self) -> &'static str {
        "iban_de"
    }

    fn class_id(&self) -> &'static str {
        "custom:iban"
    }

    fn locale(&self) -> Option<&'static str> {
        None
    }

    fn generate(&self, seed: u64) -> String {
        let mut rng = StdRng::seed_from_u64(seed);
        let bank_code = random_digits(&mut rng, 8);
        let account = random_digits(&mut rng, 10);
        let bban = format!("{bank_code}{account}");
        let check_input = format!("{bban}131400");
        let check_digits = 98 - mod97(&check_input);
        format!("DE{check_digits:02}{bban}")
    }
}

fn random_digits(rng: &mut StdRng, len: usize) -> String {
    (0..len)
        .map(|idx| {
            let digit = if idx == 0 {
                rng.gen_range(1..=9)
            } else {
                rng.gen_range(0..=9)
            };
            char::from(b'0' + digit)
        })
        .collect()
}

fn mod97(digits: &str) -> u32 {
    digits
        .bytes()
        .fold(0_u32, |acc, byte| (acc * 10 + u32::from(byte - b'0')) % 97)
}
