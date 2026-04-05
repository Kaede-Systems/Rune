use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArduinoUnoEntrypointKind {
    Main,
    SetupLoop,
}

pub fn rewrite_arduino_uno_cbe_llvm_ir(
    llvm_ir: &str,
    entrypoint: ArduinoUnoEntrypointKind,
) -> String {
    let mut rename_map = HashMap::new();
    for line in llvm_ir.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("define ") {
            continue;
        }
        let Some(at_index) = trimmed.find('@') else {
            continue;
        };
        let name_start = at_index + 1;
        let name_end = trimmed[name_start..]
            .find('(')
            .map(|index| name_start + index)
            .unwrap_or(trimmed.len());
        let name = &trimmed[name_start..name_end];
        if name.starts_with("rune_rt_") {
            continue;
        }
        if matches!(
            (entrypoint, name),
            (ArduinoUnoEntrypointKind::Main, "main")
                | (ArduinoUnoEntrypointKind::SetupLoop, "setup")
                | (ArduinoUnoEntrypointKind::SetupLoop, "loop")
        ) {
            continue;
        }
        rename_map.insert(name.to_string(), format!("rune_cbe_{name}"));
    }

    rewrite_llvm_global_identifiers(llvm_ir, &rename_map)
}

pub fn rewrite_arduino_uno_cbe_source(
    c_source: &str,
    entrypoint: ArduinoUnoEntrypointKind,
) -> String {
    let renamed = match entrypoint {
        ArduinoUnoEntrypointKind::Main => {
            c_source.replace("int main(void)", "int rune_entry_main(void)")
        }
        ArduinoUnoEntrypointKind::SetupLoop => c_source
            .replace("void setup(void)", "void rune_entry_setup(void)")
            .replace("void loop(void)", "void rune_entry_loop(void)"),
    };
    internalize_rune_cbe_c_functions(&renamed)
}

fn rewrite_llvm_global_identifiers(source: &str, rename_map: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'@' {
            let start = index + 1;
            let mut end = start;
            while end < bytes.len() {
                let ch = bytes[end];
                if ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'.' {
                    end += 1;
                } else {
                    break;
                }
            }
            if end > start {
                let name = &source[start..end];
                if let Some(replacement) = rename_map.get(name) {
                    out.push('@');
                    out.push_str(replacement);
                    index = end;
                    continue;
                }
            }
        }
        out.push(bytes[index] as char);
        index += 1;
    }
    out
}

fn internalize_rune_cbe_c_functions(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    for line in source.lines() {
        let trimmed = line.trim_start();
        let function_name_start = trimmed.find(" rune_cbe_");
        let Some(function_name_start) = function_name_start else {
            out.push_str(line);
            out.push('\n');
            continue;
        };
        let function_name_start = function_name_start + 1;
        let Some(paren_index) = trimmed[function_name_start..]
            .find('(')
            .map(|index| function_name_start + index)
        else {
            out.push_str(line);
            out.push('\n');
            continue;
        };
        let is_function_decl_or_def = trimmed.ends_with('{')
            || trimmed.ends_with(';')
            || trimmed.ends_with(" ;")
            || trimmed.contains("__FUNCTIONALIGN__");
        if trimmed.starts_with("static ")
            || trimmed.starts_with("/*")
            || trimmed.contains('=')
            || !is_function_decl_or_def
            || !trimmed[function_name_start..paren_index].starts_with("rune_cbe_")
        {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let indent_len = line.len() - trimmed.len();
        out.push_str(&line[..indent_len]);
        out.push_str("static ");
        out.push_str(trimmed);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        rewrite_arduino_uno_cbe_llvm_ir, rewrite_arduino_uno_cbe_source,
        ArduinoUnoEntrypointKind,
    };

    #[test]
    fn rewrites_non_runtime_functions_for_main_entry() {
        let llvm_ir = "\
define i64 @main() {\n\
  ret i64 0\n\
}\n\
define i64 @helper() {\n\
  ret i64 1\n\
}\n\
define void @rune_rt_fail(i32 %code) {\n\
  ret void\n\
}\n";
        let rewritten = rewrite_arduino_uno_cbe_llvm_ir(llvm_ir, ArduinoUnoEntrypointKind::Main);
        assert!(rewritten.contains("@main()"));
        assert!(rewritten.contains("@rune_cbe_helper()"));
        assert!(rewritten.contains("@rune_rt_fail(i32 %code)"));
    }

    #[test]
    fn preserves_setup_loop_entrypoints() {
        let llvm_ir = "\
define void @setup() {\n\
  ret void\n\
}\n\
define void @loop() {\n\
  ret void\n\
}\n\
define i64 @helper() {\n\
  ret i64 1\n\
}\n";
        let rewritten =
            rewrite_arduino_uno_cbe_llvm_ir(llvm_ir, ArduinoUnoEntrypointKind::SetupLoop);
        assert!(rewritten.contains("@setup()"));
        assert!(rewritten.contains("@loop()"));
        assert!(rewritten.contains("@rune_cbe_helper()"));
    }

    #[test]
    fn internalizes_rune_cbe_c_helpers() {
        let c_source = "\
void rune_cbe_helper(void) __FUNCTIONALIGN__(1) ;\n\
\n\
void rune_cbe_helper(void) {\n\
}\n\
\n\
int main(void) {\n\
  return 0;\n\
}\n";
        let rewritten = rewrite_arduino_uno_cbe_source(c_source, ArduinoUnoEntrypointKind::Main);
        assert!(rewritten.contains("static void rune_cbe_helper(void) __FUNCTIONALIGN__(1) ;"));
        assert!(rewritten.contains("static void rune_cbe_helper(void) {"));
        assert!(rewritten.contains("int rune_entry_main(void) {"));
    }
}
