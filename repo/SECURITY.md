# Security Policy

## Tool Installation Pinning

All `cargo install` invocations in scripts, CI, Dockerfiles, and docs must pass both:

- `--version <exact-version>`
- `--locked`

This keeps CI and local workflows reproducible and avoids implicitly trusting a new crates.io release during hardening checks. When bumping a tool, update every matching invocation in the same change and run the relevant CI command locally where practical.

## GitHub Actions Pinning

All workflow `uses:` entries must be pinned to a full commit SHA and keep the human-readable ref in a trailing comment, for example:

```yaml
uses: actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5 # v4
```

When upgrading an action, resolve the intended tag or branch with `git ls-remote`, replace the SHA, update the trailing comment if the ref changed, and run the affected CI job locally where practical. Dependabot may propose action updates, but the resulting workflow still needs an immutable SHA before merge.
