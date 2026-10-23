//! Minimal NBT reader for chunk data.
//!
//! A chunk holds a lot of data the map does not need (heightmaps, entities, structures, ...).
//! Instead of building the whole tree, the parser asks a `keep(depth, key)` filter for every
//! key and skips what is not wanted without allocating.

#[derive(Debug, Clone)]
pub enum Nbt {
    /// Byte, short, int and long are all stored as i64.
    Num(i64),
    Str(String),
    List(Vec<Nbt>),
    Compound(Vec<(String, Nbt)>),
    Longs(Vec<i64>),
    /// Anything that was parsed but is of no interest (floats, byte arrays, ...).
    Other,
}

impl Nbt {
    pub fn get(&self, key: &str) -> Option<&Nbt> {
        match self {
            Nbt::Compound(v) => v.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Nbt::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Nbt::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_list(&self) -> Option<&[Nbt]> {
        match self {
            Nbt::List(v) => Some(v),
            _ => None,
        }
    }
    pub fn as_longs(&self) -> Option<&[i64]> {
        match self {
            Nbt::Longs(v) => Some(v),
            _ => None,
        }
    }
}

/// `keep(depth, key)`: `depth` is the nesting level of the compound that holds `key`
/// (the root compound is 0; compounds inside lists count like compounds inside compounds).
pub type Keep<'a> = &'a dyn Fn(usize, &str) -> bool;

struct Cur<'a> {
    d: &'a [u8],
    p: usize,
}

type R<T> = Result<T, String>;

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize) -> R<&'a [u8]> {
        let d: &'a [u8] = self.d;
        let end = self.p.checked_add(n).filter(|&e| e <= d.len()).ok_or_else(|| "unexpected end of NBT data".to_string())?;
        let s = &d[self.p..end];
        self.p = end;
        Ok(s)
    }
    fn u8(&mut self) -> R<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> R<u16> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn i16(&mut self) -> R<i16> {
        Ok(self.u16()? as i16)
    }
    fn i32(&mut self) -> R<i32> {
        let b = self.take(4)?;
        Ok(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i64(&mut self) -> R<i64> {
        let b = self.take(8)?;
        Ok(i64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
    }
    fn len(&mut self) -> R<usize> {
        let n = self.i32()?;
        if n < 0 {
            return Err("negative length in NBT data".into());
        }
        Ok(n as usize)
    }
    fn string(&mut self) -> R<String> {
        let n = self.u16()? as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
}

const MAX_DEPTH: usize = 48;

fn skip(c: &mut Cur, tag: u8, depth: usize) -> R<()> {
    if depth > MAX_DEPTH {
        return Err("NBT nested too deeply".into());
    }
    match tag {
        1 => {
            c.take(1)?;
        }
        2 => {
            c.take(2)?;
        }
        3 | 5 => {
            c.take(4)?;
        }
        4 | 6 => {
            c.take(8)?;
        }
        7 => {
            let n = c.len()?;
            c.take(n)?;
        }
        8 => {
            let n = c.u16()? as usize;
            c.take(n)?;
        }
        9 => {
            let t = c.u8()?;
            let n = c.len()?;
            match t {
                0 => {}
                1 => {
                    c.take(n)?;
                }
                2 => {
                    c.take(n.checked_mul(2).ok_or("NBT list too long")?)?;
                }
                3 | 5 => {
                    c.take(n.checked_mul(4).ok_or("NBT list too long")?)?;
                }
                4 | 6 => {
                    c.take(n.checked_mul(8).ok_or("NBT list too long")?)?;
                }
                _ => {
                    for _ in 0..n {
                        skip(c, t, depth + 1)?;
                    }
                }
            }
        }
        10 => loop {
            let t = c.u8()?;
            if t == 0 {
                break;
            }
            let n = c.u16()? as usize;
            c.take(n)?;
            skip(c, t, depth + 1)?;
        },
        11 => {
            let n = c.len()?;
            c.take(n.checked_mul(4).ok_or("NBT array too long")?)?;
        }
        12 => {
            let n = c.len()?;
            c.take(n.checked_mul(8).ok_or("NBT array too long")?)?;
        }
        t => return Err(format!("unknown NBT tag {t}")),
    }
    Ok(())
}

fn compound(c: &mut Cur, depth: usize, keep: Keep) -> R<Nbt> {
    if depth > MAX_DEPTH {
        return Err("NBT nested too deeply".into());
    }
    let mut out = Vec::new();
    loop {
        let tag = c.u8()?;
        if tag == 0 {
            break;
        }
        let n = c.u16()? as usize;
        let raw = c.take(n)?;
        let key = String::from_utf8_lossy(raw);
        if keep(depth, &key) {
            let v = payload(c, tag, depth, keep)?;
            out.push((key.into_owned(), v));
        } else {
            skip(c, tag, depth + 1)?;
        }
    }
    Ok(Nbt::Compound(out))
}

/// `depth` is the depth of the compound that contains this value.
fn payload(c: &mut Cur, tag: u8, depth: usize, keep: Keep) -> R<Nbt> {
    Ok(match tag {
        1 => Nbt::Num(c.u8()? as i8 as i64),
        2 => Nbt::Num(c.i16()? as i64),
        3 => Nbt::Num(c.i32()? as i64),
        4 => Nbt::Num(c.i64()?),
        8 => Nbt::Str(c.string()?),
        9 => {
            let t = c.u8()?;
            let n = c.len()?;
            if t == 0 || n == 0 {
                return Ok(Nbt::List(Vec::new()));
            }
            let mut v = Vec::with_capacity(n.min(4096));
            for _ in 0..n {
                v.push(payload(c, t, depth, keep)?);
            }
            Nbt::List(v)
        }
        10 => compound(c, depth + 1, keep)?,
        12 => {
            let n = c.len()?;
            let raw = c.take(n.checked_mul(8).ok_or("NBT array too long")?)?;
            Nbt::Longs(
                raw.chunks_exact(8)
                    .map(|b| i64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
                    .collect(),
            )
        }
        other => {
            skip(c, other, depth + 1)?;
            Nbt::Other
        }
    })
}

/// Parse a whole NBT document (a named root compound), keeping only what `keep` accepts.
pub fn parse_root(data: &[u8], keep: Keep) -> Result<Nbt, String> {
    let mut c = Cur { d: data, p: 0 };
    let tag = c.u8()?;
    if tag != 10 {
        return Err("chunk data does not start with a compound tag".into());
    }
    let _name = c.string()?;
    compound(&mut c, 0, keep)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(buf: &mut Vec<u8>, text: &str) {
        buf.extend((text.len() as u16).to_be_bytes());
        buf.extend(text.as_bytes());
    }

    #[test]
    fn keeps_wanted_skips_rest() {
        let mut b = vec![10];
        s(&mut b, "");
        // junk: float, byte array, list of compounds, compound
        b.push(5);
        s(&mut b, "f");
        b.extend(1.5f32.to_be_bytes());
        b.push(7);
        s(&mut b, "ba");
        b.extend(3i32.to_be_bytes());
        b.extend([1, 2, 3]);
        b.push(9);
        s(&mut b, "junklist");
        b.push(10);
        b.extend(1i32.to_be_bytes());
        b.push(1);
        s(&mut b, "x");
        b.push(7);
        b.push(0);
        // wanted: int and long array
        b.push(3);
        s(&mut b, "want");
        b.extend((-5i32).to_be_bytes());
        b.push(12);
        s(&mut b, "longs");
        b.extend(2i32.to_be_bytes());
        b.extend(1i64.to_be_bytes());
        b.extend((-2i64).to_be_bytes());
        b.push(0);

        let keep = |d: usize, k: &str| d == 0 && (k == "want" || k == "longs");
        let n = parse_root(&b, &keep).unwrap();
        assert_eq!(n.get("want").unwrap().as_i64(), Some(-5));
        assert_eq!(n.get("longs").unwrap().as_longs(), Some(&[1i64, -2][..]));
        assert!(n.get("junklist").is_none());
    }

    #[test]
    fn truncated_data_is_an_error() {
        let b = vec![10, 0, 0, 3, 0, 1, b'a', 0, 0];
        assert!(parse_root(&b, &|_, _| true).is_err());
    }
}
