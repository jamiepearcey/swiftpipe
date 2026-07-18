# Testing Workflows

## Parser fuzz smoke

Install the fuzz runner when it is not already available:

```sh
cargo install cargo-fuzz --version 0.13.1 --locked
```

Run the malformed-block parser fuzz target from the `swift-core` crate:

```sh
cd crates/swift-core
cargo fuzz run parse_message_fuzz -- -max_total_time=60
```

Run the malformed-tag text-field scanner fuzz target from the same directory:

```sh
cargo fuzz run parse_text_fields_fuzz -- -max_total_time=60
```
