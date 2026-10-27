use serde_json::Value;

// ---------------------------------------------------------------------------
// Cleaning (pruning empty structures)
// ---------------------------------------------------------------------------

pub fn clean(value: &Value) -> Value {
    let mut cloned = value.clone();
    clean_in_place(&mut cloned);
    cloned
}

pub fn clean_in_place(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (_, v) in map.iter_mut() {
                clean_in_place(v);
            }
            // retain, not collect-keys-then-remove: cloning every key just to
            // drop some of them is pure allocation churn on large documents.
            map.retain(|_, v| !is_empty(v));
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                clean_in_place(item);
            }
            arr.retain(|v| !is_empty(v));
        }
        _ => {}
    }
}

fn is_empty(val: &Value) -> bool {
    match val {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Array(arr) => arr.is_empty(),
        Value::Object(map) => map.is_empty(),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Scalar Analysis & Formatting — regex-free
// ---------------------------------------------------------------------------

fn is_bool_like(s: &str) -> bool {
    matches!(s, "true" | "True" | "TRUE" | "false" | "False" | "FALSE")
}

fn is_int_like(s: &str) -> bool {
    let t = s.strip_prefix(['+', '-']).unwrap_or(s);
    if t.is_empty() {
        return false;
    }
    if t == "0" {
        return true;
    }
    if let Some(rest) = t.strip_prefix("0b") {
        if rest.is_empty() {
            return false;
        }
        return rest.chars().all(|c| c == '0' || c == '1' || c == '_');
    }
    if let Some(rest) = t.strip_prefix("0x") {
        if rest.is_empty() {
            return false;
        }
        return rest.chars().all(|c| c.is_ascii_hexdigit() || c == '_');
    }
    if let Some(after_zero) = t.strip_prefix('0') {
        let rest = after_zero.strip_prefix('o').unwrap_or(after_zero);
        if !rest.is_empty() && rest.chars().all(|c| matches!(c, '0'..='7' | '_')) {
            return true;
        }
    }
    let mut chars = t.chars();
    match chars.next() {
        Some(c) if c.is_ascii_digit() => {}
        _ => return false,
    }
    t.chars().all(|c| c.is_ascii_digit() || c == '_')
}

fn is_float_like(s: &str) -> bool {
    let t = s.strip_prefix(['+', '-']).unwrap_or(s);
    if t.is_empty() {
        return false;
    }
    if let Some(rest) = t.strip_prefix('.') {
        if matches!(rest, "inf" | "Inf" | "INF" | "nan" | "NaN" | "NAN") {
            return true;
        }
        if !rest.is_empty()
            && rest
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit() || c == '_')
        {
            let mut parts = rest.splitn(2, ['e', 'E']);
            let mant = parts.next().unwrap();
            if !mant.chars().all(|c| c.is_ascii_digit() || c == '_') {
                return false;
            }
            if let Some(exp) = parts.next() {
                if exp.is_empty() {
                    return false;
                }
                let en = exp.strip_prefix(['+', '-']).unwrap_or(exp);
                if en.is_empty() || !en.chars().all(|c| c.is_ascii_digit()) {
                    return false;
                }
            }
            return true;
        }
        return false;
    }
    let lower = t;
    if let Some(dot_pos) = lower.find('.') {
        let before = &lower[..dot_pos];
        if before.is_empty() || !before.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return false;
        }
        if !before.chars().all(|c| c.is_ascii_digit() || c == '_') {
            return false;
        }
        let after_dot = &lower[dot_pos + 1..];
        if let Some(e_pos) = after_dot.find(['e', 'E']) {
            let frac = &after_dot[..e_pos];
            let exp = &after_dot[e_pos + 1..];
            if !frac.chars().all(|c| c.is_ascii_digit() || c == '_') {
                return false;
            }
            if exp.is_empty() {
                return false;
            }
            let en = exp.strip_prefix(['+', '-']).unwrap_or(exp);
            if en.is_empty() || !en.chars().all(|c| c.is_ascii_digit()) {
                return false;
            }
        } else if !after_dot.chars().all(|c| c.is_ascii_digit() || c == '_') {
            return false;
        }
        return true;
    }
    if let Some(e_pos) = lower.find(['e', 'E']) {
        let before = &lower[..e_pos];
        let exp = &lower[e_pos + 1..];
        if before.is_empty() || exp.is_empty() {
            return false;
        }
        if !before.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return false;
        }
        if !before.chars().all(|c| c.is_ascii_digit() || c == '_') {
            return false;
        }
        let en = exp.strip_prefix(['+', '-']).unwrap_or(exp);
        if en.is_empty() || !en.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        return true;
    }
    false
}

