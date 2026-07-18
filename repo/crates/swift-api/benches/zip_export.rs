use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const ENTRY_COUNT: usize = 100;
const ENTRY_BYTES: usize = 16 * 1024;
const PREFIX_URI: &str = "s3://swiftpipe-outbox/jobs/bench/artifacts/";
const ZIP_URI: &str = "s3://swiftpipe-outbox/jobs/bench/exports.zip";

fn fixture() -> TempDir {
    let temp = tempfile::tempdir().expect("temp dir");
    let root = temp
        .path()
        .join("swiftpipe-outbox")
        .join("jobs")
        .join("bench")
        .join("artifacts");
    fs::create_dir_all(&root).expect("artifact dir");

    let payload = payload();
    for index in 0..ENTRY_COUNT {
        let path = root.join(format!("artifact-{index:03}.txt"));
        fs::write(path, &payload).expect("artifact write");
    }

    temp
}

fn payload() -> Vec<u8> {
    let mut payload = Vec::with_capacity(ENTRY_BYTES);
    let line = b":20C::SEME//BENCH00000000\n:35B:ISIN GB00B03MLX29 SWIFTPIPE BENCH PAYLOAD\n";
    while payload.len() < ENTRY_BYTES {
        payload.extend_from_slice(line);
    }
    payload.truncate(ENTRY_BYTES);
    payload
}

fn write_zip(root: &Path) {
    swift_api::write_zip_for_bench(
        root,
        PREFIX_URI,
        ZIP_URI,
        u64::try_from(ENTRY_COUNT * ENTRY_BYTES * 2).expect("zip cap"),
        ENTRY_COUNT + 1,
    )
    .expect("zip export");
}

fn zip_export_benches(c: &mut Criterion) {
    let sample_fixture = fixture();
    write_zip(sample_fixture.path());

    let mut group = c.benchmark_group("swift_api_zip_export");
    group.throughput(Throughput::Bytes(
        u64::try_from(ENTRY_COUNT * ENTRY_BYTES).expect("fixture size"),
    ));
    group.bench_function("100_text_artifacts", |bench| {
        bench.iter_batched(
            fixture,
            |fixture| {
                write_zip(black_box(fixture.path()));
                black_box(fixture);
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(benches, zip_export_benches);
criterion_main!(benches);
