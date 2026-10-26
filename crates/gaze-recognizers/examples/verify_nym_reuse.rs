use std::io::{self, BufRead};

use gaze_recognizers::safety_net::nym::{test_support, NymSafetyNet};
use serde::Deserialize;

#[derive(Deserialize)]
struct Input {
    text: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let net = NymSafetyNet::from_env()?;
    let mut documents = 0;
    let mut windows = 0;
    for line in io::stdin().lock().lines() {
        let input: Input = serde_json::from_str(&line?)?;
        windows += test_support::verify_window_reuse(&net, &input.text)?;
        documents += 1;
        println!(
            "{}",
            serde_json::json!({"documents": documents, "verified_windows": windows})
        );
    }
    if documents == 0 || windows == 0 {
        return Err("window equivalence proof executed no model windows".into());
    }
    Ok(())
}
