use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use super::Generator;

pub struct EmailGlobalGenerator;

impl Generator for EmailGlobalGenerator {
    fn id(&self) -> &'static str {
        "email_global"
    }

    fn class_id(&self) -> &'static str {
        "Email"
    }

    fn locale(&self) -> Option<&'static str> {
        None
    }

    fn generate(&self, seed: u64) -> String {
        let mut rng = StdRng::seed_from_u64(seed);
        format!(
            "{}{}@example.invalid",
            pick(&mut rng, LOCAL_PARTS),
            rng.gen_range(100..=999)
        )
    }
}

fn pick<'a>(rng: &mut StdRng, values: &'a [&'a str]) -> &'a str {
    values[rng.gen_range(0..values.len())]
}

const LOCAL_PARTS: &[&str] = &[
    "alice.synthetic",
    "blair.review",
    "casey.agent",
    "dana.audit",
    "evan.fixture",
    "morgan.clean",
];
