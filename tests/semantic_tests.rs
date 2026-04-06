use rune::semantic::{Type, check_source};

#[test]
fn checks_valid_async_program() {
    let checked = check_source(
        "async def main() -> i32:\n    let name = await input()\n    println(name)\n    return 0\n",
    )
    .expect("program should check");

    assert_eq!(checked.functions.len(), 1);
    assert_eq!(checked.functions[0].return_type, Type::I32);
}

#[test]
fn checks_raised_error_type() {
    let checked = check_source("def fail() -> unit raises String:\n    raise \"bad\"\n")
        .expect("program should check");

    assert_eq!(checked.functions[0].raises, Some(Type::String));
}

#[test]
fn rejects_unknown_variable() {
    let error = check_source("def main() -> i32:\n    return missing\n")
        .expect_err("missing variable should fail");
    assert!(error.message.contains("unknown identifier"));
}

#[test]
fn rejects_bad_return_type() {
    let error = check_source("def main() -> i32:\n    return \"bad\"\n")
        .expect_err("wrong return type should fail");
    assert!(
        error
            .message
            .contains("return value expected `i32`, found `String`")
    );
}

#[test]
fn rejects_raise_without_raises() {
    let error = check_source("def main() -> unit:\n    raise \"bad\"\n")
        .expect_err("raise without raises should fail");
    assert!(error.message.contains("cannot `raise`"));
}

#[test]
fn rejects_await_in_sync_function() {
    let error = check_source("def main() -> String:\n    return await input()\n")
        .expect_err("await in sync function should fail");
    assert!(error.message.contains("`await` is only allowed"));
}

#[test]
fn rejects_break_outside_loop() {
    let error = check_source("def main() -> i32:\n    break\n")
        .expect_err("break outside a loop should fail");
    assert!(error.message.contains("`break` is only allowed inside a loop"));
}

#[test]
fn rejects_continue_outside_loop() {
    let error = check_source("def main() -> i32:\n    continue\n")
        .expect_err("continue outside a loop should fail");
    assert!(error.message.contains("`continue` is only allowed inside a loop"));
}

#[test]
fn rejects_wrong_argument_count() {
    let error = check_source(
        "def add(a: i32, b: i32) -> i32:\n    return a + b\n\ndef main() -> i32:\n    return add(1)\n",
    )
    .expect_err("wrong arity should fail");
    assert!(error.message.contains("expects 2 arguments but got 1"));
}

#[test]
fn accepts_keyword_arguments() {
    let checked = check_source(
        "def add(lhs: i32, rhs: i32) -> i32:\n    return lhs + rhs\n\ndef main() -> i32:\n    return add(rhs=22, lhs=20)\n",
    )
    .expect("keyword arguments should check");
    assert_eq!(checked.functions.len(), 2);
}

#[test]
fn rejects_unknown_keyword_argument() {
    let error = check_source(
        "def add(lhs: i32, rhs: i32) -> i32:\n    return lhs + rhs\n\ndef main() -> i32:\n    return add(value=1, rhs=2)\n",
    )
    .expect_err("unknown keyword should fail");
    assert!(error.message.contains("has no parameter named `value`"));
}

#[test]
fn checks_std_time_builtin_wrapper() {
    let checked = check_source("def now() -> i64:\n    return __rune_builtin_time_now_unix()\n")
        .expect("builtin time call should check");
    assert_eq!(checked.functions[0].return_type, Type::I64);
}

#[test]
fn checks_env_and_network_builtins() {
    let checked = check_source(
        "def demo() -> bool:\n    let count: i32 = __rune_builtin_env_arg_count()\n    let port: i32 = __rune_builtin_env_get_i32(\"PORT\", 8080)\n    return __rune_builtin_network_tcp_connect(\"127.0.0.1\", port)\n",
    )
    .expect("env and network builtins should check");
    assert_eq!(checked.functions[0].return_type, Type::Bool);
}

