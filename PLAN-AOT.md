# Sloth AOT / 模板解释器后端计划

## 目标

为 sloth-lang 新增一个 AOT（模板解释器）后端：

```text
source.slt
  → scanner / parser
  → Chunk 树（变长字节码 + 常量 + 元数据）
  → LLVM IR codegen
  → llc-21 -filetype=obj
  → cc + libsloth_runtime.a
  → 目标平台可执行文件
```

设计选择：

- **混合内联**：数值运算、比较、分支、常量、真值判断等热路径直接生成 LLVM IR；
  堆分配、字符串池、数组/字典/类、GC、native 调用、帧切换走小型 runtime ABI。
- **CallFrame + dispatcher**：函数调用和 fiber 仍使用 VM 的 `CallFrame` 与中央
  dispatcher，不改为 native 递归调用。这样闭包、upvalue、异常、fiber 语义可以复用
  现有实现。
- **静态链接**：生成的可执行文件静态链接 `libsloth_runtime.a`。
- **静态 import 预编译**：编译期扫描字符串字面量 `import`，递归编译模块并嵌入镜像。
- **不含图形**：AOT runtime 不链接 raylib；`draw` 模块在 AOT 模式下明确报错。

## 工程结构

```text
Cargo.toml                 # workspace + raylib optional feature
runtime/
  Cargo.toml               # sloth-runtime, staticlib
  src/lib.rs               # C ABI
src/
  backend/
    mod.rs
    llvm.rs                # Chunk → LLVM IR
    image.rs               # Chunk 树序列化/反序列化
    modules.rs             # 静态 import 扫描与递归编译
    driver.rs              # llc / cc 驱动
  main.rs                  # run / build 子命令
  vm.rs                    # step/exec、frame_epoch、AOT 支持
  value.rs                 # bits / retain / release
  lib.rs                   # 模块公开、build API
```

## 程序镜像

`Chunk` 增加 `native_id: usize` 字段。镜像格式：

```text
magic "SLTI" + version
root_count
每个 root：
  path（主程序为空，模块为 import 路径）
  chunk 子树
chunk：
  native_id
  file
  parameter_num / num_locals / is_va
  upvalues（kind + index + name）
  constants（Number / String）
  bytecodes
  lines
  child chunks
```

- 所有 chunk 按 DFS 顺序分配唯一 `native_id`。
- 镜像作为 LLVM 全局常量嵌入：

```llvm
@sloth_image = private constant [N x i8] c"..."
```

- runtime 启动时反序列化为 `Chunk` 树和 `StringPool`。

## LLVM IR

每个 chunk 生成一个 native 函数：

```llvm
define i32 @sloth_chunk_0(ptr %vm) {
entry:
  %pc = call i64 @sloth_rt_pc(ptr %vm)
  switch i64 %pc, label %bad [
    i64 0, label %b0
    i64 5, label %b5
  ]
b0:
  call void @sloth_rt_set_pc(ptr %vm, i64 0)
  ; 指令模板
  br label %b5
}
```

返回码：

- `0`：继续当前 chunk
- `1`：frame/fiber 变化，回到 dispatcher
- `2`：程序结束
- `-1`：错误

生成的 `main` 负责初始化 runtime，并根据当前 chunk 调用对应的
`sloth_chunk_N`。

内联模板：

- `PushNil`、`LoadTrue`、`LoadFalse`
- `Load(Number)`
- `Jump` / `JumpIfNot` / `JumpIfTrue`
- 数值 `Add/Sub/Mul/Negate`
- 数值比较 `Gt/Lt/Ge/Le/Eq/Ne`
- `Not`
- `GetLocal` / `SetLocal`
- `Pop`、`Swap2`

其余指令首版通过 `sloth_rt_exec` 兜底（内部复用 `Vm::exec`），后续可拆成专用 ABI。

runtime ABI：

- 字符串拼接/比较/迭代
- 数组/字典创建、索引、赋值
- 类、实例、方法查找、运算符重载
- `Iterator` / `Next`
- `LoadChunk` / 闭包
- upvalue / global / this
- `Call` / `Return`
- `InitArray` / `InitDict` / GC
- native 模块函数
- 错误处理

## 运行时 ABI

首版实际 ABI：

```text
sloth_rt_new(image: ptr, len: i64) -> ptr
sloth_rt_free(vm: ptr)
sloth_rt_take_error(vm: ptr) -> ptr
sloth_rt_free_string(s: ptr)
sloth_rt_report_error(vm)
sloth_rt_flush()
sloth_rt_bad_pc(vm)

sloth_rt_current_chunk(vm) -> i64
sloth_rt_pc(vm) -> i64
sloth_rt_set_pc(vm, pc: i64)
sloth_rt_frame_epoch(vm) -> u64
sloth_rt_finished(vm) -> i32

sloth_rt_exec(vm, op: i32, operand: i64) -> i32

sloth_rt_push(vm, bits: u64)
sloth_rt_push_number(vm, bits: u64)
sloth_rt_pop(vm) -> u64
sloth_rt_pop_discard(vm)
sloth_rt_peek(vm, depth: i64) -> u64
sloth_rt_peek_raw(vm, depth: i64) -> u64
sloth_rt_top_truthy(vm) -> i32
sloth_rt_local_get(vm, idx: i64) -> u64
sloth_rt_local_set(vm, idx: i64, bits: u64)
sloth_rt_swap2(vm)
sloth_rt_push_const(vm, idx: i64)
sloth_rt_retain(bits: u64)
sloth_rt_release(bits: u64)
```

规划中的专用慢路径 ABI（尚未全部拆分）：

