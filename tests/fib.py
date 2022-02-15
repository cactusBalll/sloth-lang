import time
def bench_func(f):
    def closure(*arg):
        start = time.time()
        ret = f(*arg)
        print(f"{(time.time() - start)*1000:.5f} ms")
        return ret  
    return closure
@bench_func
def call_fib(x):
     print(fib(x))

def fib(x):
    if x == 1 or x == 0:
        return 1
    else:
        return fib(x - 1) + fib(x - 2)

#call_fib(20)

@bench_func
def fib1(n):
    arr = [1,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]
    i = 2
    while(i < n):
        arr[i] = arr[i-1] + arr[i-2]
        i = i + 1
    #print(arr)
    return arr[19] 
       
print(fib1(20))