//! Criterion benchmarks for the switch-based bytecode VM.
//!
//! The same programs are also compiled with the AOT template backend and run
//! against CPython by `benches/compare.py`; see `BENCHMARKS.md`.

use criterion::measurement::WallTime;
use criterion::{criterion_group, criterion_main, BenchmarkGroup, Criterion};
use sloth_lang_core::run_string;
use std::hint::black_box;
use std::time::Duration;

const FIB_REC: &str = include_str!("programs/fib_rec.slt");
const LOOP_ARITH: &str = include_str!("programs/loop_arith.slt");
const SIEVE: &str = include_str!("programs/sieve.slt");
const OBJECT_METHODS: &str = include_str!("programs/object_methods.slt");
const DICT_LOOKUP: &str = include_str!("programs/dict_lookup.slt");
const STRING_OPS: &str = include_str!("programs/string_ops.slt");

fn bench_program(group: &mut BenchmarkGroup<'_, WallTime>, name: &str, source: &'static str) {
    group.bench_function(name, |b| {
        b.iter(|| black_box(run_string(source, false).unwrap()))
    });
}

fn interpreter_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("switch_vm");
    // The benchmark programs run for roughly 0.3-1.3 s each, so a small
    // sample count keeps the suite fast while still giving a stable median.
    group
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(5));

    bench_program(&mut group, "fib_rec", FIB_REC);
    bench_program(&mut group, "loop_arith", LOOP_ARITH);
    bench_program(&mut group, "sieve", SIEVE);
    bench_program(&mut group, "object_methods", OBJECT_METHODS);
    bench_program(&mut group, "dict_lookup", DICT_LOOKUP);
    bench_program(&mut group, "string_ops", STRING_OPS);

    group.finish();
}

criterion_group!(benches, interpreter_benchmark);
criterion_main!(benches);
