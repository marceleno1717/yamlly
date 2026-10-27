use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Key(String),
    Index(usize),
}

fn is_all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn parse_bracket(s: &str) -> Option<(&str, usize)> {
    if !s.ends_with(']') {
        return None;
    }
    let open = s.rfind('[')?;
    let idx_str = &s[open + 1..s.len() - 1];
    if idx_str.is_empty() || !is_all_digits(idx_str) {
        return None;
    }
    let idx: usize = idx_str.parse().ok()?;
    Some((&s[..open], idx))
}

/// Convert path segments that contain `[N]` suffixes to (key, int) pairs.
pub fn expand_path<S: AsRef<str>>(path: &[S]) -> Vec<Segment> {
    let mut result = Vec::new();
    for seg in path {
        let s = seg.as_ref();
        if let Some((name, idx)) = parse_bracket(s) {
            if !name.is_empty() {
                result.push(Segment::Key(name.to_string()));
            }
            result.push(Segment::Index(idx));
        } else if is_all_digits(s) {
            if let Ok(idx) = s.parse::<usize>() {
                result.push(Segment::Index(idx));
            }
        } else {
            result.push(Segment::Key(s.to_string()));
        }
    }
    result
}

/// Set `value` at `path` inside a nested Value (dict/list structure).
pub fn set_nested<S: AsRef<str>>(root: &mut Value, path: &[S], value: Value) {
    if path.is_empty() {
        return;
    }
    let expanded = expand_path(path);
    _set(root, &expanded, value);
}

fn _ensure_container(parent: &mut Value, key: &Segment, next_key: &Segment) {
    let need_list = matches!(next_key, Segment::Index(_));
    match parent {
        Value::Array(arr) => {
            if let Segment::Index(idx) = key {
                let idx = *idx;
                if let Some(item) = arr.get_mut(idx) {
                    if need_list && !item.is_array() {
                        *item = Value::Array(Vec::new());
                    } else if !need_list && !item.is_object() {
                        *item = Value::Object(serde_json::Map::new());
                    }
                }
            }
        }
        Value::Object(map) => {
            if let Segment::Key(k) = key {
                // Only clone the key when we actually have to insert.
                if need_list {
                    if !map.get(k).is_some_and(Value::is_array) {
                        map.insert(k.clone(), Value::Array(Vec::new()));
                    }
                } else if !map.get(k).is_some_and(Value::is_object) {
                    map.insert(k.clone(), Value::Object(serde_json::Map::new()));
                }
            }
        }
        _ => {}
    }
}

fn _set(root: &mut Value, path: &[Segment], value: Value) {
    let Some((key, rest)) = path.split_first() else {
        return;
    };

    if let Some(arr) = root.as_array_mut() {
        let Segment::Index(idx) = key else { return };
        let idx = *idx;
        while arr.len() <= idx {
            arr.push(Value::Object(serde_json::Map::new()));
        }
        if rest.is_empty() {
            arr[idx] = value;
        } else {
            _ensure_container(root, key, &rest[0]);
            _set(
                &mut root.as_array_mut().expect("still an array")[idx],
                rest,
                value,
            );
        }
        return;
    }

    if !root.is_object() {
        *root = Value::Object(serde_json::Map::new());
    }
    // Borrow the key instead of cloning it at every level of the walk.
    let borrowed_key;
    let k: &str = match key {
        Segment::Key(s) => s.as_str(),
        Segment::Index(i) => {
            borrowed_key = i.to_string();
            borrowed_key.as_str()
        }
    };

    if rest.is_empty() {
        let drop_it = value.is_null() || matches!(&value, Value::String(s) if s.is_empty());
        let map = root.as_object_mut().expect("now an object");
        if drop_it {
            map.remove(k);
        } else {
            map.insert(k.to_string(), value);
        }
    } else {
        _ensure_container(root, key, &rest[0]);
        let child = root
            .as_object_mut()
            .expect("now an object")
            .get_mut(k)
            .expect("child exists");
        _set(child, rest, value);
    }
}

/// Return the value at `path`, or `None` if not found.
pub fn get_nested<'a, S: AsRef<str>>(root: &'a Value, path: &[S]) -> Option<&'a Value> {
    let expanded = expand_path(path);
    let mut node = root;
    for seg in &expanded {
        match seg {
            Segment::Key(k) => match node {
                Value::Object(map) => {
                    node = map.get(k)?;
                }
                _ => return None,
            },
            Segment::Index(idx) => match node {
                Value::Array(arr) => {
                    node = arr.get(*idx)?;
                }
                _ => return None,
            },
        }
    }
    Some(node)
}

/// Encode a dotted field path to a safe widget ID (no regex).
pub fn encode_path(dotted: &str) -> String {
    let mut tmp = String::with_capacity(dotted.len() + 8);
    let bytes = dotted.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            if let Some(end) = dotted[i..].find(']') {
                let inner = &dotted[i + 1..i + end];
                tmp.push_str("-ARR");
                tmp.push_str(inner);
                i += end + 1;
                continue;
            }
        }
        tmp.push(bytes[i] as char);
        i += 1;
    }
    let mut out = String::with_capacity(tmp.len() + 8);
    for ch in tmp.chars() {
        if ch == '.' {
            out.push_str("__");
        } else {
            out.push(ch);
        }
    }
    format!("sf_{}", out)
}

