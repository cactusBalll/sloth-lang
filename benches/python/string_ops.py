# python equivalent of benches/programs/string_ops.slt
parts = "abcdefghij"
s = ""
i = 0
while i < 10000:
    s = s + parts
    i = i + 1
count = 0
for ch in s:
    if ch == "a":
        count = count + 1
print(count)
