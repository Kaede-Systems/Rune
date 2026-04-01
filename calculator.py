def add(a: int, b: int) -> int:
    return a + b


def sub(a: int, b: int) -> int:
    return a - b


def mul(a: int, b: int) -> int:
    return a * b


def div(a: int, b: int) -> int:
    return a // b


def mod(a: int, b: int) -> int:
    return a % b


def eq(a: int, b: int) -> bool:
    return a == b


def ne(a: int, b: int) -> bool:
    return a != b


def gt(a: int, b: int) -> bool:
    return a > b


def ge(a: int, b: int) -> bool:
    return a >= b


def lt(a: int, b: int) -> bool:
    return a < b


def le(a: int, b: int) -> bool:
    return a <= b


def run_benchmark(limit: int) -> int:
    i = 1
    total = 0

    while i <= limit:
        total += add(i, 3)
        total += sub(i, 1)
        total += mul(i, 2)
        total += div(i + 8, 3)
        total += mod(i + 11, 7)

        if eq(mod(i, 2), 0):
            total += 1
        if ne(mod(i, 3), 0):
            total += 1
        if gt(i, 10):
            total += 1
        if ge(i, 10):
            total += 1
        if lt(i, limit):
            total += 1
        if le(i, limit):
            total += 1

        i += 1

    return total


def main() -> None:
    x = 42
    y = 5

    print("Python calculator")
    print("add=", add(x, y))
    print("sub=", sub(x, y))
    print("mul=", mul(x, y))
    print("div=", div(x, y))
    print("mod=", mod(x, y))
    print("eq=", eq(x, y))
    print("ne=", ne(x, y))
    print("gt=", gt(x, y))
    print("ge=", ge(x, y))
    print("lt=", lt(x, y))
    print("le=", le(x, y))
    print("checksum=", run_benchmark(200000))


if __name__ == "__main__":
    main()
