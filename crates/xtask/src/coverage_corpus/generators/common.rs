use rand::rngs::StdRng;
use rand::Rng;

pub fn pick<'a>(rng: &mut StdRng, values: &'a [&'a str]) -> &'a str {
    values[rng.gen_range(0..values.len())]
}

pub fn random_digit(rng: &mut StdRng) -> u8 {
    rng.gen_range(0..=9)
}

pub fn luhn_valid(input: &str) -> bool {
    let digits = input
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace() && *byte != b'-')
        .map(|byte| byte - b'0')
        .collect::<Vec<_>>();
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(index, digit)| {
            let mut value = u32::from(*digit);
            if index % 2 == 1 {
                value *= 2;
                if value > 9 {
                    value -= 9;
                }
            }
            value
        })
        .sum();
    (13..=19).contains(&digits.len()) && sum.is_multiple_of(10)
}
