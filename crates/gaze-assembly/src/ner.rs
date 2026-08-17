use gaze::PolicyError;
use gaze_recognizers::{NerOptions, NerRecognizer};

use crate::{registration::AssemblyBuilder, BuildError};

pub(crate) fn register_ner(
    builder: &mut AssemblyBuilder,
    policy: &gaze::Policy,
    ner_threshold: Option<f32>,
) -> Result<(), BuildError> {
    if let Some(ner) = &policy.ner {
        if let Some(path) = &ner.model_dir {
            let detector = NerRecognizer::load_with_options(
                path,
                NerOptions {
                    locale: ner.locale.clone(),
                    threshold: ner_threshold.unwrap_or(ner.threshold),
                },
            )
            .map_err(|err| PolicyError::NerLoad(err.to_string()))?;
            builder.recognizer(detector);
        }
    }

    Ok(())
}
