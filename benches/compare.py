#!/usr/bin/env python3
"""Compare the sloth switch VM, the AOT template backend, and CPython.

The script builds the release interpreter and runtime, compiles every program
in ``benches/programs`` with the AOT backend, then runs each workload several
times and reports medians.  It is intentionally independent of Criterion so
that in-process interpreter runs and native/Python process runs can be compared
with the same wall-clock methodology.

Usage:
    python3 benches/compare.py
    python3 benches/compare.py --reps 9 --cpu 3 --python python3.12
"""

from __future__ import annotations

import argparse
import json
import shutil
import statistics
import subprocess
import sys
import time
from dataclasses import dataclass, asdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PROGRAMS = ROOT / "benches" / "programs"
PYTHON_DIR = ROOT / "benches" / "python"
BENCH_BIN = ROOT / "target" / "bench-bin"
SLOTH_BIN = ROOT / "target" / "release" / "sloth-lang-core"
RUNTIME_LIB = ROOT / "target" / "sloth-runtime" / "release" / "libsloth_runtime.a"


@dataclass
class Result:
    program: str
    switch_ms: float
    aot_ms: float
    python_ms: float

    @property
    def aot_speedup(self) -> float:
        return self.switch_ms / self.aot_ms

    @property
    def switch_vs_python(self) -> float:
        return self.switch_ms / self.python_ms

    @property
    def aot_vs_python(self) -> float:
        return self.aot_ms / self.python_ms


def run(cmd: list[str], what: str) -> None:
    proc = subprocess.run(cmd, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if proc.returncode != 0:
        sys.stderr.write(proc.stdout.decode(errors="replace"))
        sys.stderr.write(proc.stderr.decode(errors="replace"))
        raise SystemExit(f"{what} failed (exit {proc.returncode})")


def build(verbose: bool = True) -> None:
    run(["cargo", "build", "--release", "--bin", "sloth-lang-core"], "interpreter build")
    run(
        [
            "cargo",
            "build",
            "--release",
            "-p",
            "sloth-runtime",
            "--target-dir",
            "target/sloth-runtime",
        ],
        "runtime build",
    )
    BENCH_BIN.mkdir(parents=True, exist_ok=True)
    for program in sorted(PROGRAMS.glob("*.slt")):
        out = BENCH_BIN / program.stem
        run(
            [
                str(SLOTH_BIN),
                "build",
                str(program),
                "-o",
                str(out),
                "--runtime-lib",
                str(RUNTIME_LIB),
            ],
            f"AOT build {program.name}",
        )
        if verbose:
            print(f"built {out.relative_to(ROOT)}", file=sys.stderr)


def time_process(cmd: list[str], reps: int) -> list[float]:
    # One warm-up run, then `reps` measured runs with output discarded.
    proc = subprocess.run(
        cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE
    )
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr.decode(errors="replace"))
        raise SystemExit(f"command failed: {' '.join(cmd)}")
    samples = []
    for _ in range(reps):
        start = time.perf_counter()
        proc = subprocess.run(
            cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE
        )
        elapsed = time.perf_counter() - start
        if proc.returncode != 0:
            sys.stderr.write(proc.stderr.decode(errors="replace"))
            raise SystemExit(f"command failed: {' '.join(cmd)}")
        samples.append(elapsed * 1000.0)
    return samples


def fmt_ms(value: float) -> str:
    if value >= 1000:
        return f"{value / 1000:.2f} s"
    return f"{value:.1f} ms"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reps", type=int, default=7, help="measured runs per configuration")
    parser.add_argument("--cpu", type=int, default=2, help="CPU to pin to with taskset")
    parser.add_argument("--python", default=sys.executable, help="Python executable to benchmark")
    parser.add_argument("--no-build", action="store_true", help="reuse existing binaries")
    parser.add_argument("--json", type=Path, help="write raw results as JSON")
    args = parser.parse_args()

    if not args.no_build:
        build()

    for path in (SLOTH_BIN, RUNTIME_LIB):
        if not path.exists():
            raise SystemExit(f"missing {path}; run without --no-build first")

    prefix: list[str] = []
    if shutil.which("taskset") and args.cpu is not None:
        prefix = ["taskset", "-c", str(args.cpu)]

    results: list[Result] = []
    for program in sorted(PROGRAMS.glob("*.slt")):
        name = program.stem
        aot = BENCH_BIN / name
        py = PYTHON_DIR / f"{name}.py"
        if not aot.exists():
            raise SystemExit(f"missing AOT binary {aot}; run without --no-build first")
        if not py.exists():
            raise SystemExit(f"missing Python equivalent {py}")

        switch_samples = time_process(prefix + [str(SLOTH_BIN), "run", str(program)], args.reps)
        aot_samples = time_process(prefix + [str(aot)], args.reps)
        py_samples = time_process(prefix + [args.python, str(py)], args.reps)

        result = Result(
            program=name,
            switch_ms=statistics.median(switch_samples),
            aot_ms=statistics.median(aot_samples),
            python_ms=statistics.median(py_samples),
        )
        results.append(result)
        print(
            f"{name:16s} switch={fmt_ms(result.switch_ms):>10s}  "
            f"AOT={fmt_ms(result.aot_ms):>10s}  "
            f"CPython={fmt_ms(result.python_ms):>10s}",
            file=sys.stderr,
        )

    print()
    print("| Program | switch VM | AOT template | CPython | AOT vs switch | switch vs CPython | AOT vs CPython |")
    print("|---|---:|---:|---:|---:|---:|---:|")
    for r in results:
        print(
            f"| `{r.program}` | {fmt_ms(r.switch_ms)} | {fmt_ms(r.aot_ms)} | "
            f"{fmt_ms(r.python_ms)} | {r.aot_speedup:.2f}x | "
            f"{r.switch_vs_python:.2f}x slower | {r.aot_vs_python:.2f}x slower |"
        )

    if args.json:
        args.json.write_text(json.dumps([asdict(r) for r in results], indent=2))
        print(f"\nwrote {args.json}", file=sys.stderr)


if __name__ == "__main__":
    main()
