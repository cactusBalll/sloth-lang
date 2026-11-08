# python equivalent of benches/programs/sieve.slt
n = 500000
a = []
i = 0
while i < n:
    a.append(1)
    i = i + 1
a[0] = 0
a[1] = 0
i = 2
while i * i < n:
    if a[i] == 1:
        j = i * i
        while j < n:
            a[j] = 0
            j = j + i
    i = i + 1
count = 0
i = 0
while i < n:
    count = count + a[i]
    i = i + 1
print(count)
