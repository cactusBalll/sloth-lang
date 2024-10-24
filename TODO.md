## tier-1 TODO

- parser 对 EOF 的处理 done
- 移除奇怪的assign（强行LL(1) done
- && || 短路运算 done
- 重构vm
- 移除不需要的内置类型 done
- 重构interned string done
- 消除warning
- fiber
- **module** done
- 重构chunk done
- Class-based OOP done
- OCaml style PIPE Operator `|>` done
- ~~list assginment~~
- **break/continue** done
- **range-based for & iterator protocol** done
- **operator overload** done
- **类型强制** done
- **重构Fiber** done
- `__array_push__([], 1) <-> [].push(1)` 内置类型点语法调用
- ~~`...`在栈上展开参数~~
## tier-2 TODO
- better error system
- debugger
- VSCode syntax highlight
```
class A:B {
    var a{

    }=(t){

    }
    var b{

    }
    var c=(t){
        
    }
    func foo() {
        print(a)
    }
    func +(b) {
        a += b
    }
    func -(b) {

    }
    func %(b) {}
    func 
    func *(b) {

    }
    func /(b) {

    }
    func __idxset__(idx,v){

    } 
    func __idxget__(idx){

    }
}
```
```
Command	Description
>	Move the pointer to the right
<	Move the pointer to the left
+	Increment the memory cell at the pointer
-	Decrement the memory cell at the pointer
.	Output the character signified by the cell at the pointer
,	Input a character and store it in the cell at the pointer
[	Jump past the matching ] if the cell at the pointer is 0
]	Jump back to the matching [ if the cell at the pointer is nonzero
```