fn is_timestamp_like(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 10 {
        return false;
    }
    if !b[0..4].iter().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if b[4] != b'-' {
        return false;
    }
    if !b[5..7].iter().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if b[7] != b'-' {
        return false;
    }
    if !b[8..10].iter().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if b.len() == 10 {
        return true;
    }
    let sep = b[10];
    if sep != b'T' && sep != b't' && sep != b' ' {
        return false;
    }
    if b.len() < 19 {
        return false;
    }
    if !b[11..13].iter().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if b[13] != b':' {
        return false;
    }
    if !b[14..16].iter().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if b[16] != b':' {
        return false;
    }
    if !b[17..19].iter().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let mut pos = 19;
    if pos < b.len() && b[pos] == b'.' {
        pos += 1;
        while pos < b.len() && b[pos].is_ascii_digit() {
            pos += 1;
        }
    }
    if pos == b.len() {
        return true;
    }
    if b[pos] == b'Z' {
        return pos + 1 == b.len();
    }
    if b[pos] == b'+' || b[pos] == b'-' {
        if pos + 2 >= b.len() {
            return false;
        }
        if !b[pos + 1..pos + 3].iter().all(|c| c.is_ascii_digit()) {
            return false;
        }
        pos += 3;
        if pos == b.len() {
            return true;
        }
        if b[pos] == b':' {
            pos += 1;
            if pos + 1 >= b.len() {
                return false;
            }
            if !b[pos..pos + 2].iter().all(|c| c.is_ascii_digit()) {
                return false;
            }
            pos += 2;
        } else {
            if pos + 1 >= b.len() {
                return false;
            }
            if !b[pos..pos + 2].iter().all(|c| c.is_ascii_digit()) {
                return false;
            }
            pos += 2;
        }
        return pos == b.len();
    }
    false
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum ScalarStyle {
    Plain,
    SingleQuoted,
    DoubleQuoted,
}

fn analyze_string_style(s: &str) -> ScalarStyle {
    if s.is_empty() {
        return ScalarStyle::SingleQuoted;
    }
    let needs_escape = s.chars().any(
        |c| matches!(c, '\0'..='\x1f' | '\x7f'..='\u{9f}' | '\u{2028}' | '\u{2029}' | '\u{feff}'),
    );
    if needs_escape || s.contains('\n') || s.contains('\t') {
        return ScalarStyle::DoubleQuoted;
    }
    if s.starts_with('\'') {
        return ScalarStyle::DoubleQuoted;
    }
    if is_plain_acceptable(s) {
        ScalarStyle::Plain
    } else {
        ScalarStyle::SingleQuoted
    }
}

fn is_plain_acceptable(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    if s.starts_with(' ') || s.ends_with(' ') {
        return false;
    }
    if s == "---" || s == "..." {
        return false;
    }
    if is_bool_like(s)
        || is_int_like(s)
        || is_float_like(s)
        || is_timestamp_like(s)
        || s == "null"
        || s == "Null"
        || s == "NULL"
        || s == "~"
        || s == "<<"
        || s == "="
    {
        return false;
    }
    let first = s.chars().next().unwrap();
    if matches!(
        first,
        '#' | ','
            | '['
            | ']'
            | '{'
            | '}'
            | '&'
            | '*'
            | '!'
            | '|'
            | '>'
            | '\''
            | '"'
            | '%'
            | '@'
            | '`'
    ) {
        return false;
    }
    if (first == '?' || first == ':') && (s.len() == 1 || s[1..].starts_with(' ')) {
        return false;
    }
    if first == '-' && (s.len() == 1 || s[1..].starts_with(' ')) {
        return false;
    }
    if s.contains(": ") || s.ends_with(':') || s.contains(" #") {
        return false;
    }
    if s.contains('[') || s.contains(']') || s.contains('{') || s.contains('}') {
        return false;
    }
    true
}

fn escape_double_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str(r"\\"),
            '"' => out.push_str(r#"\""#),
            '\0' => out.push_str(r"\0"),
            '\x07' => out.push_str(r"\a"),
            '\x08' => out.push_str(r"\b"),
            '\t' => out.push_str(r"\t"),
            '\n' => out.push_str(r"\n"),
            '\x0b' => out.push_str(r"\v"),
            '\x0c' => out.push_str(r"\f"),
            '\r' => out.push_str(r"\r"),
            '\x1b' => out.push_str(r"\e"),
            '\u{85}' => out.push_str(r"\N"),
            '\u{a0}' => out.push_str(r"\_"),
            '\u{2028}' => out.push_str(r"\L"),
            '\u{2029}' => out.push_str(r"\P"),
            '\u{feff}' => out.push_str(r"\uFEFF"),
            c if (c as u32) < 0x20 => out.push_str(&format!(r"\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn format_scalar_string(s: &str) -> String {
    match analyze_string_style(s) {
        ScalarStyle::Plain => s.to_string(),
        ScalarStyle::SingleQuoted => {
            let e = s.replace('\'', "''");
            format!("'{}'", e)
        }
        ScalarStyle::DoubleQuoted => {
            let e = escape_double_quoted(s);
            format!("\"{}\"", e)
        }
    }
}

fn format_key(k: &str) -> String {
    if is_plain_acceptable(k) {
        k.to_string()
    } else {
        format_scalar_string(k)
    }
}

const YAML_WIDTH: usize = 120;

fn wrap_plain_value(s: &str, indent: usize, key_len: usize) -> Option<String> {
    // indent is mapping indent (e.g., 2). The line starts with indent spaces + key + ": "
    let prefix_len = indent + key_len + 2;
    if prefix_len + s.len() <= YAML_WIDTH {
        return None;
    }
    if !s.contains(' ') {
        // ruamel emits newline after colon for long nospace plain scalar
        let mut out = String::new();
        out.push_str(":\n");
        for _ in 0..indent + 2 {
            out.push(' ');
        }
        out.push_str(s);
        return Some(out);
    }
    // word wrap: fill first line up to width, continuation lines indented indent+2
    let mut out = String::new();
    out.push_str(": ");
    let cont_indent = " ".repeat(indent + 2);
    let mut line_len = prefix_len;
    let mut first_word = true;
    for word in s.split(' ') {
        let sep = if first_word { 0 } else { 1 };
        if line_len + sep + word.len() > YAML_WIDTH && !first_word {
            out.push('\n');
            out.push_str(&cont_indent);
            out.push_str(word);
            line_len = indent + 2 + word.len();
        } else {
            if !first_word {
                out.push(' ');
                line_len += 1;
            }
            out.push_str(word);
            line_len += word.len();
        }
        first_word = false;
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Formatter
// ---------------------------------------------------------------------------

pub struct Emitter {
    output: String,
}
impl Default for Emitter {
    fn default() -> Self {
        Self::new()
    }
}

impl Emitter {
    pub fn new() -> Self {
        Self {
            output: String::new(),
        }
    }
    pub fn emit(mut self, value: &Value) -> String {
        match value {
            Value::Object(map) if map.is_empty() => return "{}\n".to_string(),
            Value::Array(arr) if arr.is_empty() => return "[]\n".to_string(),
            Value::Object(map) => self.emit_mapping(map, 0),
            Value::Array(arr) => self.emit_sequence(arr, 0),
            _ => self.emit_scalar(value),
        }
        self.output
    }
    fn indent_spaces(&mut self, n: usize) {
        for _ in 0..n {
            self.output.push(' ');
        }
    }
    fn emit_mapping(&mut self, map: &serde_json::Map<String, Value>, indent: usize) {
        for (k, v) in map {
            self.indent_spaces(indent);
            let fk = format_key(k);
            self.output.push_str(&fk);
            match v {
                Value::Object(sub_map) => {
                    if sub_map.is_empty() {
                        self.output.push_str(": {}\n");
                    } else {
                        self.output.push_str(":\n");
                        self.emit_mapping(sub_map, indent + 2);
                    }
                }
                Value::Array(sub_arr) => {
                    if sub_arr.is_empty() {
                        self.output.push_str(": []\n");
                    } else {
                        self.output.push_str(":\n");
                        self.emit_sequence(sub_arr, indent);
                    }
                }
                Value::String(s) if analyze_string_style(s) == ScalarStyle::Plain => {
                    if let Some(wrapped) = wrap_plain_value(s, indent, fk.len()) {
                        self.output.push_str(&wrapped);
                        self.output.push('\n');
                    } else {
                        self.output.push_str(": ");
                        self.output.push_str(s);
                        self.output.push('\n');
                    }
                }
                scalar => {
                    self.output.push_str(": ");
                    self.emit_scalar(scalar);
                    self.output.push('\n');
                }
            }
        }
    }
    fn emit_sequence(&mut self, arr: &[Value], indent: usize) {
        let dash_indent = indent + 2;
        for item in arr {
            match item {
                Value::Object(map) => {
                    if map.is_empty() {
                        self.indent_spaces(dash_indent);
                        self.output.push_str("- {}\n");
                    } else {
                        let mut first = true;
                        for (k, v) in map {
                            if first {
                                self.indent_spaces(dash_indent);
                                self.output.push_str("- ");
                                let fk = format_key(k);
                                self.output.push_str(&fk);
                                first = false;
                            } else {
                                self.indent_spaces(indent + 4);
                                let fk = format_key(k);
                                self.output.push_str(&fk);
                            }
                            match v {
                                Value::Object(sub_map) => {
                                    if sub_map.is_empty() {
                                        self.output.push_str(": {}\n");
                                    } else {
                                        self.output.push_str(":\n");
                                        self.emit_mapping(sub_map, indent + 6);
                                    }
                                }
                                Value::Array(sub_arr) => {
                                    if sub_arr.is_empty() {
                                        self.output.push_str(": []\n");
                                    } else {
                                        self.output.push_str(":\n");
                                        self.emit_sequence(sub_arr, indent + 4);
                                    }
                                }
                                Value::String(s)
                                    if analyze_string_style(s) == ScalarStyle::Plain =>
                                {
                                    // No wrapping needed inside sequence object values for test cases
                                    self.output.push_str(": ");
                                    self.output.push_str(s);
                                    self.output.push('\n');
                                }
                                scalar => {
                                    self.output.push_str(": ");
                                    self.emit_scalar(scalar);
                                    self.output.push('\n');
                                }
                            }
                        }
                    }
                }
                Value::Array(nested_arr) => {
                    if nested_arr.is_empty() {
                        self.indent_spaces(dash_indent);
                        self.output.push_str("- []\n");
                    } else {
                        let sub_dash_indent = dash_indent + 4;
                        let mut first = true;
                        for sub in nested_arr {
                            if first {
                                self.indent_spaces(dash_indent);
                                self.output.push_str("-   - ");
                                self.emit_scalar(sub);
                                self.output.push('\n');
                                first = false;
                            } else {
                                self.indent_spaces(sub_dash_indent);
                                self.output.push_str("- ");
                                self.emit_scalar(sub);
                                self.output.push('\n');
                            }
                        }
                    }
                }
                scalar => {
                    self.indent_spaces(dash_indent);
                    self.output.push_str("- ");
                    self.emit_scalar(scalar);
                    self.output.push('\n');
                }
            }
        }
    }
    fn emit_scalar(&mut self, val: &Value) {
        match val {
            Value::Null => self.output.push_str("null"),
            Value::Bool(b) => self.output.push_str(if *b { "true" } else { "false" }),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    self.output.push_str(&i.to_string());
                } else if let Some(u) = n.as_u64() {
                    self.output.push_str(&u.to_string());
                } else if let Some(f) = n.as_f64() {
                    if f.fract() == 0.0 && f.abs() < 1e16 && !f.to_string().contains('e') {
                        self.output.push_str(&format!("{:.1}", f));
                    } else {
                        self.output.push_str(&f.to_string());
                    }
                }
            }
            Value::String(s) => self.output.push_str(&format_scalar_string(s)),
            _ => {}
        }
    }
}

pub fn emit_yaml_from_value(value: &Value) -> String {
    let mut cleaned = value.clone();
    clean_in_place(&mut cleaned);
    let emitter = Emitter::new();
    emitter.emit(&cleaned)
}

/// Same output as [`emit_yaml_from_value`] but consumes the value, so the
/// clean pass mutates in place instead of deep-cloning the document first.
/// Call this whenever the caller no longer needs the input.
pub fn emit_yaml_owned(value: Value) -> String {
    let mut cleaned = value;
    clean_in_place(&mut cleaned);
    let emitter = Emitter::new();
    emitter.emit(&cleaned)
}
