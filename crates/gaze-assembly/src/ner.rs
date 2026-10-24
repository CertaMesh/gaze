use gaze::PolicyError;
use gaze_recognizers::{DobJudgeRecognizer, NerOptions, NerRecognizer};

use crate::{registration::AssemblyBuilder, BuildError};

pub(crate) fn register_ner(
    builder: &mut AssemblyBuilder,
    policy: &gaze::Policy,
    ner_threshold: Option<f32>,
    resident: Option<&gaze::RecognizerRegistry>,
) -> Result<(), BuildError> {
    if let Some(ner) = &policy.ner {
        if let Some(path) = &ner.model_dir {
            if let Some(resident) = resident {
                return reuse_model(builder, resident, gaze::NER_RECOGNIZER_ID);
            }
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

pub(crate) fn register_dob_judge(
    builder: &mut AssemblyBuilder,
    policy: &gaze::Policy,
    resident: Option<&gaze::RecognizerRegistry>,
) -> Result<(), BuildError> {
    if let Some(config) = &policy.dob_judge {
        if let Some(resident) = resident {
            return reuse_model(builder, resident, DobJudgeRecognizer::ID);
        }
        builder.recognizer(DobJudgeRecognizer::load(
            &config.model_dir,
            config.threshold,
        )?);
    }
    Ok(())
}

fn reuse_model(
    builder: &mut AssemblyBuilder,
    resident: &gaze::RecognizerRegistry,
    id: &str,
) -> Result<(), BuildError> {
    let recognizer = resident
        .recognizer(id)
        .ok_or_else(|| PolicyError::NerLoad("resident model recognizer missing".into()))?;
    builder.recognizer_arc(std::sync::Arc::clone(recognizer));
    Ok(())
}
