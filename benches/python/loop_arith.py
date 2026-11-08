# python equivalent of benches/programs/loop_arith.slt
i = 0
acc = 0
while i < 2000000:
    acc = acc + (i * 3) % 7 - 1
    i = i + 1
print(acc)
