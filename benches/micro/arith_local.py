# python equivalent of benches/micro/arith_local.slt
def run():
    i = 0
    acc = 0
    while i < 2000000:
        acc = acc + (i * 3) % 7 - 1
        i = i + 1
    return acc
print(run())
