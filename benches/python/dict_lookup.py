# python equivalent of benches/programs/dict_lookup.slt
d = {"a": 1, "b": 2, "c": 3, "d": 4, "e": 5}
i = 0
acc = 0
while i < 1000000:
    acc = acc + d["a"] + d["c"] + d["e"]
    i = i + 1
print(acc)
