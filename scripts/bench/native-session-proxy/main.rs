//! Native benchmark host: each revision's full setup policy and explicit session continuity.
use std::{path::PathBuf, sync::Arc};

use gaze_proxy::adapters::{AnthropicAdapter, GeminiAdapter, OpenAiAdapter};
use gaze_proxy::{ProviderAdapter, ProxyConfig, SessionRegistryConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("expected bind, setup policy, and synthetic upstream URL".into());
    }
    let policy = gaze::Policy::load_for_cli(&PathBuf::from(&args[2]))?;
    if policy.safety_net.backend != gaze::SafetyNetPolicyBackend::Nym {
        return Err("native session capture requires the own-policy Nym arm".into());
    }
    let inputs = gaze_assembly::resolve_policy_inputs(&policy, None, None, None)?;
    let context = gaze::Context::from_json_str("{}")?;
    let pipeline = gaze_assembly::build_pipeline(
        &policy,
        &context,
        &inputs.rulepacks,
        &inputs.locale_chain,
        Some(inputs.ner_threshold),
    )?;
    let model = std::env::var_os("GAZE_NYM_MODEL_DIR").map(PathBuf::from);
    let pipeline =
        gaze_assembly::attach_nym_safety_net(pipeline, &policy, model.as_deref(), None, None)?;
    let upstream = url::Url::parse(&args[3])?;
    let adapters: Vec<Arc<dyn ProviderAdapter>> = vec![
        Arc::new(OpenAiAdapter::new(upstream.clone())),
        Arc::new(GeminiAdapter::new(upstream.clone())),
        Arc::new(
            AnthropicAdapter::builder(upstream)
                .session_registry_config(SessionRegistryConfig::default())
                .build()?,
        ),
    ];
    let config = ProxyConfig::new(args[1].parse()?, adapters)
        .with_locale_chain(inputs.locale_chain)
        .with_dictionaries(inputs.dictionaries);
    gaze_proxy::server::serve(config, Arc::new(pipeline)).await?;
    Ok(())
}
