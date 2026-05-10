use fake::faker::internet::en::SafeEmail;
use fake::Fake;
use rand::rngs::StdRng;
use rand::SeedableRng;

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
        SafeEmail().fake_with_rng(&mut rng)
    }
}
