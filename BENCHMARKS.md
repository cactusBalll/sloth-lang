# Sloth 性能基准：P0 优化后的 AOT 后端 vs switch 解释器 vs CPython

本文评估 P0 优化（AOT 代码生成第 1–4 项）落地后的执行性能：

- **switch 解释器**：`Vm::step()` 从变长字节码解码一条 `Instr`，再进入
  `Vm::exec()` 的 `match instr` 分发。
- **AOT 后端**：`sloth build` 把每个 `Chunk` 的每条字节码编译成一个 LLVM
  基本块，再链接 `libsloth_runtime.a`。优化后，热路径直接在原生代码中读写
  VM 的值栈，只有帧切换和复杂操作才进入 runtime ABI。
- 对比基线是 **CPython 3.12.13**。

## P0 优化内容

本轮实现对应 `benchmark.md` 的以下建议：

1. **不再逐指令同步 `pc`**。基本块内的跳转直接生成 `br`；只在调用 runtime
   慢路径（需要错误行号或 `Call`/`Return` 语义）以及离开当前 chunk 时同步
   `pc`。
2. **把栈指针/frame base 暴露给生成代码**。runtime 导出
   `@sloth_rt_stack_data`、`@sloth_rt_stack_len`、`@sloth_rt_stack_cap` 和
   `@sloth_rt_frame_base`；`Push`/`Pop`/`GetLocal`/`SetLocal`/`Swap2`/真值判断
   直接在 IR 中完成，不再调用 `sloth_rt_push`、`sloth_rt_pop`、
   `sloth_rt_peek_raw`、`sloth_rt_local_get`、`sloth_rt_local_set`。
3. **内联更多常用快路径**：`Div`/`Mod` 的数值路径、`And`/`Or` 的布尔路径；
   `GetGlobal`/`SetGlobal`、集合访问、`Call`/`Return`、`Iterator`/`Next`、
   `InitArray`/`InitDict` 等改为专用 ABI。
4. **删除通用 `sloth_rt_exec` 兜底**。每个剩余操作都有独立入口，不再经历
   `OpCode::from_u8` → 构造 `Instr` → `Vm::exec` 的通用分发。

栈上的引用计数仍然正确：`GetLocal` 在复制字符串/boxed 值前调用
`sloth_rt_retain`，`Pop`/`SetLocal` 覆盖旧值前调用 `sloth_rt_release`；数值
和 GC 指针不进入引用计数分支。

## 结论摘要

1. **AOT 相对 switch 解释器提升明显**：6 个程序为 **1.13x–3.60x**，
   几何平均 **2.43x**；总墙钟时间从 **4.63 s** 降到 **1.80 s**（**2.57x**）。
2. **相对 CPython 3.12**：
   - switch 解释器慢 **6.5x–10.5x**（几何平均 **7.6x**）；
   - AOT 后端慢 **1.8x–7.6x**（几何平均 **3.1x**）。
3. **微基准提升最大**：
   - 纯局部变量空循环：AOT **21.6x**（P0 前 4.18x）；
   - 局部变量算术循环：**10.0x**（P0 前 3.57x）；
   - 顶层全局变量算术循环：**3.60x**（P0 前 1.96x）。
4. 剩余最大缺口是 **字符串**（1.06x）：`string_ops` 的主要成本仍是
   `StringPool` 线性扫描和 `StringIter` 的 `chars().skip(i)`，属于 P1 范围。
   集合访问、方法调用、`Call`/`Return` 已改为专用 ABI，但仍保留完整的
   `HashMap`/帧管理逻辑。

## 环境与测量方法

| 项目 | 值 |
|---|---|
| CPU | AMD Ryzen 7 H 260 (16 vCPU, WSL2) |
| OS | Linux 6.18.54.1-microsoft-standard-WSL2 x86-64 |
| Rust | rustc 1.99.0-nightly |
| LLVM | llc-21 (LLVM 21.1.8) |
| CPython | 3.12.13 |
| 编译 | `cargo build --release`；AOT 使用 `llc -O2` + release runtime |
| 计时 | 每个配置先 warm-up 1 次，再测 9 次，取中位数；`taskset -c 2` 绑核 |
| 输出 | 子进程 stdout 丢弃，避免 I/O 干扰 |

复现：

```bash
cargo build --release --bin sloth-lang-core
cargo build --release -p sloth-runtime --target-dir target/sloth-runtime
python3 benches/compare.py --python python3.12 --reps 9 --cpu 2
cargo bench --bench interpreter_bench
```

> **工具链注意**：`benches/compare.py` 显式传入 release runtime：
> `--runtime-lib target/sloth-runtime/release/libsloth_runtime.a`。当前
> `find_runtime_lib()` 会先找 `target/sloth-runtime/debug`；如果误链 debug
> runtime，AOT 程序会显著变慢。

