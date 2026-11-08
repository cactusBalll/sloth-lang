# Sloth 性能基准：switch 解释器 vs AOT 模板后端 vs CPython

本文评估当前仓库中的两种执行方式：

- **switch 解释器**：`Vm::step()` 从变长字节码解码一条 `Instr`，再进入
  `Vm::exec()` 的 `match instr` 分发。
- **AOT 模板后端**：`sloth build` 把每个 `Chunk` 的每条字节码编译成一个 LLVM
  基本块（README/提交中称为“模板解释器”），再链接 `libsloth_runtime.a`。
  它不是在同一个进程里用函数指针/computed-goto 做分发的传统模板解释器，而是
  “字节码 → 原生基本块”的 AOT 实现。

对比基线是 **CPython 3.12.13**。

## 结论摘要

1. **AOT 模板后端相对 switch 解释器的提升有限**：6 个程序为 **1.07x–2.33x**，
   几何平均 **1.67x**，总墙钟时间从 4.75 s 降到 2.72 s（**1.74x**）。
   只有算术、比较、分支、局部变量等被真正内联的路径收益明显；函数调用、全局
   变量、数组/字典、字符串、方法调用仍然走通用 runtime/VM 路径。
2. **相对 CPython 3.12**：
   - switch 解释器慢 **6.8x–10.1x**（几何平均 **7.8x**）；
   - AOT 模板后端慢 **3.4x–7.4x**（几何平均 **4.7x**）。
3. **性能缺口的主因不是 NaN-boxing 或数值运算本身**，而是：
   - switch 解释器每条指令解码两次（`step()` 一次，`pc_add()` 又一次）；
   - AOT 每个字节码仍然调用大量 `sloth_rt_*` C ABI，且每条指令都写一次
     `sloth_rt_set_pc`；通用慢路径 `sloth_rt_exec` 会重新构造 `Instr` 并回到
     `Vm::exec`；
   - 全局变量用 `HashMap<IString, Value>` 每次查找，无内联缓存；
   - 每次方法访问都克隆并 `Box` 一个绑定闭包；GC 只在数组/字典创建时触发；
   - 字符串驻留池是线性扫描，字符串迭代用 `chars().skip(i)`，导致 O(n²)。
4. **微基准**显示 AOT 的收益与被内联指令的比例强相关：
   - 纯局部变量空循环：AOT **4.18x**；
   - 局部变量算术循环：**3.57x**；
   - 顶层全局变量算术循环：只有 **1.96x**；
   - 递归 fib：**2.33x**；对象方法：**1.57x**；字符串：**1.07x**。

## 环境与测量方法

| 项目 | 值 |
|---|---|
| CPU | AMD Ryzen 7 H 260 (16 vCPU, WSL2) |
| OS | Linux 6.18.54.1-microsoft-standard-WSL2 x86-64 |
| Rust | rustc 1.99.0-nightly |
| LLVM | llc-21 (LLVM 21.1.8) |
| CPython | 3.12.13（主对比）；附录给出 3.10.12 |
| Criterion | 0.8.2 |
| 编译 | `cargo build --release`；AOT 使用 `llc -O2` + release runtime |
| 计时 | 每个配置先 warm-up 1 次，再测 9 次，取中位数；`taskset -c 2` 绑核 |
| 输出 | 子进程 stdout 丢弃，避免 I/O 干扰 |

进程启动开销（中位数）：switch 约 **2.1 ms**，AOT 约 **1.8 ms**，CPython
**11.6 ms**。除 `string_ops`（CPython 仅 48 ms）外，启动开销占比很小；文中
的比值均为包含启动的墙钟时间。

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
> runtime，AOT 程序会慢 5x–260x（实测 `string_ops` 从 0.34 s 变成 93.8 s）。
> 后续应让 runtime 选择跟随当前 cargo profile。

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

`object_methods` 的 300,000 次迭代保证累加值小于 2^53，因此 sloth 的 f64
结果与 Python 整数结果完全一致（两者都输出
`44999850000 8999999999900000`）。

## 主结果

单位 ms；`AOT vs switch` 越大越好，`vs CPython` 越小越好。

