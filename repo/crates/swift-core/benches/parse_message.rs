use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use swift_core::{parse_message, ParsedMessage};

// Expected parser throughput baseline: at least 400 MiB/s on a 2024 MBP.
// repo/README.md currently records local core parser microbenchmarks around
// 509-514 MiB/s; this large-input case is intended to catch allocator-pressure
// regressions without changing the parser's zero-copy contract.
const TARGET_LARGE_MESSAGE_BYTES: usize = 500 * 1024;

fn semt_fixture() -> &'static [u8] {
    br#"{1:F01BANKBEBBAXXX0000000000}{2:I540BANKDEFFXXXXN}{3:{108:ABC123}{121:550e8400-e29b-41d4-a716-446655440000}}{4:
:16R:GENL
:20C::SEME//ABC123
:23G:NEWM
:98A::PREP//20260511
:16R:LINK
:20C::RELA//REL1
:16S:LINK
:16R:TRADDET
:98A::TRAD//20260510
:35B:ISIN GB00B03MLX29
ACME PLC ORD
:16R:FIAC
:36B::SETT//UNIT/1000,
:97A::SAFE//123456789
:16S:FIAC
:16S:TRADDET
:16S:GENL
-}{5:{CHK:123456789ABC}}"#
}

fn large_fixture() -> Vec<u8> {
    let mut message = Vec::from(
        &b"{1:F01BANKBEBBAXXX0000000000}{2:I540BANKDEFFXXXXN}{3:{108:BENCH500}}{4:\n"[..],
    );
    let suffix = b"-}{5:{CHK:123456789ABC}}";
    let mut index = 0_u32;

    while message.len() + suffix.len() < TARGET_LARGE_MESSAGE_BYTES {
        message.extend_from_slice(b":35B:ISIN GB00B03MLX29 ");
        message.extend_from_slice(format!("BENCH{index:08}").as_bytes());
        message.extend_from_slice(b" ");
        message.extend_from_slice(
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZABCDEFGHIJKLMNOPQRSTUVWXYZABCDEFGHIJKLMNOPQRSTUV",
        );
        message.extend_from_slice(b"\n");
        index += 1;
    }

    message.extend_from_slice(suffix);
    assert!(message.len() >= TARGET_LARGE_MESSAGE_BYTES);
    assert!(message.len() <= TARGET_LARGE_MESSAGE_BYTES + 128);
    message
}

fn assert_zero_copy(input: &[u8], parsed: &ParsedMessage<'_>) {
    for block in &parsed.blocks {
        assert_slice_borrows_input(input, block.raw);
        assert_slice_borrows_input(input, block.content);
    }
    for field in &parsed.fields {
        assert_slice_borrows_input(input, field.tag);
        assert_slice_borrows_input(input, field.value);
        if let Some(qualifier) = field.qualifier {
            assert_slice_borrows_input(input, qualifier);
        }
        for frame in &field.sequence_path {
            assert_slice_borrows_input(input, frame.name);
        }
    }
}

fn assert_slice_borrows_input(input: &[u8], slice: &[u8]) {
    let input_start = input.as_ptr() as usize;
    let input_end = input_start + input.len();
    let slice_start = slice.as_ptr() as usize;
    let slice_end = slice_start + slice.len();

    assert!(slice_start >= input_start);
    assert!(slice_end <= input_end);
}

fn parse_message_benches(c: &mut Criterion) {
    let sample = semt_fixture();
    let large = large_fixture();
    let parsed_large = parse_message(&large);
    assert!(parsed_large.diagnostics.is_empty());
    assert_zero_copy(&large, &parsed_large);

    let mut group = c.benchmark_group("swift_core_parse_message");
    group.throughput(Throughput::Bytes(sample.len() as u64));
    group.bench_function("semt_structural", |bench| {
        bench.iter(|| {
            let parsed = parse_message(black_box(sample));
            black_box(parsed);
        });
    });

    group.throughput(Throughput::Bytes(large.len() as u64));
    group.bench_function("parse_500kb", |bench| {
        bench.iter(|| {
            let parsed = parse_message(black_box(&large));
            black_box(parsed);
        });
    });
    group.finish();
}

criterion_group!(benches, parse_message_benches);
criterion_main!(benches);