## 基准程序

程序位于 `benches/programs/`，Python 等价版本位于 `benches/python/`。

| 程序 | 负载 | 主要覆盖的指令/路径 |
|---|---|---|
| `fib_rec.slt` | `fib(30)`，832040 | 函数调用/返回、局部变量、比较、分支、加法 |
| `loop_arith.slt` | 2,000,000 次 `while` 算术循环 | 顶层全局变量、`Add/Sub/Mul/Mod`、比较、跳转 |
| `sieve.slt` | 500,000 的埃氏筛 | 数组创建/索引/赋值、嵌套循环、`push` 方法 |
| `object_methods.slt` | 300,000 次对象方法/字段访问 | 类实例化、字段读写、绑定方法、返回 |
| `dict_lookup.slt` | 1,000,000 次字典查找 | 字典字面量、字符串键哈希、`GetCollection` |
| `string_ops.slt` | 10,000 次字符串拼接 + 100,000 字符扫描 | 字符串拼接、迭代器、逐字符比较 |

## 主结果

单位 ms；`AOT vs switch` 越大越好，`vs CPython` 越小越好。

| 程序 | switch | AOT | CPython 3.12 | AOT vs switch | switch vs CPython | AOT vs CPython |
|---|---:|---:|---:|---:|---:|---:|
| `fib_rec` | 822.4 | 239.8 | 78.4 | **3.43x** | 10.49x slower | 3.06x slower |
| `loop_arith` | 1252.8 | 347.7 | 191.6 | **3.60x** | 6.54x slower | 1.82x slower |
| `sieve` | 974.2 | 347.9 | 131.1 | **2.80x** | 7.43x slower | 2.65x slower |
| `object_methods` | 380.1 | 176.3 | 60.7 | **2.16x** | 6.26x slower | 2.90x slower |
| `dict_lookup` | 802.0 | 332.9 | 114.2 | **2.41x** | 7.02x slower | 2.91x slower |
| `string_ops` | 402.3 | 355.4 | 46.8 | **1.13x** | 8.59x slower | 7.59x slower |
| **合计/几何平均** | **4634** | **1800** | **622** | **2.43x geo / 2.57x total** | **7.6x geo** | **3.1x geo** |

### 优化前后 AOT 对比

同一台机器、同一组程序，P0 前的 AOT 数据取自旧版本报告：

| 程序 | P0 前 AOT | 当前 AOT | 变化 |
|---|---:|---:|---:|
| `fib_rec` | 350.3 ms | 239.8 ms | 1.46x |
| `loop_arith` | 638.9 ms | 347.7 ms | 1.84x |
| `sieve` | 587.1 ms | 347.9 ms | 1.69x |
| `object_methods` | 302.8 ms | 176.3 ms | 1.72x* |
| `dict_lookup` | 486.6 ms | 332.9 ms | 1.46x |
| `string_ops` | 358.8 ms | 355.4 ms | 1.01x（噪声范围内） |

\* `object_methods` 同时还受益于本轮之前的“方法调用快路径”提交；P0 主要
改善了它的调用分发、字段访问和局部变量路径。

### 逐项解读

- **`fib_rec`（3.43x）**：调用/返回仍通过专用 ABI 和 `CallFrame`，但局部变量
  读写、算术、比较、真值判断和跳转全部在生成代码中完成，且不再逐指令同步
  `pc`。
- **`loop_arith`（3.60x）**：`GetGlobal`/`SetGlobal` 改为专用 ABI，`Mod`、
  `Div`、`And`、`Or` 有内联快路径；顶层全局变量不再落入通用
  `sloth_rt_exec`。
- **`sieve`（2.80x）**：数组创建、索引和赋值走专用 ABI；循环变量、比较和
  跳转完全内联。
- **`object_methods`（2.16x）**：方法调用使用专用 `InvokeProperty` ABI，
  接收者通过 `CallFrame.bound_this` 传递，不再为每次方法访问分配绑定闭包。
- **`dict_lookup`（2.41x）**：字符串键哈希和字典查找仍在 runtime 中完成，
  但已没有通用 opcode 重建和二次分发。
- **`string_ops`（1.13x）**：字符串拼接和迭代器仍是慢路径；`StringPool`
  线性扫描和 `chars().skip(i)` 的 O(n²) 属于 P1 优化范围。

## 微基准

| 微基准（2,000,000 次） | switch | AOT | AOT 提升 | P0 前 AOT 提升 |
|---|---:|---:|---:|---:|
| 纯局部变量空循环 | 404.2 ms | 18.7 ms | **21.6x** | 4.18x |
| 局部变量算术循环 | 908.6 ms | 90.8 ms | **10.0x** | 3.57x |
| 顶层全局变量算术循环 | 1252.8 ms | 347.7 ms | **3.60x** | 1.96x |

