import random as rd

uop = ["+", "-", "!"]
bop = ["+", "-", "*", "==", "!=", "<", "<=", ">", ">=", "&&", "||"]
sym = []

VMIN = -8
VMAX = 7
NEXPR = 10

print("int main() {")

def gen_expr():
    r = rd.randint(0, 9)
    if r <= 2:
        print(rd.randint(VMIN, VMAX), end="")
    elif r <= 5:
        if len(sym) == 0:
            print(rd.randint(VMIN, VMAX), end="")
        else:
            print(rd.choice(sym), end="")
    elif r <= 7:
        print(rd.choice(uop), end="")
        print("(", end="")
        gen_expr()
        print(")", end="")
    else:
        print("(", end="")
        gen_expr()
        print(")", end="")
        print(rd.choice(bop), end="")
        print("(", end="")
        gen_expr()
        print(")", end="")

for i in range(NEXPR):
    print(f"    const int a{i} = ", end="")
    gen_expr()
    print(";")
    sym.append(f"a{i}")

print(f"    return a{rd.randint(0, NEXPR-1)} % 255;")

print("}")
