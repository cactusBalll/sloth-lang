use sloth_lang_core::*;

#[test]
fn basic1() {
    let prog = r#"
        var cnt = 0;
        var arr = Array(0,0,0,0,0);
        while (cnt < 5){
            assign arr[cnt] = cnt;
            assign cnt = cnt + 1;
        }
        print(arr);
    "#;
    run_string_debug(prog).unwrap();
}
#[test]
fn basic2() {
    let prog = r#"
       func fib(n){
           if (n == 1 or n == 0) {
               return 1;
           } else {
               return fib(n - 1) + fib(n -2);
           }
       }
       print(fib(10));
       //233;
    "#;
    run_string_debug(prog).unwrap();
}
#[test]
fn basic3() {
    let prog = r#"
       func Counter(n){ 
            func wrapper(){
                func add(){
                    assign n = n + 1;
                }
                func sub(){
                    assign n = n - 1;
                }
                func count(){
                    return n;
                }
                return Dict("add">add,"sub">sub,"count">count);
            }
            return wrapper;
       }
       var counter = Counter(5)();
       counter.sub();
       counter.sub();
       counter["sub"]();
       counter.add();
       print(counter["count"]());
    "#;
    run_string_debug(prog).unwrap();
}
#[test]
fn basic4() {
    let prog = r#"
       var vec2 = Vec2(2,3);
       var vec3 = Vec3(3,4,5);
       vec2[1];
       vec3[2];
       var arr = Array(Vec2(1,2),2,3,Dict("233">3));
       arr[3]["233"];
    "#;
    run_string_debug(prog).unwrap();
}
#[test]
fn basic5() {
    let prog = r#"
       func wrong(){
           except 2333;
       }
       var e = wrong()?.info;
       print(wrong()?);
    "#;
    run_string_debug(prog).unwrap();
}

#[test]
fn basic6() {
    let prog = r#"
       var arr = Array(Nil,Nil,Nil);
       assign arr[1] = arr;
       print(arr);
    "#;
    run_string_debug(prog).unwrap();
}
#[test]
fn basic7() {
    let prog = r#"
    //233,it's a comment
       var arr = Array("233",233,Vec2(2,3),Vec3(2,3,3),Dict("i">233));
       {
           var i = 0;
           while(i < 5){
               var elem = arr[i];
               print(typeof(elem));
               print(typeof(elem) == "Dict");
               assign i = i + 1;
           }
       }
       var i = 1;
    "#;
    run_string_debug(prog).unwrap();
}
#[test]
fn basic8() {
    let prog = r#"
    func fib(n){
        var arr = Array(1,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0);
        var i = 2;
        while(i < n){
            assign arr[i] = arr[i-1] + arr[i-2];
            assign i = i + 1;
        }
        print(arr);
        return arr[19]; 
   }
   fib(20);

   "#;
   run_string_debug(prog).unwrap();
}