| 程序 | switch | AOT 模板 | CPython 3.12 | AOT vs switch | switch vs CPython | AOT vs CPython |
|---|---:|---:|---:|---:|---:|---:|
| `fib_rec` | 817.5 | 350.3 | 81.4 | **2.33x** | 10.05x slower | 4.31x slower |
| `loop_arith` | 1287.2 | 638.9 | 189.2 | **2.01x** | 6.80x slower | 3.38x slower |
| `sieve` | 986.3 | 587.1 | 135.2 | **1.68x** | 7.30x slower | 4.34x slower |
| `object_methods` | 474.4 | 302.8 | 59.6 | **1.57x** | 7.96x slower | 5.08x slower |
| `dict_lookup` | 802.0 | 486.6 | 110.6 | **1.65x** | 7.25x slower | 4.40x slower |
| `string_ops` | 382.8 | 358.8 | 48.4 | **1.07x** | 7.90x slower | 7.41x slower |
| **合计/几何平均** | **4750** | **2724** | **624** | **1.67x geo / 1.74x total** | **7.8x geo** | **4.7x geo** |

Criterion 0.8.2 的进程内 switch 结果与上表 switch 列一致（fib ≈ 797 ms，
loop ≈ 1.30 s，sieve ≈ 990 ms，object ≈ 525 ms，dict ≈ 804 ms，string ≈ 306 ms），
说明结果不是脚本计时误差造成的。

### 逐项解读

- **`fib_rec`（2.33x）**：AOT 内联了 `n < 2`、`n - 1`、加法、跳转，但
  `Call`/`Return`、栈操作、局部变量访问仍通过 runtime ABI 和 `Vm::exec`。
  switch 版还要对每条指令做两次解码。最终 AOT 仍比 CPython 慢 4.3x。
- **`loop_arith`（2.01x）**：顶层 `i`/`acc` 是全局变量，AOT 对
  `GetGlobal`/`SetGlobal` 没有内联，每次都要走 `sloth_rt_exec`；此外 `%`
  （`Mod`）也没有内联。AOT 的收益主要来自加法、乘法和分支。
- **`sieve`（1.68x）**：数组的 `GetCollection`/`SetCollection` 全部走
  `sloth_rt_exec`，每次访问都有边界检查和 `Value` 克隆；数组创建还会触发
  `run_gc`。AOT 只内联了循环变量和比较/跳转。
- **`object_methods`（1.57x）**：每次 `c.add(i)` / `c.norm()` 都要在
  `GetCollection` 慢路径中克隆 `Closure`、设置 `this_ref`、`Box` 一个绑定
  闭包并加入 `self.objects`。AOT 完全没有改变这条路径，所以只有 1.57x。
- **`dict_lookup`（1.65x）**：字典查找本身是 `HashMap<IString, Value>`，
  字符串哈希和键查找在 runtime 中完成；AOT 只省掉了解释器分发。
- **`string_ops`（1.07x）**：AOT 对字符串 `Add` 和迭代器 `Next` 完全没有
  内联；这两条路径正好是字符串程序的主要成本。详见下文。

## 指令数与 IPC

`perf stat -e cycles,instructions` 统计的是每次运行的总指令数（含启动），
单位 10 亿条（B）：

| 程序 | switch insn | AOT insn | CPython insn | switch/py | AOT/py |
|---|---:|---:|---:|---:|---:|
| `fib_rec` | 11.88 | 6.02 | 1.36 | 8.8x | 4.4x |
| `loop_arith` | 17.69 | 10.48 | 3.59 | 4.9x | 2.9x |
| `sieve` | 14.38 | 9.06 | 2.45 | 5.9x | 3.7x |
| `object_methods` | 5.99 | 4.13 | 0.88 | 6.8x | 4.7x |
| `dict_lookup` | 11.88 | 7.89 | 2.13 | 5.6x | 3.7x |
| `string_ops` | 5.43 | 5.08 | 0.10 | 54x | 51x |

典型 IPC：

| 程序 | switch | AOT | CPython |
|---|---:|---:|---:|
| `fib_rec` | 3.16 | 3.74 | 3.95 |
| `loop_arith` | 2.93 | 3.61 | 4.48 |
| `sieve` | 3.25 | 3.54 | 4.33 |
| `object_methods` | 3.10 | 3.45 | 3.82 |