#[test]
fn checks_extended_stdlib_builtins() {
    let checked = check_source(
        "def demo() -> unit:\n    let cpus: i32 = __rune_builtin_system_cpu_count()\n    let mono: i64 = __rune_builtin_time_monotonic_ms()\n    let enabled: bool = __rune_builtin_env_get_bool(\"ENABLED\", false)\n    let ok: bool = __rune_builtin_network_tcp_connect_timeout(\"127.0.0.1\", cpus, 250)\n    __rune_builtin_time_sleep_ms(mono)\n    println(enabled)\n    println(ok)\n    return\n",
    )
    .expect("extended stdlib builtins should check");
    assert_eq!(checked.functions[0].return_type, Type::Unit);
}

#[test]
fn checks_fs_terminal_and_audio_builtins() {
    let checked = check_source(
        "def demo(path: String) -> bool:\n    let before: bool = __rune_builtin_fs_exists(path)\n    let wrote: bool = __rune_builtin_fs_write_string(path, \"hello\")\n    let text: String = __rune_builtin_fs_read_string(path)\n    __rune_builtin_terminal_clear()\n    __rune_builtin_terminal_move_to(1, 1)\n    __rune_builtin_terminal_hide_cursor()\n    __rune_builtin_terminal_set_title(text)\n    __rune_builtin_terminal_show_cursor()\n    return wrote and (before or __rune_builtin_audio_bell())\n",
    )
    .expect("fs, terminal, and audio builtins should check");
    assert_eq!(checked.functions[0].return_type, Type::Bool);
}

#[test]
fn checks_json_builtins_and_stdlib_shape() {
    let checked = check_source(
        "def main() -> unit:\n    let doc: Json = __rune_builtin_json_parse(\"{\\\"name\\\":\\\"Rune\\\",\\\"nums\\\":[1,2,3],\\\"ok\\\":true}\")\n    let name: String = __rune_builtin_json_to_string(__rune_builtin_json_get(doc, \"name\"))\n    let second: i64 = __rune_builtin_json_to_i64(__rune_builtin_json_index(__rune_builtin_json_get(doc, \"nums\"), 1))\n    let ok: bool = __rune_builtin_json_to_bool(__rune_builtin_json_get(doc, \"ok\"))\n    println(__rune_builtin_json_stringify(doc))\n    println(__rune_builtin_json_kind(doc))\n    println(str(__rune_builtin_json_is_null(__rune_builtin_json_get(doc, \"missing\"))))\n    println(__rune_builtin_json_len(__rune_builtin_json_get(doc, \"nums\")))\n    println(name)\n    println(second)\n    println(str(ok))\n    return\n",
    )
    .expect("json stdlib program should check");
    assert_eq!(checked.functions[0].return_type, Type::Unit);
}

#[test]
fn accepts_direct_json_equality() {
    let checked = check_source(
        "def main() -> unit:\n    let left: Json = __rune_builtin_json_parse(\"1\")\n    let right: Json = __rune_builtin_json_parse(\"1\")\n    if left == right:\n        println(\"same\")\n    return\n",
    )
    .expect("direct json equality should check");
    assert_eq!(checked.functions[0].return_type, Type::Unit);
}

#[test]
fn infers_untyped_locals_from_values() {
    let checked =
        check_source("def main() -> unit:\n    let value = 42\n    println(value)\n    return\n")
            .expect("untyped locals should check through inference");
    assert_eq!(checked.functions[0].return_type, Type::Unit);
}

#[test]
fn accepts_dynamic_parameters_from_untyped_signatures() {
    let checked = check_source(
        "def echo(value) -> unit:\n    println(value)\n    return\n\n\
         def main() -> unit:\n    echo(42)\n    echo(\"hi\")\n    return\n",
    )
    .expect("dynamic parameters should accept primitive arguments");
    assert_eq!(checked.functions.len(), 2);
}

#[test]
fn accepts_reassignment_for_matching_types() {
    let checked =
        check_source("def main() -> unit:\n    let value: i32 = 1\n    value = 2\n    return\n")
            .expect("typed reassignment should check");
    assert_eq!(checked.functions[0].return_type, Type::Unit);
}

#[test]
fn rejects_reassignment_for_mismatched_static_types() {
    let error =
        check_source("def main() -> unit:\n    let value: i32 = 1\n    value = true\n    return\n")
            .expect_err("mismatched reassignment should fail");
    assert!(
        error
            .message
            .contains("assignment value expected `i32`, found `bool`")
    );
}

