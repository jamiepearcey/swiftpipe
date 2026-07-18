#![no_main]

use libfuzzer_sys::fuzz_target;
use swift_core::parse_message;

fuzz_target!(|data: &[u8]| {
    let mut message = Vec::with_capacity(data.len() + 6);
    message.extend_from_slice(b"{4:\n");
    message.extend_from_slice(data);
    message.extend_from_slice(b"\n-}");
    let _ = parse_message(&message);
});
