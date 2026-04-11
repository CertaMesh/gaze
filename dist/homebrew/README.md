# Ghostwriter Homebrew Formula

This formula installs `ghostwriter` from source using `cargo`.

## Publish to naoray/homebrew-tap

1. Push the `ghostwriter-v0.1` branch (or its merged main commit) to GitHub:

   ```bash
   git push origin ghostwriter-v0.1
   ```

2. In a clone of `naoray/homebrew-tap`, copy this formula into `Formula/`:

   ```bash
   cp dist/homebrew/ghostwriter.rb /path/to/homebrew-tap/Formula/ghostwriter.rb
   ```

3. Update the `url` field to point at a specific tag tarball once a release
   tag exists. Until then, users can install HEAD:

   ```bash
   brew tap naoray/tap
   brew install --HEAD naoray/tap/ghostwriter
   ```

4. Commit and push to the tap repo.

## Local dev install (no tap required)

```bash
cargo install --path crates/ghostwriter --force
```

This installs `ghostwriter` into `~/.cargo/bin`.

## Smoke test

```bash
echo '{"text":"Hi Markus Mueller","context":{"customer_name":"Markus Mueller"}}' \
  | ghostwriter sanitize
```