结论：sloth 与 CPython 的差距主要是**执行了更多指令**（2.9x–8.8x），
而不是单纯 CPU 流水线效率差；CPython 3.12 的专门化字节码和 inline cache
把每条动态操作压到了更少的指令。

峰值 RSS（`/usr/bin/time -f %M`）：

| 程序 | switch | AOT | CPython 3.12 |
|---|---:|---:|---:|
| `fib_rec` | 3.4 MB | 2.5 MB | 10.2 MB |
| `loop_arith` | 3.2 MB | 2.4 MB | 10.1 MB |
| `sieve` | 7.4 MB | 6.2 MB | 14.0 MB |
| `object_methods` | **49.2 MB** | **48.0 MB** | 10.1 MB |
| `dict_lookup` | 3.5 MB | 2.4 MB | 10.2 MB |
| `string_ops` | 4.0 MB | 3.2 MB | 10.4 MB |

sloth 的常驻内存通常比 CPython 小（NaN-boxed 单字 `Value`、按需加载模块），
但 `object_methods` 是明显异常：AOT 也没有改善，原因见下文“方法调用”。

## 性能缺口定位

### 1. switch 解释器：每条指令解码两次

`Vm::step()` 先 `decode()` 当前指令；随后 `exec()` 的 `pc_add()` 调用
`CallFrame::nxt()`，`nxt()` 再次 `decode()` **同一条指令**只为得到长度。
因此每条指令至少两次 LEB128 解码/边界检查。`perf` 中
`decode_instr`/`read_operand` 约占总指令的 20%。

一个 2,000,000 次的纯局部变量空循环：

| 配置 | 时间 | 每迭代指令数 | IPC |
|---|---:|---:|---:|
| switch | 412.3 ms | **3323** | 3.26 |
| AOT | 98.6 ms | **894** | 3.96 |
| CPython 3.12 | 43.4 ms | **391** | 4.57 |

即使只做 `while (i < N) { i = i + 1; }`，switch 版也要 3323 条指令/迭代，
是 CPython 的 8.5 倍。AOT 降到 894 条，仍是 CPython 的 2.3 倍。

### 2. AOT 模板后端：每个字节码仍是一次 extern 调用

以最简单的局部循环为例，生成 IR 的循环体近似为：

```llvm
bb_4:
  call void @sloth_rt_set_pc(ptr %vm, i64 4)
  %lv = call i64 @sloth_rt_local_get(ptr %vm, i64 0)
  call void @sloth_rt_push(ptr %vm, i64 %lv)
bb_6:
  call void @sloth_rt_set_pc(ptr %vm, i64 6)
  call void @sloth_rt_push(ptr %vm, i64 <const>)
bb_8:
  call void @sloth_rt_set_pc(ptr %vm, i64 8)
  %b = call i64 @sloth_rt_peek_raw(ptr %vm, i64 0)
  %a = call i64 @sloth_rt_peek_raw(ptr %vm, i64 1)
  ...
  %bv = call i64 @sloth_rt_pop(ptr %vm)
  %av = call i64 @sloth_rt_pop(ptr %vm)
  call void @sloth_rt_push(ptr %vm, i64 %res)
```

简单循环每迭代仍有约 **30 次** `sloth_rt_*` 调用，其中每字节码一次
`sloth_rt_set_pc`。生成的原生代码本身只占循环时间的一小部分。

`perf` 采样（近似）显示 AOT 程序的 cycles 分布：

| 路径 | `loop_arith` | `fib_rec` |
|---|---:|---:|
| `sloth_rt_exec` + opcode 重建/解码（通用慢路径） | 26.1% | 29.9% |
| `Value` clone/drop/kind | 22.0% | 19.3% |
| `Vm::exec` 本体 | 10.3% | 18.2% |
| `IString` 引用计数 | 9.0% | 2.4% |
| 生成的 `sloth_chunk_*` 代码 | **3.9%** | **5.6%** |
| `sloth_rt_local_get/push/pop/peek` | ~12% | ~14% |
| `sloth_rt_set_pc` | 2.5% | 3.2% |

