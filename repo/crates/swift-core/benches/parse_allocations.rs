use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use swift_core::parse_message;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

struct CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

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
    message
}

fn allocation_count_for(input: &[u8]) -> usize {
    ALLOCATIONS.store(0, Ordering::Relaxed);
    let parsed = parse_message(input);
    assert!(parsed.diagnostics.is_empty());
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    black_box(parsed);
    allocations
}

fn parse_allocation_benches(c: &mut Criterion) {
    let sample = semt_fixture();
    let large = large_fixture();
    let sample_allocations = allocation_count_for(sample);
    let large_allocations = allocation_count_for(&large);
    eprintln!("swift_core_parse_allocations/semt_structural allocations: {sample_allocations}");
    eprintln!("swift_core_parse_allocations/parse_500kb allocations: {large_allocations}");
    assert!(sample_allocations <= 2);
    assert!(large_allocations <= 2);

    let mut group = c.benchmark_group("swift_core_parse_allocations");
    group.bench_function("semt_structural", |bench| {
        bench.iter(|| black_box(allocation_count_for(black_box(sample))));
    });
    group.bench_function("parse_500kb", |bench| {
        bench.iter(|| black_box(allocation_count_for(black_box(&large))));
    });
    group.finish();
}

criterion_group!(benches, parse_allocation_benches);
criterion_main!(benches);
