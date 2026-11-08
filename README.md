# sloth-lang

出于学习目的编写的基于字节码的解释器。
sloth definitely runs slower than python ()

BUAA ADSL2024 课程设计

## 字节码格式

`Chunk::bytecodes` 保存的是可变长字节码，而不再是 `Vec<Instr>`：

- 每条指令以 1 字节 `OpCode` 开头；
- 索引/数量操作数使用无符号 LEB128（小索引只占 1 字节）；
- 跳转操作数使用固定 4 字节有符号小端偏移，偏移量为相对于跳转指令起始位置的**字节**偏移；
- `Chunk::lines` 为每个字节记录源码行号，VM 的 `pc` 直接作为字节偏移使用。

编译器在解析/回填跳转时仍然使用 `Instr` 中间表示，关闭函数作用域时由
`bytecode::encode_instrs` 汇编成最终字节码。

## 值表示

默认仍使用原来的 tagged-union `Value` 枚举。启用 `nan-boxing` feature 后，
`Value` 变为单个 64 位字：

- 非 NaN 的 `f64` 直接存储；
- 立即数（nil/bool）与堆指针使用 quiet-NaN 标记；
- `Range`、迭代器和 NaN 数值存放在引用计数的 box 中。

两种表示都通过 `Value::kind() -> ValueKind` 暴露相同的视图，VM 与 native
模块只依赖这个视图。

```bash
cargo test                          # tagged-union
cargo test --features nan-boxing    # NaN-boxing
```
