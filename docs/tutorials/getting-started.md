# Protect a model call from Rust

We will build a Rust program that sends tokens to a mock model and restores
its reply with the original session snapshot. Allow ten minutes.

You need Cargo and Rust 1.89 or newer. Start outside an existing Cargo workspace.
For the CLI path, start with
[Your first pseudonymization](first-pseudonymization.md).

## 1. Create a project

First, create a binary and add the core and assembly crates:

```sh
cargo new gaze-model-call --bin --vcs none
cd gaze-model-call
cargo add gaze-pii gaze-assembly
printf 'Project ready\n'
```

Cargo reports the dependencies it adds, followed by:

```text
Project ready
```

The package is `gaze-pii`; Rust imports it as `gaze`.

## 2. Write the model loop

Now, replace `src/main.rs`:

```sh
cat > src/main.rs <<'RS'
use gaze::{CleanDocument, Scope, SensitiveSnapshot, Session};
use gaze_assembly::CorePipelineConfig;

fn mock_model(clean_text: &str) -> String {
	format!("Reply: {clean_text}")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let pipeline = CorePipelineConfig::new().build()?;
	let session = Session::new(Scope::Conversation("demo".into()))?;
	let original = format!("Contact {}@{}.", "alice", "example.invalid");
	let cleaned = pipeline.pseudonymize_text(&session, original.clone())?;
	let CleanDocument::Text(clean_text) = cleaned else {
		return Err("expected text output".into());
	};
	assert!(!clean_text.contains("example.invalid"));
	assert!(clean_text.contains(":Email_"));
	println!("Model receives: {clean_text}");

	let owner_blob = session.export()?.into_bytes();
	let reply = mock_model(&clean_text);
	let restored_session = Session::import(SensitiveSnapshot::from(owner_blob))?;
	let restored = restored_session.restore_strict_text(&reply)?;
	assert_eq!(restored, format!("Reply: {original}"));
	println!("Restored reply matches: true");
	Ok(())
}
RS
printf 'Saved src/main.rs\n'
```

You should see:

```text
Saved src/main.rs
```

Only `clean_text` crosses the mock model boundary. Keep `owner_blob` private;
a production app stores snapshots encrypted and bound to the conversation.

## 3. Run the program

Run the complete loop:

```sh
cargo run --quiet
```

The program prints the protected input and confirms restore:

```text
Model receives: Contact <{session_hex}:Email_1>.
Restored reply matches: true
```

The eight-character session prefix changes on each run. The assertions check
that the model sees a token and that restore preserves the original reply.

This example uses bundled deterministic rules. To load the setup policy's
NER and Nym models, follow [Use the setup policy from Rust](../how-to/rust-library.md).
For isolation and lifetime rules, see the [session contract](../explanation/core/session-contract.md).
