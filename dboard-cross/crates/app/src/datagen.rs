//! Plausible sample values for filling a table with test data.

use dboard_core::model::Column;

/// Small deterministic-per-seed generator (no external crate needed for test data).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

const WORDS: &[&str] = &["alpha", "bravo", "cedar", "delta", "ember", "falcon", "garnet", "harbor", "indigo", "jasper", "kestrel", "lumen", "maple", "nimbus", "onyx", "pebble"];
const FIRST: &[&str] = &["Ada", "Ben", "Cleo", "Dev", "Eli", "Fay", "Gus", "Hana", "Ivo", "Jun", "Kai", "Lea"];
const LAST: &[&str] = &["Stone", "Rivera", "Okafor", "Lindqvist", "Tanaka", "Moreau", "Silva", "Novak"];

fn varchar_limit(type_name: &str) -> Option<usize> {
    let open = type_name.find('(')?;
    let close = type_name[open..].find(')')? + open;
    type_name[open + 1..close].split(',').next()?.trim().parse().ok()
}

/// A value for `col` in generated row number `n` (1-based). `None` means NULL.
/// Columns the database fills itself (a default, e.g. serial ids) are skipped by the caller.
pub fn value_for(col: &Column, n: usize, rng: &mut Rng) -> Option<String> {
    let t = col.type_name.to_lowercase();
    let name = col.name.to_lowercase();
    // Occasionally NULL for optional columns, so the data is not unrealistically complete.
    if col.nullable && !col.is_primary_key && rng.below(12) == 0 {
        return None;
    }
    let v = if t.contains("bool") || t == "tinyint(1)" {
        (rng.below(2) == 1).to_string()
    } else if t.contains("uuid") {
        format!("{:08x}-{:04x}-4{:03x}-a{:03x}-{:012x}", rng.next() as u32, rng.next() as u16, rng.next() as u16 & 0xfff, rng.next() as u16 & 0xfff, rng.next() & 0xffff_ffff_ffff)
    } else if t.contains("timestamp") || t.contains("datetime") {
        let day = 1 + rng.below(28);
        format!("2024-{:02}-{:02} {:02}:{:02}:{:02}", 1 + rng.below(12), day, rng.below(24), rng.below(60), rng.below(60))
    } else if t == "date" {
        format!("2024-{:02}-{:02}", 1 + rng.below(12), 1 + rng.below(28))
    } else if t.starts_with("time") {
        format!("{:02}:{:02}:{:02}", rng.below(24), rng.below(60), rng.below(60))
    } else if t.contains("json") {
        format!("{{\"n\": {n}}}")
    } else if t.contains("int") || t.contains("serial") {
        if t.contains("small") || t == "tinyint" { rng.below(100).to_string() } else { (n as u64 * 7 + rng.below(1000)).to_string() }
    } else if ["numeric", "decimal", "float", "double", "real", "money"].iter().any(|k| t.contains(k)) {
        format!("{}.{:02}", rng.below(5000), rng.below(100))
    } else if name.contains("email") {
        format!("{}{n}@example.com", WORDS[rng.below(WORDS.len() as u64) as usize])
    } else if name == "name" || name.ends_with("_name") || name.contains("fullname") {
        format!("{} {}", FIRST[rng.below(FIRST.len() as u64) as usize], LAST[rng.below(LAST.len() as u64) as usize])
    } else {
        format!("{} {n}", WORDS[rng.below(WORDS.len() as u64) as usize])
    };
    // Respect varchar(n) / char(n).
    Some(match varchar_limit(&t) {
        Some(max) if (t.contains("char") || t.contains("text")) && v.chars().count() > max => v.chars().take(max).collect(),
        _ => v,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, ty: &str, nullable: bool) -> Column {
        Column { name: name.into(), type_name: ty.into(), nullable, is_primary_key: false, default: None, fk: None }
    }

    #[test]
    fn values_match_their_types() {
        let mut r = Rng::new(42);
        assert!(value_for(&col("n", "integer", false), 1, &mut r).unwrap().parse::<i64>().is_ok());
        assert!(value_for(&col("p", "numeric(10,2)", false), 1, &mut r).unwrap().parse::<f64>().is_ok());
        let b = value_for(&col("b", "boolean", false), 1, &mut r).unwrap();
        assert!(b == "true" || b == "false");
        assert!(value_for(&col("e", "text", false), 3, &mut r).is_some());
        let u = value_for(&col("u", "uuid", false), 1, &mut r).unwrap();
        assert_eq!(u.len(), 36);
        assert!(value_for(&col("when", "timestamp with time zone", false), 1, &mut r).unwrap().starts_with("2024-"));
    }

    #[test]
    fn emails_and_names_look_right() {
        let mut r = Rng::new(7);
        assert!(value_for(&col("email", "text", false), 5, &mut r).unwrap().ends_with("@example.com"));
        assert!(value_for(&col("first_name", "text", false), 1, &mut r).unwrap().contains(' '));
    }

    #[test]
    fn varchar_length_is_respected_and_nulls_only_when_nullable() {
        let mut r = Rng::new(3);
        for n in 0..200 {
            assert!(value_for(&col("c", "character varying(4)", false), n, &mut r).unwrap().chars().count() <= 4);
        }
        let nulls = (0..400).filter(|n| value_for(&col("x", "text", true), *n, &mut r).is_none()).count();
        assert!(nulls > 0 && nulls < 100);
    }
}