也就是说，AOT 程序的大部分时间仍花在 runtime/VM 的通用路径上，而不是
模板生成的代码里。通用 `sloth_rt_exec(vm, opcode, operand)` 会
`OpCode::from_u8` → 构造 `Instr` → `Vm::exec(instr)` 再 `match` 一次；
凡是没被专门内联的操作（全局变量、数组、字典、字符串、方法、`Div/Mod`、
`Call/Return` 等）都要吃这份成本。

AOT 收益与被内联比例的关系非常清楚：

| 微基准（2,000,000 次） | switch | AOT | AOT 提升 |
|---|---:|---:|---:|
| 纯局部变量空循环 | 412.3 ms | 98.6 ms | **4.18x** |
| 局部变量算术循环 | 919.6 ms | 257.8 ms | **3.57x** |
| 顶层全局变量算术循环 | 1226.0 ms | 626.1 ms | **1.96x** |

微基准源码位于 `benches/micro/`（`empty_loop`、`arith_local`），全局变量版本
直接使用 `benches/programs/loop_arith.slt`。构建 AOT 二进制时同样必须显式
链接 release runtime，例如：

```bash
target/release/sloth-lang-core build benches/micro/empty_loop.slt \
  -o target/bench-bin/empty_loop \
  --runtime-lib target/sloth-runtime/release/libsloth_runtime.a
```

### 3. 全局变量：无内联缓存

`GetGlobal`/`SetGlobal` 每次都用常量里的 `IString` 去
`self.global.last().unwrap()` 的 `HashMap` 查找；`SetGlobal` 还会把键插入
或替换。顶层代码变量都是全局变量，所以 `loop_arith` 每轮有大量哈希查找。
对比同算法的函数局部变量版本：

| 配置 | 顶层全局 | 函数局部 | 局部快多少 |
|---|---:|---:|---:|
| switch | 1226.0 ms | 919.6 ms | 25% |
| AOT | 626.1 ms | 257.8 ms | **2.4x** |
| CPython 3.12 | 184.1 ms | 128.7 ms | 30% |

AOT 的 `GetLocal/SetLocal` 至少有专门 ABI（`local_get` + `push` 等），而全局
变量必须走 `sloth_rt_exec`，所以全局/局部的差距被放大到 2.4x。CPython 3.12
对 `LOAD_GLOBAL` 有专门化 + inline cache，差距只有 30%。

switch 版顶层循环的 `perf` 采样中，`IString` 引用计数/析构路径也占了约 27%
（包含内联和采样归因，数量级可参考）。原因是 `GetGlobal`/`SetGlobal` 每次
都要克隆常量里的 `IString` 键，查找后再丢弃；这进一步放大了全局变量的成本。

### 4. 函数调用与方法调用

- `Call` 每次创建 `CallFrame`（含 `va_args: Vec`），把帧压入 `Vec`，并递增
  `frame_epoch`；`Return` 弹出帧、遍历 `self.upvalues`、为剩余 upvalue 新建
  `Vec`、清理局部变量、处理 fiber 切换。
- 方法访问 `GetCollection` 在实例字段/方法表中查找后，会
  `Closure::clone()`、设置 `this_ref`、`Box::new(binded_closure)`，再塞进
  `self.objects`。**每次方法调用都分配一个绑定闭包**。
- `run_gc()` 只在 `InitArray`/`InitDict` 时调用（`src/vm.rs:923/929`）。
  `object_methods` 循环里没有数组/字典创建，因此 300,000 轮 × 2 次方法访问
  产生的约 600,000 个绑定闭包不会被回收；实测 RSS 约 **49 MB**，而 CPython
  约 **10 MB**。
- `fib_rec` 的 AOT 采样中，`CallFrame`/调用分发约 5%，通用慢路径 + `Vm::exec`
  约 48%，生成代码只有 5.6%。这说明 AOT 没有改变“调用仍回中央解释器”的结构。

### 5. 字符串：驻留池和迭代器的 O(n²)

`src/interned_string.rs::creat_istring` 的实现是：

1. 线性扫描 `StringPool::data`，逐项比较完整字符串；
2. 找到相同内容才复用，否则新建；
3. 每次新建后还遍历整个池，删除 `ref_count <= 0` 的项并 `retain`。

因此每创建一个新字符串都是 O(池大小) + O(池大小) 清理。字符串程序还要做
10,000 次 `s = s + parts`，天然是超线性。