空循环每迭代约 **156** 条指令（P0 前 894 条，CPython 3.12 为 391 条），说明
直接栈访问和取消逐指令 `set_pc` 确实消除了主要的分发开销。

微基准源码位于 `benches/micro/`。构建 AOT 二进制时同样必须显式链接 release
runtime：

```bash
target/release/sloth-lang-core build benches/micro/empty_loop.slt \
  -o target/bench-bin/empty_loop \
  --runtime-lib target/sloth-runtime/release/libsloth_runtime.a
```

## 指令数与 IPC

`perf stat -e instructions,cycles` 统计的是每次运行的总指令数（含启动），
单位 10 亿条（B）：

| 程序 | switch insn | AOT insn | switch/py | AOT/py | AOT 指令下降 |
|---|---:|---:|---:|---:|---:|
| `fib_rec` | 12.16 | 3.66 | 8.9x | 2.7x | 3.32x |
| `loop_arith` | 17.85 | 4.88 | 5.0x | 1.4x | 3.66x |
| `sieve` | 14.07 | 5.05 | 5.7x | 2.1x | 2.79x |
| `object_methods` | 5.43 | 2.59 | 6.2x | 2.9x | 2.10x |
| `dict_lookup` | 11.53 | 4.77 | 5.4x | 2.2x | 2.42x |
| `string_ops` | 5.43 | 5.01 | 54x | 50x | 1.08x |

典型 IPC：

| 程序 | switch | AOT |
|---|---:|---:|
| `fib_rec` | 3.25 | 3.27 |
| `loop_arith` | 2.86 | 3.06 |
| `sieve` | 3.15 | 3.11 |
| `object_methods` | 3.16 | 3.22 |
| `dict_lookup` | 3.17 | 3.13 |
| `string_ops` | 3.57 | 3.64 |

结论：AOT 的收益主要来自**执行更少的指令**，而不是提高 IPC。`fib_rec` 的
指令数降到原来的约 1/3.3，`loop_arith` 降到约 1/3.7。

## 峰值 RSS

`/usr/bin/time -f %M`：

| 程序 | switch | AOT | CPython 3.12 |
|---|---:|---:|---:|
| `fib_rec` | 3.5 MB | 2.6 MB | 10.2 MB |
| `loop_arith` | 3.6 MB | 2.6 MB | 10.1 MB |
| `sieve` | 7.6 MB | 6.4 MB | 14.0 MB |
| `object_methods` | 3.7 MB | 2.5 MB | 10.1 MB |
| `dict_lookup` | 3.7 MB | 2.7 MB | 10.2 MB |
| `string_ops` | 4.4 MB | 3.4 MB | 10.4 MB |

sloth 的常驻内存仍普遍小于 CPython。`object_methods` 不再有绑定闭包线性增长
问题；直接栈访问也没有引入额外的堆分配。

## 仍然存在的主要缺口

1. **字符串（P1）**：`StringPool::creat_istring` 仍是线性扫描 + 全池清理；
   `StringIter` 仍用 `chars().skip(i)`。`string_ops` 因此几乎没有从 P0 中受益。
2. **集合/字典**：`GetCollection`/`SetCollection` 已是专用 ABI，但内部仍是
   `HashMap<IString, Value>` 查找和 `Value` 克隆，没有 inline cache。
3. **函数调用**：`Call`/`Return` 已去除通用 `sloth_rt_exec`，但 `CallFrame`
   创建、`va_args` 和 upvalue 清理仍是完整 runtime 逻辑。
4. **顶层全局变量**：专用 ABI 消除了 opcode 重建，但仍是哈希查找；P1 建议的
   slot 索引/inline cache 尚未实现。

## 测量局限

- 数据来自单线程、WSL2、无 PGO 的 release 构建；绝对时间会随机器和负载变化，
  但同一台机器上的相对关系稳定。
- 墙钟时间包含进程启动；`string_ops` 的 Python 时间较小，启动占比相对明显，
  报告同时给出了指令数作为交叉验证。
- sloth 的数字都是 f64；Python 版本使用惯用整数。`object_methods` 特意选择
  300,000 次迭代，使累加值落在 2^53 内，两者输出一致。
- `object_methods` 的旧 AOT 基线包含绑定闭包分配开销，当前结果同时受益于本轮
  之前的“方法调用快路径”提交。
- AOT 结果依赖 release runtime；如果链接 debug runtime，所有 AOT 数字都会
  严重失真（见开头的工具链注意）。