#[test]
fn accepts_explicit_dynamic_reassignment_to_new_types() {
    let checked = check_source(
        "def main() -> unit:\n    let value: dynamic = 1\n    value = true\n    value = \"hi\"\n    return\n",
    )
    .expect("explicit dynamic reassignment should allow type changes");
    assert_eq!(checked.functions[0].return_type, Type::Unit);
}

#[test]
fn infers_struct_type_for_untyped_object_locals() {
    let checked = check_source(
        "class Point:\n    x: i32\n    def value(self) -> i32:\n        return self.x\n\n\
         def make() -> Point:\n    return Point(x=7)\n\n\
         def main() -> i32:\n    let point = make()\n    return point.value()\n",
    )
    .expect("untyped object locals should infer their struct type");
    assert_eq!(checked.functions.len(), 3);
}

#[test]
fn checks_string_concat_and_conversions() {
    let checked = check_source(
        "def main() -> unit:\n    let name: String = str(42)\n    let full: String = \"hi \" + name\n    let value: i64 = int(full)\n    println(value)\n    return\n",
    )
    .expect("string concat and conversions should check");
    assert_eq!(checked.functions[0].return_type, Type::Unit);
}

#[test]
fn accepts_i64_comparisons_with_i32_integer_literals() {
    let checked = check_source(
        "def main() -> unit:\n    let b: i64 = 10\n    if b == 0:\n        println(\"zero\")\n    if b > 1:\n        println(\"gt\")\n    return\n",
    )
    .expect("mixed i64 comparisons with integer literals should check");
    assert_eq!(checked.functions[0].return_type, Type::Unit);
}

#[test]
fn widens_i32_literals_for_i64_function_arguments() {
    let checked = check_source(
        "def takes_i64(value: i64) -> unit:\n    println(value)\n    return\n\n\
         def main() -> unit:\n    takes_i64(0)\n    return\n",
    )
    .expect("i32 integer literals should widen to i64 arguments");
    assert_eq!(checked.functions.len(), 2);
}

#[test]
fn accepts_dynamic_add_for_numbers_and_strings() {
    let checked = check_source(
        "def main() -> unit:\n    let value = 40\n    value = value + 2\n    value = value + \"!\"\n    println(value)\n    return\n",
    )
    .expect("dynamic + should check for numeric add and string concat");
    assert_eq!(checked.functions[0].return_type, Type::Unit);
}

#[test]
fn accepts_dynamic_comparisons() {
    let checked = check_source(
        "def main() -> unit:\n    let value = 40\n    if value == 40:\n        println(\"eq\")\n    if value < 50:\n        println(\"lt\")\n    value = \"40\"\n    if value == 40:\n        println(\"str-eq\")\n    return\n",
    )
    .expect("dynamic comparisons should check");
    assert_eq!(checked.functions[0].return_type, Type::Unit);
}

#[test]
fn accepts_dynamic_numeric_arithmetic() {
    let checked = check_source(
        "def main() -> unit:\n    let value = 10\n    value = value - 3\n    value = value * 5\n    value = value / 7\n    println(value)\n    return\n",
    )
    .expect("dynamic numeric arithmetic should check");
    assert_eq!(checked.functions[0].return_type, Type::Unit);
}

#[test]
fn accepts_boolean_operators_and_dynamic_conditions() {
    let checked = check_source(
        "def main() -> unit:\n    let value = 1\n    value = true\n    if value and not false:\n        println(\"yes\")\n    while false or false:\n        return\n    return\n",
    )
    .expect("boolean operators and dynamic conditions should check");
    assert_eq!(checked.functions[0].return_type, Type::Unit);
}

#[test]
fn accepts_modulo_for_static_and_dynamic_values() {
    let checked = check_source(
        "def main() -> unit:\n    let a: i32 = 10 % 3\n    let value = 10\n    value = true\n    value = 10 % 4\n    println(a)\n    println(value)\n    return\n",
    )
    .expect("modulo should check");
    assert_eq!(checked.functions[0].return_type, Type::Unit);
}

#[test]
fn rejects_division_by_zero_literal() {
    let error = check_source("def main() -> unit:\n    println(10 / 0)\n    return\n")
        .expect_err("division by zero literal should fail");
    assert!(error.message.contains("division by zero"));
}

