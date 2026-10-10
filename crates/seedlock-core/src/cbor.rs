//! A small, strict CBOR (RFC 8949) reader and writer: just what the UR formats for keys and wallet
//! descriptors need (unsigned integers, byte and text strings, arrays, maps, tags, true/false/null).
//! Anything else (negative numbers, floats, indefinite lengths) is refused, as is nesting deeper
//! than a fixed limit, so a hostile QR code cannot make the reader run away.

use crate::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Uint(u64),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<Value>),
    /// Keys and values in the order written.
    Map(Vec<(Value, Value)>),
    Tag(u64, Box<Value>),
    Bool(bool),
    Null,
}

const MAX_DEPTH: usize = 16;

fn err(text: &str) -> Error {
    Error::Multisig(format!("the QR data is not valid CBOR: {text}"))
}

impl Value {
    pub fn tag(tag: u64, value: Value) -> Value {
        Value::Tag(tag, Box::new(value))
    }

    /// The value under an unsigned-integer key of a map.
    pub fn get(&self, key: u64) -> Option<&Value> {
        match self {
            Value::Map(entries) => entries.iter().find(|(k, _)| *k == Value::Uint(key)).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_uint(&self) -> Option<u64> {
        match self {
            Value::Uint(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Bytes(b) => Some(b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }
}

fn head(out: &mut Vec<u8>, major: u8, n: u64) {
    let m = major << 5;
    if n < 24 {
        out.push(m | n as u8);
    } else if n <= 0xff {
        out.extend([m | 24, n as u8]);
    } else if n <= 0xffff {
        out.push(m | 25);
        out.extend((n as u16).to_be_bytes());
    } else if n <= 0xffff_ffff {
        out.push(m | 26);
        out.extend((n as u32).to_be_bytes());
    } else {
        out.push(m | 27);
        out.extend(n.to_be_bytes());
    }
}

/// The shortest encoding of `value`.
pub fn encode(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    write(&mut out, value);
    out
}

fn write(out: &mut Vec<u8>, value: &Value) {
    match value {
        Value::Uint(n) => head(out, 0, *n),
        Value::Bytes(b) => {
            head(out, 2, b.len() as u64);
            out.extend(b);
        }
        Value::Text(t) => {
            head(out, 3, t.len() as u64);
            out.extend(t.as_bytes());
        }
        Value::Array(items) => {
            head(out, 4, items.len() as u64);
            for item in items {
                write(out, item);
            }
        }
        Value::Map(entries) => {
            head(out, 5, entries.len() as u64);
            for (k, v) in entries {
                write(out, k);
                write(out, v);
            }
        }
        Value::Tag(tag, inner) => {
            head(out, 6, *tag);
            write(out, inner);
        }
        Value::Bool(false) => out.push(0xf4),
        Value::Bool(true) => out.push(0xf5),
        Value::Null => out.push(0xf6),
    }
}

/// Read exactly one value; there must be nothing after it.
pub fn decode(data: &[u8]) -> Result<Value> {
    let mut pos = 0;
    let value = read(data, &mut pos, 0)?;
    if pos != data.len() {
        return Err(err("there is data after its end"));
    }
    Ok(value)
}

fn take<'a>(data: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8]> {
    let end = pos.checked_add(n).filter(|end| *end <= data.len()).ok_or_else(|| err("it ends too early"))?;
    let bytes = &data[*pos..end];
    *pos = end;
    Ok(bytes)
}

fn read(data: &[u8], pos: &mut usize, depth: usize) -> Result<Value> {
    if depth > MAX_DEPTH {
        return Err(err("it is nested too deeply"));
    }
    let first = take(data, pos, 1)?[0];
    let (major, info) = (first >> 5, first & 0x1f);
    if major == 7 {
        return match info {
            20 => Ok(Value::Bool(false)),
            21 => Ok(Value::Bool(true)),
            22 => Ok(Value::Null),
            _ => Err(err("it holds a kind of value that is not used here")),
        };
    }
    let n = match info {
        0..=23 => u64::from(info),
        24 => u64::from(take(data, pos, 1)?[0]),
        25 => u64::from(u16::from_be_bytes(take(data, pos, 2)?.try_into().unwrap())),
        26 => u64::from(u32::from_be_bytes(take(data, pos, 4)?.try_into().unwrap())),
        27 => u64::from_be_bytes(take(data, pos, 8)?.try_into().unwrap()),
        _ => return Err(err("it uses an indefinite length")),
    };
    let len = |n: u64| usize::try_from(n).map_err(|_| err("a length is too large"));
    match major {
        0 => Ok(Value::Uint(n)),
        2 => Ok(Value::Bytes(take(data, pos, len(n)?)?.to_vec())),
        3 => String::from_utf8(take(data, pos, len(n)?)?.to_vec()).map(Value::Text).map_err(|_| err("a text is not UTF-8")),
        4 => {
            let count = len(n)?;
            if count > data.len() {
                return Err(err("an array is longer than the data"));
            }
            (0..count).map(|_| read(data, pos, depth + 1)).collect::<Result<Vec<_>>>().map(Value::Array)
        }
        5 => {
            let count = len(n)?;
            if count > data.len() {
                return Err(err("a map is longer than the data"));
            }
            let mut entries = Vec::with_capacity(count);
            for _ in 0..count {
                let key = read(data, pos, depth + 1)?;
                let value = read(data, pos, depth + 1)?;
                entries.push((key, value));
            }
            Ok(Value::Map(entries))
        }
        6 => Ok(Value::tag(n, read(data, pos, depth + 1)?)),
        _ => Err(err("it holds a negative number")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_reads_the_values_used_here() {
        let value = Value::Map(vec![
            (Value::Uint(1), Value::Uint(0x73c5_da0a)),
            (Value::Uint(2), Value::Array(vec![Value::tag(401, Value::tag(303, Value::Map(vec![(Value::Uint(3), Value::Bytes(vec![2; 33]))])))])),
            (Value::Uint(3), Value::Array(vec![Value::Uint(48), Value::Bool(true), Value::Array(vec![]), Value::Bool(false)])),
            (Value::Uint(4), Value::Text("vault".into())),
            (Value::Uint(5), Value::Null),
        ]);
        let bytes = encode(&value);
        assert_eq!(decode(&bytes).unwrap(), value);
        // RFC 8949 examples
        assert_eq!(encode(&Value::Uint(500)), [0x19, 0x01, 0xf4]);
        assert_eq!(encode(&Value::Uint(1_000_000)), [0x1a, 0x00, 0x0f, 0x42, 0x40]);
        assert_eq!(encode(&Value::tag(303, Value::Null)), [0xd9, 0x01, 0x2f, 0xf6]);
        assert_eq!(value.get(4), Some(&Value::Text("vault".into())));
    }

    #[test]
    fn refuses_what_it_does_not_expect() {
        assert!(decode(&[0x20]).is_err(), "a negative number");
        assert!(decode(&[0xfb, 0, 0, 0, 0, 0, 0, 0, 0]).is_err(), "a float");
        assert!(decode(&[0x9f, 0xff]).is_err(), "an indefinite length");
        assert!(decode(&[0x43, 1, 2]).is_err(), "cut short");
        assert!(decode(&[0x01, 0x02]).is_err(), "data after the end");
        assert!(decode(&[0x9b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]).is_err(), "a huge array");
        let deep: Vec<u8> = std::iter::repeat(0x81).take(40).chain([0x00]).collect();
        assert!(decode(&deep).is_err(), "nested too deeply");
    }
}
