use sloth_lang_core::run_string;

/// Allocate enough objects to force several collections while keeping
/// instances, closures and nested/cyclic arrays reachable.
#[test]
fn gc_keeps_reachable_objects() {
    let src = r#"
        class Node {
            func __init__(v) {
                this.v = v;
            }
            func get() {
                return this.v;
            }
        }

        var nodes = [];
        for (var i: 0..200) {
            nodes.push(Node(i));
        }

        func make_adder(n) {
            func add(x) {
                return n + x;
            }
            return add;
        }

        var adders = [];
        for (var i: 0..100) {
            adders.push(make_adder(i));
        }

        var cyclic = [1, 2];
        cyclic.push(cyclic);
        cyclic.push(nodes);

        for (var i: 0..500) {
            var garbage = [i, [i, [i, i]], [Node(i), make_adder(i)]];
        }

        var sum = 0;
        for (var n: nodes) {
            sum = sum + n.get();
        }

        var add_sum = 0;
        for (var a: adders) {
            add_sum = add_sum + a(5);
        }

        print(sum, add_sum, cyclic[0]);
    "#;
    let res = run_string(src, false);
    assert!(res.is_ok(), "{res:?}");
}

/// Values passed through a variadic call only live in `CallFrame::va_args`
/// and must be treated as GC roots.
#[test]
fn gc_keeps_variadic_arguments() {
    let src = r#"
        class Box {
            func __init__(v) {
                this.v = v;
            }
            func get() {
                return this.v;
            }
        }

        func collect(a, ...) {
            for (var i: 0..400) {
                var garbage = [i, [i, i]];
            }
            return va_arg();
        }

        var r = collect(0, [1, 2], [3, 4], Box(5));
        print(r[0][0], r[1][1], r[2].get());
    "#;
    let res = run_string(src, false);
    assert!(res.is_ok(), "{res:?}");
}

/// A receiver bound by the direct method-call fast path is not on the operand
/// stack, so `CallFrame::bound_this` must be treated as a GC root.
#[test]
fn gc_keeps_bound_this_receiver() {
    let src = r#"
        class Counter {
            func __init__(v) {
                this.v = v;
            }
            func value() {
                var i = 0;
                while (i < 400) {
                    var garbage = [i, [i, i]];
                    i = i + 1;
                }
                return this.v;
            }
        }

        var result = Counter(41).value();
        print(result);
        if (result != 41) {
            var bad = [][0];
        }
    "#;
    let res = run_string(src, false);
    assert!(res.is_ok(), "{res:?}");
}
