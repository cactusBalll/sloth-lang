# sloth-lang

出于学习目的编写的基于字节码的解释器。
sloth definitely runs slower than python ()
1. 2022起，个人解释器项目
2. 2024, BUAA ADSL2024 课程设计
3. 2026, AOT支持等优化

## 字节码格式

`Chunk::bytecodes` 保存的是可变长字节码，而不再是 `Vec<Instr>`：

- 每条指令以 1 字节 `OpCode` 开头；
- 索引/数量操作数使用无符号 LEB128（小索引只占 1 字节）；
- 跳转操作数使用固定 4 字节有符号小端偏移，偏移量为相对于跳转指令起始位置的**字节**偏移；
- `Chunk::lines` 为每个字节记录源码行号，VM 的 `pc` 直接作为字节偏移使用。

编译器在解析/回填跳转时仍然使用 `Instr` 中间表示，关闭函数作用域时由
`bytecode::encode_instrs` 汇编成最终字节码。

## 值表示

默认启用 `nan-boxing` feature，`Value` 变为单个 64 位字：

- 非 NaN 的 `f64` 直接存储；
- 立即数（nil/bool）与堆指针使用 quiet-NaN 标记；
- `Range`、迭代器和 NaN 数值存放在引用计数的 box 中。

关闭 `nan-boxing` 后仍可使用原来的 tagged-union `Value` 枚举。两种表示都通过
`Value::kind() -> ValueKind` 暴露相同的视图，VM 与 native 模块只依赖这个视图。

```bash
cargo test --features nan-boxing    # NaN-boxing（默认）
cargo test --no-default-features    # tagged-union
```

## AOT / 模板解释器后端

新增 `backend` 和 `runtime/`：
把 `Chunk` 树编译为 LLVM IR，再用 `llc` 生成目标平台对象文件，最后静态链接
`libsloth_runtime.a` 得到独立可执行文件。

生成代码中：

- 每个 `Chunk` 对应一个 native 函数；
- 每条字节码对应一个基本块/模板；
- 数值运算、比较、常量、局部变量、跳转、真值判断等热路径直接内联为 LLVM IR；
- 生成代码直接读写 runtime 暴露的值栈（`sloth_rt_stack_data/len/cap`），
  不再为每条 `push`/`pop`/`GetLocal` 调用 ABI；
- `pc` 只在离开当前 chunk 或进入需要它的慢路径时同步，块内跳转直接 `br`；
- 字符串池、数组/字典、类、GC、native 调用、CallFrame/fiber 切换走专用
  runtime ABI；没有通用 `sloth_rt_exec` 兜底；
- 函数调用仍使用 VM 的 `CallFrame` 和中央 dispatcher，因此 fiber/闭包语义与解释器一致。

### 构建

```bash
# 可选：预构建 AOT 运行时静态库（sloth build 也会在缺失时自动构建）
cargo build -p sloth-runtime --target-dir target/sloth-runtime

# 编译为可执行文件
cargo run -- build sloth/sloth_examples/hello.slt -o hello
./hello

# 只生成 LLVM IR / 对象文件
cargo run -- build sloth/sloth_examples/hello.slt --emit llvm -o hello.ll
cargo run -- build sloth/sloth_examples/hello.slt --emit obj -o hello.o
```

`build` 选项：

```text
-o, --output <PATH>        输出路径
--emit <llvm|obj|exe>      输出类型，默认 exe
--llc <PATH>               llc 路径，默认 llc-21
--cc <PATH>                链接器，默认 cc
--opt-level <0-3>          llc 优化级别，默认 2
--target <TRIPLE>          LLVM target triple
--runtime-lib <PATH>       libsloth_runtime.a 路径
--keep-temps               保留中间 .ll / .o
```

### 限制

- AOT 后端要求 `nan-boxing`（默认开启）。
- 静态 `import("literal")` 会在编译期递归预编译；动态 import 路径不支持。
- AOT runtime 不链接 raylib，`draw` 模块不可用；解释器模式仍支持。
- 首版以 host target 为主；交叉目标需要自备对应 `llc` 和链接器。

## 性能基准

`benches/programs/` 下有 6 个典型 sloth 程序（递归、数值循环、数组筛法、
对象方法、字典查找、字符串处理），每个程序都有对应的 CPython 版本。
`benches/compare.py` 会分别测量 switch 解释器、AOT 模板后端和 CPython 的
墙钟时间；`cargo bench --bench interpreter_bench` 使用 criterion 0.8.2
对 switch 解释器做进程内基准。

```bash
python3 benches/compare.py --python python3.12
cargo bench --bench interpreter_bench
```

当前结果、性能缺口分析和优化建议见 [BENCHMARKS.md](BENCHMARKS.md)。

> 注意：AOT 对比务必链接 release 版 `libsloth_runtime.a`。当前
> `find_runtime_lib` 会优先选择 debug 版；误链 debug runtime 会让 AOT
> 程序慢一个数量级以上。