```text
sloth_rt_binary_slow / unary_slow
sloth_rt_get_collection / set_collection
sloth_rt_init_array / init_dict
sloth_rt_make_range / iterator / next
sloth_rt_class_is / init_class / add_method / class_extend / get_super_method
sloth_rt_load_chunk
sloth_rt_get_global / set_global
sloth_rt_get_upvalue / set_upvalue
sloth_rt_get_this / unpack_va
sloth_rt_call / return
```

## `Vm` 重构

- 将 `Vm::run` 拆为 `step()` + `exec(instr)`。
- 增加 `Step` 枚举。
- 增加 `frame_epoch`，每次 frame push/pop 或 fiber 切换时递增。
- 增加 `current_chunk_id()`、`current_pc()`、`set_pc()`、`take_error()`。
- 增加 `precompiled_modules` 和 `load_precompiled_module()`。
- 保留解释器 `run()` 原有行为。

## 静态 import

- 编译期扫描 token：

```text
Symbol("import") LParen String(path) RParen
```

- 以 `cwd.join(path)` 解析，递归编译模块，检测循环。
- 每个模块一个 root chunk，记录原始 path 和 canonical path。
- 非字面量 import 在 AOT 模式下无法预编译，运行期加载时会明确报错。
- 运行期 `sloth_load_module` 优先查预编译模块表；找不到则明确报错。

## CLI

```bash
sloth script.slt                 # 旧用法，解释执行
sloth run script.slt --debug
sloth build script.slt -o out \
  [--emit llvm|obj|exe] \
  [--target TRIPLE] \
  [--llc PATH] [--cc PATH] [--opt-level N] \
  [--runtime-lib PATH] [--keep-temps]
```

驱动流程：

```bash
llc-21 -O2 -relocation-model=pic -filetype=obj -o out.o out.ll
cc out.o libsloth_runtime.a -o out -lm -lpthread -ldl
```

## 实施阶段

### M0：工具链与镜像骨架

- 新增 `runtime/` crate、`raylib` optional、`draw` cfg。
- 实现镜像序列化/反序列化。
- 最小 IR：嵌入镜像，`main` 调用临时 ABI 运行现有 VM。
- 实现 `sloth build`，用 hello.slt 验证静态链接可执行文件。

### M1：Vm 重构 + dispatcher + 正确性

- `Vm::step/exec`、`Step`、`frame_epoch`、错误存储。
- `Chunk.native_id`、`load_module` 拆分。
- per-chunk IR + dispatcher + ABI 原语。
- 核心语言测试。

### M2：内联模板

- 内联数值运算、比较、真值、常量、跳转、`Pop`/`Swap2`。
- `retain_bits/release_bits` 与所有权规则。
- GC/字符串压力测试。

### M3：静态 import

- token 扫描、递归编译、path → root chunk。
- 运行期预编译模块加载。
- `func_tool.slt` 测试。

### M4：完善

- 错误行号、`--target`、`--opt-level`、`--keep-temps`。
- README、性能对比、可选优化。

## 测试

- 镜像 round-trip。
- IR 能被 `llc-21` 接受。
- 解释执行与 AOT 输出对比：hello、算术、字符串、数组、类、闭包、fiber、
  variadic、import、错误路径。
- 没有 `llc-21` 时跳过 AOT 测试。

## 风险

- `Vm::run` 拆分回归：机械搬移，先保持解释器行为，跑现有测试。
- FFI panic / 引用计数 / GC：`catch_unwind`、所有权规则、压力测试。
- 静态链接：M0 先验证，必要时回退动态 `.so`。
- fiber：保持 CallFrame + dispatcher，专项测试。
- import 路径/循环：canonicalize + visited set。
- raylib：optional dependency，AOT runtime 不启用。

## 验证

非 GUI 测试：

```bash
cargo build -p sloth-runtime --target-dir target/sloth-runtime
cargo test --lib
cargo test --test aot_backend
cargo test --test gc
```

`tests/raylib_test.rs`、`game_of_life.slt`、`snake.slt`、`brainfk`（读取 stdin）
需要人工交互，已标记为 `#[ignore]`，不会在自动测试中运行。

## 实施进度

已完成：

- `Chunk.native_id`、`Vm::step/exec`、`Step`、`frame_epoch`、AOT 错误存储。
- `Value` 的 bits / retain / release 接口。
- `backend/image.rs`：Chunk 树序列化/反序列化（含 round-trip 测试）。
- `backend/modules.rs`：静态字面量 `import` 扫描、递归编译、DFS native_id。
- `backend/llvm.rs`：每个 chunk 一个 native 函数、dispatcher、逐指令基本块；
  已内联数值运算/比较/局部变量/常量/跳转/Pop/Swap2/Not/Negate。
- `backend/driver.rs`：`llc-21` + `cc` 静态链接驱动。
- `runtime/`：`libsloth_runtime.a`、C ABI、image 启动、VM/native 模块注册。
- CLI：`sloth run` / `sloth build`，保留 `sloth script.slt` 旧用法。
- `raylib` optional feature，AOT runtime 不包含图形依赖。
- 集成测试：IR 生成、`llc` 接受 IR、端到端 exe 运行（非 GUI）。

后续可继续：

- 将 `Call`/`Return`/集合访问等更多操作从通用 `sloth_rt_exec` 拆成专用 ABI。
- 内联更多字符串/数组快路径。
- 更友好的 AOT 错误行号与缺失 `draw` 模块诊断。
- release runtime 自动构建与交叉目标支持。
- 非 GUI 测试矩阵扩展到更多示例。

