use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use super::common::{luhn_valid, random_digit};
use super::Generator;

pub struct CardLuhnGenerator;

impl Generator for CardLuhnGenerator {
    fn id(&self) -> &'static str {
        "card_luhn"
    }

    fn class_id(&self) -> &'static str {
        "custom:credit_card"
    }

    fn locale(&self) -> Option<&'static str> {
        None
    }

    fn generate(&self, seed: u64) -> String {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut digits = vec![
            4,
            rng.gen_range(0..=9),
            rng.gen_range(0..=9),
            rng.gen_range(0..=9),
        ];
        while digits.len() < 15 {
            digits.push(random_digit(&mut rng));
        }
        digits.push(check_digit(&digits));
        digits
            .chunks(4)
            .map(|chunk| {
                chunk
                    .iter()
                    .map(|digit| char::from(b'0' + digit))
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn check_digit(prefix: &[u8]) -> u8 {
    for digit in 0..=9 {
        let mut candidate = prefix.to_vec();
        candidate.push(digit);
        let value = candidate
            .iter()
            .map(|digit| char::from(b'0' + digit))
            .collect::<String>();
        if luhn_valid(&value) {
            return digit;
        }
    }
    unreachable!("one Luhn check digit must be valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_value_passes_class_validator() {
        let generator = CardLuhnGenerator;
        for seed in 0..100 {
            assert!(luhn_valid(&generator.generate(seed)));
        }
    }
}
