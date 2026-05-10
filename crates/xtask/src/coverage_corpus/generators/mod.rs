use std::collections::HashMap;

#[allow(dead_code)]
pub trait Generator: Send + Sync {
    fn id(&self) -> &'static str;
    fn class_id(&self) -> &'static str;
    fn locale(&self) -> Option<&'static str>;
    fn generate(&self, seed: u64) -> String;
}

#[allow(dead_code)]
#[derive(Default)]
pub struct GeneratorRegistry {
    by_class_locale: HashMap<(&'static str, Option<&'static str>), Box<dyn Generator>>,
}

impl GeneratorRegistry {
    #[allow(dead_code)]
    pub fn default_phase_1() -> Self {
        Self::default()
    }

    #[allow(dead_code)]
    pub fn lookup(&self, class_id: &str, locale: Option<&str>) -> Option<&dyn Generator> {
        self.by_class_locale
            .iter()
            .find(|((registered_class, registered_locale), _)| {
                *registered_class == class_id && *registered_locale == locale
            })
            .map(|(_, generator)| generator.as_ref())
    }
}