`StringIter` 的 `Next`（`src/vm.rs` 的 `ValueKind::StringIter` 分支）用：

```rust
let c = s.get_inner().chars().skip(i).next();
```

每次取第 i 个字符都要从头 `skip`，完整扫描是 O(n²)，并且每个字符都要
`format!("{c}")` + `creat_istring` 生成一个 1 字符字符串。

`string_ops` 的指令数：switch 5.43B、AOT 5.08B、CPython **0.10B**——相差
约 50 倍。AOT 对字符串 `Add`/`Next` 没有内联，所以只快 1.07x。

> 同一程序在 CPython 3.10 上实测约 16 ms，而 3.12 约 48 ms，说明该 workload
> 受 CPython 字符串拼接实现细节影响。因此字符串一项的“与 Python 比值”对
> Python 版本敏感；但 **AOT vs switch 的 1.07x** 不受影响。

### 6. 数组/字典/GC

- `InitArray`/`InitDict` 和 `GetCollection`/`SetCollection` 都在 `Vm::exec`
  的匹配里，AOT 没有内联；每次访问都有 `ValueKind` 匹配、边界检查、克隆。
- `run_gc` 的触发条件是 `self.objects.len() >= 128` 且当前指令是
  `InitArray`/`InitDict`。这意味着大量临时闭包/实例在两次数组创建之间不会
  被回收，既增加内存，也可能拖慢后续分配。
- 字符串不是 GC 对象，驻留池的清理是另一套 refcount 逻辑，见上。

## 优化建议（按预期收益排序）

### P0：AOT 代码生成

1. **不要每条指令都调用 `sloth_rt_set_pc`**。`pc` 只在离开当前 chunk
   （Call/Return/Except/fiber/错误）时才需要同步；块内跳转可以直接 `br`。
2. **把栈指针/frame base 暴露给生成代码**，在 IR 里直接做 push/pop/local
   load/store，而不是每条指令调用 `sloth_rt_local_get`、`sloth_rt_push`、
   `sloth_rt_pop`、`sloth_rt_peek_raw`。
3. **把 `Div`/`Mod`/`And`/`Or`/`GetGlobal`/`SetGlobal`/集合访问/Call/Return
   的常用快路径也内联**；当前 `%` 和全局变量直接掉进 `sloth_rt_exec`。
4. **拆掉通用 `sloth_rt_exec` 兜底**，为每类慢路径提供专用 ABI，避免
   `OpCode::from_u8` + 构造 `Instr` + `Vm::exec` 二次匹配。
5. 考虑对 runtime 做 LTO/`llvm-link`，或把最热的 ABI 用 LLVM IR 生成，让
   `llc` 能跨边界内联。

仅第 1、2 项就可能把空循环的 894 指令/迭代大幅降低（目前 CPython 是 391）。

### P0：switch 解释器

1. **消除双重解码**：`step()` 返回指令长度，`pc_add()` 直接加长度；或把
   字节码预解码成固定宽度 `Vec<Instr>` / `(Instr, len)`。
2. 用 computed-goto/直接线程化，或至少把 `step` 和 `exec` 合并成一个循环，
   避免每条指令重新取 `call_frame`、`stack`、`closure`。
3. 对 `GetLocal + Load + Add + SetLocal`、`GetLocal + Lt + JumpIfNot` 等
   常见序列做指令融合/超级指令。

### P1：全局变量、方法、调用

1. 编译期把全局变量解析成 slot 索引，运行期用 `Vec<Value>` 或固定表访问；
   或给 `GetGlobal/SetGlobal` 加 inline cache。
2. 方法调用不要每次 `Box` 绑定闭包：`Call` 直接携带 receiver（`this`），
   在 `CallFrame` 中保存；方法查找加缓存。
3. 减少 `Call/Return` 的分配：复用 `CallFrame`、避免每次 `Return` 新建
   `Vec<*mut UpValueObject>`，无 upvalue 时走零成本快路径。
4. GC 改为按分配量触发，并优先回收短命绑定闭包；否则 `object_methods`
   这类程序的内存会随循环线性增长。

### P1：字符串

1. `StringPool` 改成 `HashMap<Box<str>, *mut Entry>`（或按哈希分桶），去掉
   线性扫描和每次插入的全池清理。