/// Decode widget ID back to path segments (no regex).
pub fn decode_path(widget_id: &str) -> Option<Vec<String>> {
    if !widget_id.starts_with("sf_") {
        return None;
    }
    let stripped = &widget_id[3..];
    let dotted = stripped.replace("__", ".");
    let mut out = String::with_capacity(dotted.len() + 8);
    let b = dotted.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i..].starts_with(b"-ARR") {
            let start = i + 4;
            let mut end = start;
            while end < b.len() && b[end].is_ascii_digit() {
                end += 1;
            }
            let digits = &dotted[start..end];
            out.push('[');
            out.push_str(digits);
            out.push(']');
            i = end;
        } else {
            out.push(b[i] as char);
            i += 1;
        }
    }
    Some(out.split('.').map(|s| s.to_string()).collect())
}

/// Canonical form used for unsaved-change detection.
pub fn normalize(value: &Value) -> Option<Value> {
    match value {
        Value::Null => None,
        Value::Bool(b) => Some(Value::String(if *b {
            "True".to_string()
        } else {
            "False".to_string()
        })),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(Value::String(i.to_string()))
            } else if let Some(u) = n.as_u64() {
                Some(Value::String(u.to_string()))
            } else if let Some(f) = n.as_f64() {
                Some(Value::String(format_python_float(f)))
            } else {
                Some(Value::String(n.to_string()))
            }
        }
        Value::String(s) => {
            if s.is_empty() {
                None
            } else {
                Some(Value::String(s.clone()))
            }
        }
        Value::Array(arr) => {
            let mut out = Vec::new();
            for item in arr {
                if let Some(norm) = normalize(item) {
                    out.push(norm);
                }
            }
            if out.is_empty() {
                None
            } else {
                Some(Value::Array(out))
            }
        }
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                if let Some(norm) = normalize(v) {
                    out.insert(k.clone(), norm);
                }
            }
            if out.is_empty() {
                None
            } else {
                Some(Value::Object(out))
            }
        }
    }
}

fn format_python_float(f: f64) -> String {
    if f.is_nan() {
        return "nan".to_string();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    let abs = f.abs();
    if abs == 0.0 {
        return if f.is_sign_negative() {
            "-0.0".to_string()
        } else {
            "0.0".to_string()
        };
    }
    if !(1e-4..1e16).contains(&abs) {
        let s = format!("{:e}", f);
        if let Some((mantissa, exp)) = s.split_once('e') {
            let exp_num: i32 = exp.parse().unwrap_or(0);
            let exp_str = if exp_num >= 0 {
                format!("+{:02}", exp_num)
            } else {
                format!("-{:02}", exp_num.abs())
            };
            return format!("{}e{}", mantissa, exp_str);
        }
    }
    if f.fract() == 0.0 && abs < 1e16 {
        format!("{:.1}", f)
    } else {
        f.to_string()
    }
}

pub fn is_dirty(current: &Value, baseline: &Value) -> bool {
    !norm_eq(current, baseline)
}

/// `true` when `normalize` would collapse `v` to `None` (empty / null).
fn norm_absent(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Bool(_) | Value::Number(_) => false,
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.iter().all(norm_absent),
        Value::Object(m) => m.values().all(norm_absent),
    }
}

/// Allocation-free equivalent of `normalize(current) == baseline`.
///
/// `baseline` must already be normalized. This walks both trees in lockstep
/// and never materializes the normalized form, so dirty-tracking on every
/// keystroke costs no allocations instead of rebuilding the whole document.
fn norm_eq(cur: &Value, base: &Value) -> bool {
    // Whatever `normalize` drops becomes the canonical empty object.
    if norm_absent(cur) {
        return base.as_object().is_some_and(|m| m.is_empty());
    }
    match cur {
        Value::Bool(b) => base.as_str() == Some(if *b { "True" } else { "False" }),
        Value::Number(_) => base.as_str() == Some(number_to_string(cur).as_str()),
        Value::String(s) => base.as_str() == Some(s.as_str()),
        Value::Array(a) => {
            let Some(b) = base.as_array() else {
                return false;
            };
            // normalize() drops absent items, so skip them on the left too.
            let mut bi = b.iter();
            for item in a {
                if norm_absent(item) {
                    continue;
                }
                match bi.next() {
                    Some(x) if norm_eq(item, x) => {}
                    _ => return false,
                }
            }
            bi.next().is_none()
        }
        Value::Object(m) => {
            let Some(b) = base.as_object() else {
                return false;
            };
            let mut kept = 0usize;
            for (k, v) in m {
                if norm_absent(v) {
                    continue;
                }
                match b.get(k) {
                    Some(x) if norm_eq(v, x) => kept += 1,
                    _ => return false,
                }
            }
            kept == b.len()
        }
        Value::Null => base.as_object().is_some_and(|m| m.is_empty()),
    }
}

/// String form `normalize` would produce for a number.
fn number_to_string(n: &Value) -> String {
    if let Some(i) = n.as_i64() {
        i.to_string()
    } else if let Some(u) = n.as_u64() {
        u.to_string()
    } else if let Some(f) = n.as_f64() {
        format_python_float(f)
    } else {
        n.to_string()
    }
}
