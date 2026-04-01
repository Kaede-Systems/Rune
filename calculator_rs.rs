fn add(a: i32, b: i32) -> i32 { a + b }
fn sub(a: i32, b: i32) -> i32 { a - b }
fn mul(a: i32, b: i32) -> i32 { a * b }
fn divide_int(a: i32, b: i32) -> i32 { a / b }
fn modulo(a: i32, b: i32) -> i32 { a % b }
fn eq(a: i32, b: i32) -> bool { a == b }
fn ne(a: i32, b: i32) -> bool { a != b }
fn gt(a: i32, b: i32) -> bool { a > b }
fn ge(a: i32, b: i32) -> bool { a >= b }
fn lt(a: i32, b: i32) -> bool { a < b }
fn le(a: i32, b: i32) -> bool { a <= b }

fn run_benchmark(limit: i32) -> i64 {
    let mut i: i32 = 1;
    let mut total: i64 = 0;

    while i <= limit {
        total += add(i, 3) as i64;
        total += sub(i, 1) as i64;
        total += mul(i, 2) as i64;
        total += divide_int(i + 8, 3) as i64;
        total += modulo(i + 11, 7) as i64;

        if eq(modulo(i, 2), 0) { total += 1; }
        if ne(modulo(i, 3), 0) { total += 1; }
        if gt(i, 10) { total += 1; }
        if ge(i, 10) { total += 1; }
        if lt(i, limit) { total += 1; }
        if le(i, limit) { total += 1; }

        i += 1;
    }

    total
}

fn main() {
    let x: i32 = 42;
    let y: i32 = 5;

    println!("Rust calculator");
    println!("add= {}", add(x, y));
    println!("sub= {}", sub(x, y));
    println!("mul= {}", mul(x, y));
    println!("div= {}", divide_int(x, y));
    println!("mod= {}", modulo(x, y));
    println!("eq= {}", eq(x, y));
    println!("ne= {}", ne(x, y));
    println!("gt= {}", gt(x, y));
    println!("ge= {}", ge(x, y));
    println!("lt= {}", lt(x, y));
    println!("le= {}", le(x, y));
    println!("checksum= {}", run_benchmark(200000));
}
