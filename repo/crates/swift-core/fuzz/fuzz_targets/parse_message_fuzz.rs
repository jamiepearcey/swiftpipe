#![no_main]

use libfuzzer_sys::fuzz_target;
use swift_core::parse_message;

fuzz_target!(|data: &[u8]| {
    let _ = parse_message(data);
});
