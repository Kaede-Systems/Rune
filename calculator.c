#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>

static int32_t add(int32_t a, int32_t b) { return a + b; }
static int32_t sub(int32_t a, int32_t b) { return a - b; }
static int32_t mul(int32_t a, int32_t b) { return a * b; }
static int32_t divide_int(int32_t a, int32_t b) { return a / b; }
static int32_t mod(int32_t a, int32_t b) { return a % b; }
static bool eq(int32_t a, int32_t b) { return a == b; }
static bool ne(int32_t a, int32_t b) { return a != b; }
static bool gt(int32_t a, int32_t b) { return a > b; }
static bool ge(int32_t a, int32_t b) { return a >= b; }
static bool lt(int32_t a, int32_t b) { return a < b; }
static bool le(int32_t a, int32_t b) { return a <= b; }

static int64_t run_benchmark(int32_t limit) {
    int32_t i = 1;
    int64_t total = 0;

    while (i <= limit) {
        total += add(i, 3);
        total += sub(i, 1);
        total += mul(i, 2);
        total += divide_int(i + 8, 3);
        total += mod(i + 11, 7);

        if (eq(mod(i, 2), 0)) total += 1;
        if (ne(mod(i, 3), 0)) total += 1;
        if (gt(i, 10)) total += 1;
        if (ge(i, 10)) total += 1;
        if (lt(i, limit)) total += 1;
        if (le(i, limit)) total += 1;

        i += 1;
    }

    return total;
}

int main(void) {
    int32_t x = 42;
    int32_t y = 5;

    printf("C calculator\n");
    printf("add= %d\n", add(x, y));
    printf("sub= %d\n", sub(x, y));
    printf("mul= %d\n", mul(x, y));
    printf("div= %d\n", divide_int(x, y));
    printf("mod= %d\n", mod(x, y));
    printf("eq= %s\n", eq(x, y) ? "true" : "false");
    printf("ne= %s\n", ne(x, y) ? "true" : "false");
    printf("gt= %s\n", gt(x, y) ? "true" : "false");
    printf("ge= %s\n", ge(x, y) ? "true" : "false");
    printf("lt= %s\n", lt(x, y) ? "true" : "false");
    printf("le= %s\n", le(x, y) ? "true" : "false");
    printf("checksum= %lld\n", (long long)run_benchmark(200000));
    return 0;
}
