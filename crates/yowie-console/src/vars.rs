//! Typed variables the console reads and writes.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

/// A float the console can change and the game can read, shared between
/// them. Cloning gives another handle to the same value.
#[derive(Clone, Debug, Default)]
pub struct FloatVar(Arc<AtomicU32>);

impl FloatVar {
    fn new(v: f32) -> Self {
        Self(Arc::new(AtomicU32::new(v.to_bits())))
    }

    /// The current value.
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    /// Sets the value.
    pub fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed);
    }
}

/// A whole number the console can change and the game can read.
#[derive(Clone, Debug, Default)]
pub struct IntVar(Arc<AtomicI64>);

impl IntVar {
    fn new(v: i64) -> Self {
        Self(Arc::new(AtomicI64::new(v)))
    }

    /// The current value.
    pub fn get(&self) -> i64 {
        self.0.load(Ordering::Relaxed)
    }

    /// Sets the value.
    pub fn set(&self, v: i64) {
        self.0.store(v, Ordering::Relaxed);
    }
}

/// A switch the console can change and the game can read.
#[derive(Clone, Debug, Default)]
pub struct BoolVar(Arc<AtomicBool>);

impl BoolVar {
    fn new(v: bool) -> Self {
        Self(Arc::new(AtomicBool::new(v)))
    }

    /// The current value.
    pub fn get(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// Sets the value.
    pub fn set(&self, v: bool) {
        self.0.store(v, Ordering::Relaxed);
    }
}

/// A string the console can change and the game can read.
#[derive(Clone, Debug, Default)]
pub struct StringVar(Arc<Mutex<String>>);

impl StringVar {
    fn new(v: &str) -> Self {
        Self(Arc::new(Mutex::new(v.to_owned())))
    }

    /// The current value.
    pub fn get(&self) -> String {
        self.0.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Sets the value.
    pub fn set(&self, v: &str) {
        if let Ok(mut g) = self.0.lock() {
            *g = v.to_owned();
        }
    }
}

/// A registered variable's storage.
#[derive(Clone, Debug)]
pub(crate) enum Value {
    Float(FloatVar),
    Int(IntVar),
    Bool(BoolVar),
    Text(StringVar),
}

/// A variable with its name and description.
#[derive(Clone, Debug)]
pub(crate) struct Var {
    pub(crate) help: String,
    pub(crate) value: Value,
}

impl Var {
    pub(crate) fn float(default: f32, help: &str) -> (Var, FloatVar) {
        let h = FloatVar::new(default);
        (
            Var {
                help: help.into(),
                value: Value::Float(h.clone()),
            },
            h,
        )
    }

    pub(crate) fn int(default: i64, help: &str) -> (Var, IntVar) {
        let h = IntVar::new(default);
        (
            Var {
                help: help.into(),
                value: Value::Int(h.clone()),
            },
            h,
        )
    }

    pub(crate) fn boolean(default: bool, help: &str) -> (Var, BoolVar) {
        let h = BoolVar::new(default);
        (
            Var {
                help: help.into(),
                value: Value::Bool(h.clone()),
            },
            h,
        )
    }

    pub(crate) fn text(default: &str, help: &str) -> (Var, StringVar) {
        let h = StringVar::new(default);
        (
            Var {
                help: help.into(),
                value: Value::Text(h.clone()),
            },
            h,
        )
    }

    /// The type's name as shown in listings.
    pub(crate) fn kind(&self) -> &'static str {
        match self.value {
            Value::Float(_) => "float",
            Value::Int(_) => "int",
            Value::Bool(_) => "bool",
            Value::Text(_) => "string",
        }
    }

    /// The value as text.
    pub(crate) fn show(&self) -> String {
        match &self.value {
            Value::Float(v) => format!("{}", v.get()),
            Value::Int(v) => v.get().to_string(),
            Value::Bool(v) => v.get().to_string(),
            Value::Text(v) => v.get(),
        }
    }

    /// Parses `text` as this variable's type and stores it.
    pub(crate) fn assign(&self, text: &str) -> Result<(), String> {
        match &self.value {
            Value::Float(v) => {
                let x: f32 = text
                    .parse()
                    .map_err(|_| format!("{text:?} is not a number"))?;
                if !x.is_finite() {
                    return Err(format!("{text:?} is not a finite number"));
                }
                v.set(x);
            }
            Value::Int(v) => {
                v.set(
                    text.parse()
                        .map_err(|_| format!("{text:?} is not a whole number"))?,
                );
            }
            Value::Bool(v) => v.set(parse_bool(text)?),
            Value::Text(v) => v.set(text),
        }
        Ok(())
    }
}

fn parse_bool(text: &str) -> Result<bool, String> {
    match text.to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "yes" => Ok(true),
        "0" | "false" | "off" | "no" => Ok(false),
        _ => Err(format!("{text:?} is not true or false")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variables_parse_by_type() {
        let (f, fh) = Var::float(1.0, "");
        f.assign("2.5").expect("float");
        assert_eq!(fh.get(), 2.5);
        assert!(f.assign("fast").is_err());
        assert!(f.assign("inf").is_err());
        let (i, ih) = Var::int(0, "");
        i.assign("-7").expect("int");
        assert_eq!(ih.get(), -7);
        assert!(i.assign("1.5").is_err());
        let (b, bh) = Var::boolean(false, "");
        for (text, want) in [("on", true), ("FALSE", false), ("1", true), ("no", false)] {
            b.assign(text).expect(text);
            assert_eq!(bh.get(), want, "{text}");
        }
        assert!(b.assign("maybe").is_err());
        let (s, sh) = Var::text("a", "");
        s.assign("hello world").expect("string");
        assert_eq!(sh.get(), "hello world");
        assert_eq!(s.show(), "hello world");
        assert_eq!(s.kind(), "string");
    }
}
