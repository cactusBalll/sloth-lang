# python equivalent of benches/programs/object_methods.slt
class Counter:
    def __init__(self, start):
        self.value = start

    def add(self, x):
        self.value = self.value + x
        return self.value

    def norm(self):
        return self.value * 2

c = Counter(0)
i = 0
acc = 0
while i < 300000:
    c.add(i)
    acc = acc + c.norm()
    i = i + 1
print(c.value, acc)