#[test]
fn rejects_modulo_by_zero_literal() {
    let error = check_source("def main() -> unit:\n    println(10 % 0)\n    return\n")
        .expect_err("modulo by zero literal should fail");
    assert!(error.message.contains("modulo by zero"));
}

#[test]
fn rejects_bad_string_concat_types() {
    // String + integer is now valid dynamic addition (returns Dynamic).
    // Check that it compiles without error.
    check_source("def main() -> unit:\n    let value = \"x\" + 1\n    return\n")
        .expect("String + integer should compile as dynamic add");

    // Truly unsupported: bool + bool has no defined + semantics.
    let error =
        check_source("def main() -> unit:\n    let a = true\n    let b = true\n    let c = a + b\n    return\n")
            .expect_err("bool + bool addition should fail");
    assert!(error.message.contains("binary `+` requires"));
}

#[test]
fn checks_struct_construction_and_field_access() {
    let checked = check_source(
        "struct Point:\n    x: i32\n    y: i32\n\n\
         def main() -> i32:\n    let point: Point = Point(x=20, y=22)\n    return point.x + point.y\n",
    )
    .expect("struct construction should check");
    assert_eq!(checked.functions[0].return_type, Type::I32);
}

#[test]
fn checks_class_construction_and_field_access() {
    let checked = check_source(
        "class Point:\n    x: i32\n    y: i32\n\n\
         def main() -> i32:\n    let point: Point = Point(x=20, y=22)\n    return point.x + point.y\n",
    )
    .expect("class construction should check");
    assert_eq!(checked.functions[0].return_type, Type::I32);
}

#[test]
fn checks_fstrings() {
    let checked = check_source(
        "def main() -> unit:\n    let value: String = f\"sum={40 + 2} ok={true}\"\n    println(value)\n    return\n",
    )
    .expect("f-strings should check");
    assert_eq!(checked.functions[0].return_type, Type::Unit);
}

#[test]
fn checks_class_method_call() {
    let checked = check_source(
        "class Point:\n    x: i32\n    y: i32\n    def sum(self) -> i32:\n        return self.x + self.y\n\n\
         def main() -> i32:\n    let point: Point = Point(x=20, y=22)\n    return point.sum()\n",
    )
    .expect("class method call should check");
    assert!(checked.functions.iter().any(|function| function.name == "Point__sum"));
    assert!(checked
        .functions
        .iter()
        .any(|function| function.name == "main" && function.return_type == Type::I32));
}

#[test]
fn rejects_unknown_struct_field() {
    let error = check_source(
        "struct Point:\n    x: i32\n\n\
         def main() -> i32:\n    let point: Point = Point(x=20)\n    return point.y\n",
    )
    .expect_err("missing field should fail");
    assert!(error.message.contains("has no field `y`"));
}

#[test]
fn checks_extern_function_signatures() {
    let checked = check_source(
        "extern def add_from_c(a: i32, b: i32) -> i32\n\ndef main() -> i32:\n    return add_from_c(20, 22)\n",
    )
    .expect("extern function call should check");
    assert_eq!(checked.functions.len(), 2);
    assert!(checked.functions[0].is_extern);
}

#[test]
fn checks_extern_string_function_signatures() {
    let checked = check_source(
        "extern def greet_from_c(name: String) -> String\n\ndef main() -> i32:\n    println(greet_from_c(\"Rune\"))\n    return 0\n",
    )
    .expect("extern string function call should check");
    assert_eq!(checked.functions.len(), 2);
    assert!(checked.functions[0].is_extern);
}

#[test]
fn rejects_unsupported_extern_types() {
    let error = check_source("struct Point:\n    x: i32\n\nextern def bad(name: Point) -> i32\n")
        .expect_err("unsupported extern params should fail");
    assert!(error.message.contains("must use bool, i32, i64, String, or unit"));
}

#[test]
fn checks_bitwise_ops_on_integers() {
    let checked = check_source(
        "def f(a: i64, b: i64) -> i64:\n    return a & b\n",
    )
    .expect("bitwise and on integers should check");
    assert_eq!(checked.functions[0].return_type, Type::I64);
}