2. `StringIter` 保存字节偏移，使用 `char_indices`/`get(offset..)` 取下一个
   `char`，避免 `chars().skip(i)` 的 O(n²)。
3. 字符迭代时避免为每个字符创建驻留字符串；可以直接返回小字符串或字符码。
4. 字符串 `Add` 在 AOT 中至少走专用 ABI，避免通用 `sloth_rt_exec`。

### P2：其他

1. NaN-boxing 本身不是瓶颈；但 `Value::kind()` 对字符串会做 retain，热路径
   可以增加不增加引用计数的借用视图。
2. 数组/字典的索引路径可以内联边界检查和元素克隆。
3. 顶层程序可以编译成一个隐式函数，把顶层变量变成局部槽位。
4. 为 AOT 生成代码打开 `-O3` 并评估 `-flto`/PGO；当前默认 `-O2`。

## 测量局限

- 数据来自单线程、WSL2、无 PGO 的 release 构建；绝对时间会随机器和负载变化，
  但同一台机器上的相对关系稳定。
- 墙钟时间包含进程启动（Python 约 11.6 ms，sloth 约 2 ms）；`string_ops` 的
  Python 时间较小，启动占比约 1/4，报告同时给出了指令数作为交叉验证。
- sloth 的数字都是 f64；Python 版本使用惯用整数。`object_methods` 特意选择
  300,000 次迭代，使累加值落在 2^53 内，两者输出一致。
- AOT 结果依赖 release runtime；如果链接 debug runtime，所有 AOT 数字都会
  严重失真（见开头的工具链注意）。
- `string_ops` 的 Python 侧结果对 CPython 版本敏感，原因见第 5 节；AOT vs
  switch 的比较不受此影响。

## 附录 A：CPython 3.10 对比

同一套程序在 CPython 3.10.12 上（Python 列为 7 次中位数；switch/AOT 列沿用
主表，因为二进制相同）：

| 程序 | switch | AOT | CPython 3.10 | AOT vs switch | AOT vs CPython 3.10 |
|---|---:|---:|---:|---:|---:|
| `fib_rec` | 817.5 | 350.3 | 129.2 | 2.33x | 2.71x slower |
| `loop_arith` | 1287.2 | 638.9 | 232.7 | 2.01x | 2.75x slower |
| `sieve` | 986.3 | 587.1 | 162.1 | 1.68x | 3.62x slower |
| `object_methods` | 474.4 | 302.8 | 85.6 | 1.57x | 3.54x slower |
| `dict_lookup` | 802.0 | 486.6 | 123.7 | 1.65x | 3.93x slower |
| `string_ops` | 382.8 | 358.8 | 16.1 | 1.07x | 22.29x slower |

3.12 在除字符串外的项目上都比 3.10 快；字符串的差异来自 CPython 自身的
实现变化，不代表 sloth 的字符串实现更好。

## 附录 B：AOT 的通用慢路径示例

`loop_arith` 的 AOT 采样（`perf record`，近似，取 >0.05% 的样本聚合）：

```text
26.1%  sloth_rt_exec + opcode 重建/解码
22.0%  Value clone/drop/kind
10.3%  Vm::exec 本体
 9.0%  IString 引用计数
 5.9%  sloth_rt_peek_raw
 3.9%  sloth_chunk_0（生成的模板代码）
 3.3%  hashing
 2.5%  sloth_rt_set_pc
 2.5%  decode_instr
 2.4%  sloth_rt_pop
 2.0%  sloth_rt_push
```

`fib_rec` 的 AOT 采样：

```text
29.9%  sloth_rt_exec + opcode 重建/解码
19.3%  Value clone/drop/kind
18.2%  Vm::exec 本体
 5.6%  sloth_chunk_1（生成的模板代码）
 5.0%  CallFrame / 调用分发
 4.0%  sloth_rt_local_get
 3.7%  sloth_rt_push
 3.2%  sloth_rt_set_pc
 2.4%  IString 引用计数
```

这两组数据直接说明了“模板解释器”的增益为什么只有 1.07x–2.33x：**模板代码
只占执行时间的一小部分，大部分时间仍在 runtime ABI、通用 `Vm::exec` 和
值/字符串操作上。**
