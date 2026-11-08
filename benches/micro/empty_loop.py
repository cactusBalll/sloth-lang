# python equivalent of benches/micro/empty_loop.slt
def run():
    i = 0
    while i < 2000000:
        i = i + 1
    return i
print(run())
