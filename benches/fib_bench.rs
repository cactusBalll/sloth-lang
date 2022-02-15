use criterion::{black_box, criterion_group, criterion_main, Criterion};
use sloth_lang_core::*;
#[inline]
fn fib_bench0() {
    let prog = r#"
       func fib(n){
           if (n == 1 or n == 0) {
               return 1;
           } else {
               return fib(n - 1) + fib(n -2);
           }
       }
       print(fib(20));
    "#;
    run_string(prog).unwrap();
}
#[inline]
fn fib_bench1() {
    let prog = r#"
       func fib(n){
            var arr = Array(1,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0);
            var i = 2;
            while(i < n){
                assign arr[i] = arr[i-1] + arr[i-2];
                assign i = i + 1;
            }
            return arr[19]; 
       }
       print(fib(20));
    "#;
    run_string(prog).unwrap();
}
fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("fib 20", |b| b.iter(fib_bench0));
    c.bench_function("fib 20_iter", |b| b.iter(fib_bench1));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