#[test]
fn checks_bitwise_or_and_xor() {
    let checked = check_source(
        "def f(a: i64, b: i64) -> i64:\n    let x: i64 = a | b\n    let y: i64 = a ^ b\n    return x\n",
    )
    .expect("bitwise or/xor should check");
    assert_eq!(checked.functions[0].return_type, Type::I64);
}

#[test]
fn checks_shift_ops_on_integers() {
    let checked = check_source(
        "def f(a: i64) -> i64:\n    return a << 2\n",
    )
    .expect("shift on integer should check");
    assert_eq!(checked.functions[0].return_type, Type::I64);
}

#[test]
fn rejects_bitwise_ops_on_non_integers() {
    let error = check_source(
        "def f(a: String, b: String) -> String:\n    return a & b\n",
    )
    .expect_err("bitwise and on strings should fail");
    assert!(error.message.contains("requires integer") || error.message.contains("integer"));
}

#[test]
fn checks_field_assignment_on_struct() {
    check_source(
        "struct Point:\n    x: i64\n    y: i64\n\ndef move_x(p: Point, dx: i64) -> unit:\n    p.x = dx\n",
    )
    .expect("field assignment should check on struct");
}

#[test]
fn checks_string_len_returns_i64() {
    let checked = check_source(
        "def f(s: String) -> i64:\n    return s.len()\n",
    )
    .expect("String.len() should type-check");
    assert_eq!(checked.functions[0].return_type, Type::I64);
}

#[test]
fn checks_string_upper_returns_string() {
    let checked = check_source(
        "def f(s: String) -> String:\n    return s.upper()\n",
    )
    .expect("String.upper() should type-check");
    assert_eq!(checked.functions[0].return_type, Type::String);
}

#[test]
fn checks_string_lower_returns_string() {
    let checked = check_source(
        "def f(s: String) -> String:\n    return s.lower()\n",
    )
    .expect("String.lower() should type-check");
    assert_eq!(checked.functions[0].return_type, Type::String);
}

#[test]
fn checks_string_strip_returns_string() {
    let checked = check_source(
        "def f(s: String) -> String:\n    return s.strip()\n",
    )
    .expect("String.strip() should type-check");
    assert_eq!(checked.functions[0].return_type, Type::String);
}

#[test]
fn checks_string_contains_returns_bool() {
    let checked = check_source(
        "def f(s: String) -> bool:\n    return s.contains(\"world\")\n",
    )
    .expect("String.contains() should type-check");
    assert_eq!(checked.functions[0].return_type, Type::Bool);
}

#[test]
fn checks_string_starts_with_returns_bool() {
    let checked = check_source(
        "def f(s: String) -> bool:\n    return s.starts_with(\"he\")\n",
    )
    .expect("String.starts_with() should type-check");
    assert_eq!(checked.functions[0].return_type, Type::Bool);
}

#[test]
fn checks_string_ends_with_returns_bool() {
    let checked = check_source(
        "def f(s: String) -> bool:\n    return s.ends_with(\"ld\")\n",
    )
    .expect("String.ends_with() should type-check");
    assert_eq!(checked.functions[0].return_type, Type::Bool);
}

#[test]
fn checks_string_replace_returns_string() {
    let checked = check_source(
        "def f(s: String) -> String:\n    return s.replace(\"hello\", \"hi\")\n",
    )
    .expect("String.replace() should type-check");
    assert_eq!(checked.functions[0].return_type, Type::String);
}

#[test]
fn rejects_string_method_wrong_arg_count() {
    let error = check_source(
        "def f(s: String) -> i64:\n    return s.len(\"extra\")\n",
    )
    .expect_err("String.len with args should fail");
    assert!(error.message.contains("len"));
}

#[test]
fn rejects_string_contains_wrong_arg_type() {
    let error = check_source(
        "def f(s: String) -> bool:\n    return s.contains(42)\n",
    )
    .expect_err("String.contains with i64 arg should fail");
    assert!(error.message.contains("contains") || error.message.contains("String"));
}

#[test]
fn rejects_unknown_string_method() {
    let error = check_source(
        "def f(s: String) -> i64:\n    return s.nonexistent()\n",
    )
    .expect_err("unknown String method should fail");
    assert!(error.message.contains("nonexistent") || error.message.contains("method"));
}
