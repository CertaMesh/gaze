use std::collections::HashMap;

mod card_luhn;
mod common;
mod email;
mod eth_eip55;
mod iban;
mod ip_v4;
mod ip_v6;
mod name_de;
mod name_en;
mod phone_national_de;
mod phone_national_us;
mod phone_structural;
mod postal_de;
mod postal_us;

use card_luhn::CardLuhnGenerator;
use email::EmailGlobalGenerator;
use eth_eip55::EthEip55Generator;
use iban::IbanDeGenerator;
use ip_v4::Ipv4Generator;
use ip_v6::Ipv6Generator;
use name_de::NameDeGenerator;
use name_en::NameEnGenerator;
use phone_national_de::PhoneNationalDeGenerator;
use phone_national_us::PhoneNationalUsGenerator;
use phone_structural::PhoneStructuralGenerator;
use postal_de::PostalDeGenerator;
use postal_us::PostalUsGenerator;

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
        registry.register(Box::new(NameEnGenerator));
        registry.register(Box::new(NameDeGenerator));
        registry.register(Box::new(PhoneStructuralGenerator));
        registry.register(Box::new(PhoneNationalDeGenerator));
        registry.register(Box::new(PhoneNationalUsGenerator));
        registry.register(Box::new(CardLuhnGenerator));
        registry.register(Box::new(Ipv4Generator));
        registry.register(Box::new(Ipv6Generator));
        registry.register(Box::new(EthEip55Generator));
        registry.register(Box::new(PostalDeGenerator));
        registry.register(Box::new(PostalUsGenerator));
        registry
    }

    pub fn lookup(&self, class_id: &str, locale: Option<&str>) -> Option<&dyn Generator> {
        self.by_class_locale
            .iter()
            .find(|((registered_class, registered_locale), _)| {
                *registered_class == class_id && *registered_locale == locale
            })
            .or_else(|| {
                fallback_locale(class_id, locale).and_then(|fallback| {
                    self.by_class_locale.iter().find(
                        |((registered_class, registered_locale), _)| {
                            *registered_class == class_id && *registered_locale == Some(fallback)
                        },
                    )
                })
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

fn fallback_locale(class_id: &str, locale: Option<&str>) -> Option<&'static str> {
    match (class_id, locale) {
        ("Name" | "custom:postal_code", Some("global")) | ("Name" | "custom:postal_code", None) => {
            Some("en-US")
        }
        _ => None,
    }
}
