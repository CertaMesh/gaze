use std::collections::HashMap;

mod email;
mod iban;

use email::EmailGlobalGenerator;
use iban::IbanDeGenerator;

pub trait Generator: Send + Sync {
    fn id(&self) -> &'static str;
    fn class_id(&self) -> &'static str;
    fn locale(&self) -> Option<&'static str>;
    fn generate(&self, seed: u64) -> String;
}

#[derive(Default)]
pub struct GeneratorRegistry {
    by_class_locale: HashMap<(&'static str, Option<&'static str>), Box<dyn Generator>>,
}

impl GeneratorRegistry {
    pub fn default_phase_1() -> Self {
        let mut registry = Self::default();
        registry.register(Box::new(EmailGlobalGenerator));
        registry.register(Box::new(IbanDeGenerator));
        registry
    }

    pub fn lookup(&self, class_id: &str, locale: Option<&str>) -> Option<&dyn Generator> {
        self.by_class_locale
            .iter()
            .find(|((registered_class, registered_locale), _)| {
                *registered_class == class_id && *registered_locale == locale
            })
            .or_else(|| {
                self.by_class_locale
                    .iter()
                    .find(|((registered_class, registered_locale), _)| {
                        *registered_class == class_id && registered_locale.is_none()
                    })
            })
            .map(|(_, generator)| generator.as_ref())
    }

    fn register(&mut self, generator: Box<dyn Generator>) {
        let key = (generator.class_id(), generator.locale());
        self.by_class_locale.insert(key, generator);
    }
}
