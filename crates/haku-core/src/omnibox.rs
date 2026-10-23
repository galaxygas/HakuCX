/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Address bar logic: turning what the user typed into a URL or a search, keyword search
//! engines, and the inline calculator and unit converter.

use crate::settings::{Search, SearchEngine};

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// Navigate to this URL.
    Url(String),
    /// Search with `engine` for `query`; `url` is the final search URL.
    Search {
        engine: String,
        query: String,
        url: String,
    },
}

impl Target {
    pub fn url(&self) -> &str {
        match self {
            Target::Url(u) => u,
            Target::Search { url, .. } => url,
        }
    }
}

const KNOWN_SCHEMES: &[&str] = &[
    "http",
    "https",
    "file",
    "haku",
    "about",
    "data",
    "view-source",
    "ftp",
];

/// Resolve address-bar input. Rules, in order:
/// 1. Explicit scheme (`https://...`, `haku://settings`, `file:///...`) is used as-is.
/// 2. `<keyword> <query>` uses that search engine when keyword shortcuts are on.
/// 3. `localhost`, IPv4/IPv6 literals and `host:port` become `http://`.
/// 4. Something that looks like a domain (has a dot, a known-looking TLD, no spaces) becomes
///    `https://`.
/// 5. Everything else is a search with the default engine.
pub fn resolve(input: &str, search: &Search, private: bool) -> Option<Target> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }

    if let Some(colon) = input.find(':') {
        let scheme = input[..colon].to_ascii_lowercase();
        if KNOWN_SCHEMES.contains(&scheme.as_str())
            && !input.contains(' ')
            && let Ok(url) = url::Url::parse(input)
        {
            return Some(Target::Url(url.to_string()));
        }
    }

    if search.keyword_shortcuts
        && let Some((keyword, rest)) = input.split_once(' ')
        && !rest.trim().is_empty()
        && let Some(engine) = search
            .engines
            .iter()
            .find(|e| e.active && e.keyword.eq_ignore_ascii_case(keyword))
    {
        return Some(search_with(engine, rest.trim()));
    }

    if !input.contains(char::is_whitespace) {
        if looks_local(input) {
            if let Ok(url) = url::Url::parse(&format!("http://{input}")) {
                return Some(Target::Url(url.to_string()));
            }
        } else if looks_like_domain(input)
            && let Ok(url) = url::Url::parse(&format!("https://{input}"))
        {
            return Some(Target::Url(url.to_string()));
        }
    }

    let engine_id = if private {
        search.private_engine.as_deref()
    } else {
        None
    }
    .unwrap_or(&search.default_engine);
    let engine = search
        .engines
        .iter()
        .find(|e| e.id == engine_id)
        .or_else(|| search.engines.first())?;
    Some(search_with(engine, input))
}

pub fn search_with(engine: &SearchEngine, query: &str) -> Target {
    let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    Target::Search {
        engine: engine.id.clone(),
        query: query.to_owned(),
        url: engine.url.replace("%s", &encoded),
    }
}

fn looks_local(input: &str) -> bool {
    let host_port = input.split(['/', '?', '#']).next().unwrap_or(input);
    let host = match host_port.rsplit_once(':') {
        Some((h, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => h,
        _ => host_port,
    };
    host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".localhost")
        || host.parse::<std::net::Ipv4Addr>().is_ok()
        || (host.starts_with('[') && host.ends_with(']'))
}

fn looks_like_domain(input: &str) -> bool {
    let host_port = input.split(['/', '?', '#']).next().unwrap_or(input);
    let host = host_port.split(':').next().unwrap_or(host_port);
    if !host.contains('.') || host.starts_with('.') || host.ends_with('.') {
        return false;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.iter().any(|l| l.is_empty() || l.len() > 63) {
        return false;
    }
    if !host
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '.')
    {
        return false;
    }
    let tld = labels.last().expect("non-empty");
    // A TLD is alphabetic (or punycode) and at least two characters: "file.txt" style input
    // with a 2-4 letter "extension" is still ambiguous, so require that it isn't a common
    // file extension.
    let tld_ok =
        (tld.len() >= 2 && tld.chars().all(|c| c.is_alphabetic())) || tld.starts_with("xn--");
    const FILE_EXTENSIONS: &[&str] = &[
        "txt", "pdf", "exe", "zip", "png", "jpg", "jpeg", "gif", "doc", "docx", "rs", "js", "ts",
        "py", "md", "json", "toml", "mp3", "mp4",
    ];
    tld_ok && !FILE_EXTENSIONS.contains(&tld.to_ascii_lowercase().as_str())
}

/// Evaluate a simple arithmetic expression (`+ - * / % ^`, parentheses, decimals).
/// Returns `None` unless the input is clearly a calculation.
pub fn calculate(input: &str) -> Option<f64> {
    let s = input.trim().trim_start_matches('=').trim();
    if !s.chars().any(|c| "+-*/%^x×÷".contains(c)) || !s.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_digit() || " .,+-*/%^()x×÷".contains(c))
    {
        return None;
    }
    let normalized: String = s
        .chars()
        .map(|c| match c {
            'x' | '×' => '*',
            '÷' => '/',
            ',' => '.',
            c => c,
        })
        .filter(|c| !c.is_whitespace())
        .collect();
    let mut parser = Calc {
        s: normalized.as_bytes(),
        i: 0,
    };
    let value = parser.expr()?;
    (parser.i == parser.s.len() && value.is_finite()).then_some(value)
}

struct Calc<'a> {
    s: &'a [u8],
    i: usize,
}

impl Calc<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        while let Some(op @ (b'+' | b'-')) = self.peek() {
            self.i += 1;
            let r = self.term()?;
            v = if op == b'+' { v + r } else { v - r };
        }
        Some(v)
    }
    fn term(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        while let Some(op @ (b'*' | b'/' | b'%')) = self.peek() {
            self.i += 1;
            let r = self.power()?;
            v = match op {
                b'*' => v * r,
                b'/' if r != 0.0 => v / r,
                b'%' if r != 0.0 => v % r,
                _ => return None,
            };
        }
        Some(v)
    }
    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if self.peek() == Some(b'^') {
            self.i += 1;
            let exp = self.power()?;
            return Some(base.powf(exp));
        }
        Some(base)
    }
    fn unary(&mut self) -> Option<f64> {
        match self.peek()? {
            b'-' => {
                self.i += 1;
                Some(-self.unary()?)
            }
            b'+' => {
                self.i += 1;
                self.unary()
            }
            b'(' => {
                self.i += 1;
                let v = self.expr()?;
                (self.peek() == Some(b')')).then(|| self.i += 1)?;
                Some(v)
            }
            _ => self.number(),
        }
    }
    fn number(&mut self) -> Option<f64> {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == b'.') {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i])
            .ok()?
            .parse()
            .ok()
    }
}

/// Format a calculation result without float noise (`0.1+0.2` -> `0.3`).
pub fn format_number(v: f64) -> String {
    let rounded = (v * 1e10).round() / 1e10;
    let s = format!("{rounded}");
    if s == "-0" { "0".into() } else { s }
}

struct Unit {
    names: &'static [&'static str],
    kind: &'static str,
    /// Multiply by this to get the base unit (meters, kilograms, liters, m/s, bytes).
    factor: f64,
}

const UNITS: &[Unit] = &[
    Unit {
        names: &["mm", "millimeter", "millimeters"],
        kind: "length",
        factor: 0.001,
    },
    Unit {
        names: &["cm", "centimeter", "centimeters"],
        kind: "length",
        factor: 0.01,
    },
    Unit {
        names: &["m", "meter", "meters", "metre", "metres"],
        kind: "length",
        factor: 1.0,
    },
    Unit {
        names: &["km", "kilometer", "kilometers"],
        kind: "length",
        factor: 1000.0,
    },
    Unit {
        names: &["in", "inch", "inches"],
        kind: "length",
        factor: 0.0254,
    },
    Unit {
        names: &["ft", "foot", "feet"],
        kind: "length",
        factor: 0.3048,
    },
    Unit {
        names: &["yd", "yard", "yards"],
        kind: "length",
        factor: 0.9144,
    },
    Unit {
        names: &["mi", "mile", "miles"],
        kind: "length",
        factor: 1609.344,
    },
    Unit {
        names: &["g", "gram", "grams"],
        kind: "mass",
        factor: 0.001,
    },
    Unit {
        names: &["kg", "kilogram", "kilograms"],
        kind: "mass",
        factor: 1.0,
    },
    Unit {
        names: &["oz", "ounce", "ounces"],
        kind: "mass",
        factor: 0.028_349_523_125,
    },
    Unit {
        names: &["lb", "lbs", "pound", "pounds"],
        kind: "mass",
        factor: 0.453_592_37,
    },
    Unit {
        names: &["ml", "milliliter", "milliliters"],
        kind: "volume",
        factor: 0.001,
    },
    Unit {
        names: &["l", "liter", "liters", "litre", "litres"],
        kind: "volume",
        factor: 1.0,
    },
    Unit {
        names: &["gal", "gallon", "gallons"],
        kind: "volume",
        factor: 3.785_411_784,
    },
    Unit {
        names: &["kmh", "km/h", "kph"],
        kind: "speed",
        factor: 1.0 / 3.6,
    },
    Unit {
        names: &["mph"],
        kind: "speed",
        factor: 0.447_04,
    },
    Unit {
        names: &["m/s", "mps"],
        kind: "speed",
        factor: 1.0,
    },
    Unit {
        names: &["b", "byte", "bytes"],
        kind: "data",
        factor: 1.0,
    },
    Unit {
        names: &["kb"],
        kind: "data",
        factor: 1e3,
    },
    Unit {
        names: &["mb"],
        kind: "data",
        factor: 1e6,
    },
    Unit {
        names: &["gb"],
        kind: "data",
        factor: 1e9,
    },
    Unit {
        names: &["tb"],
        kind: "data",
        factor: 1e12,
    },
    Unit {
        names: &["kib"],
        kind: "data",
        factor: 1024.0,
    },
    Unit {
        names: &["mib"],
        kind: "data",
        factor: 1_048_576.0,
    },
    Unit {
        names: &["gib"],
        kind: "data",
        factor: 1_073_741_824.0,
    },
];

fn unit(name: &str) -> Option<&'static Unit> {
    let n = name.to_ascii_lowercase();
    UNITS.iter().find(|u| u.names.contains(&n.as_str()))
}

fn temperature(name: &str) -> Option<char> {
    match name.to_ascii_lowercase().trim_start_matches('°') {
        "c" | "celsius" => Some('C'),
        "f" | "fahrenheit" => Some('F'),
        "k" | "kelvin" => Some('K'),
        _ => None,
    }
}

/// Convert `"<number> <unit> to|in <unit>"`, e.g. `10 km to mi`, `72 °F in C`.
/// Returns the formatted result (`6.2137 mi`).
pub fn convert(input: &str) -> Option<String> {
    let lower = input.trim().to_lowercase();
    let (left, right) = lower
        .split_once(" to ")
        .or_else(|| lower.split_once(" in "))?;
    let left = left.trim();
    let right = right.trim();
    let split = left
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == ','))
        .unwrap_or(left.len());
    let value: f64 = left[..split].replace(',', ".").trim().parse().ok()?;
    let from = left[split..].trim();

    if let (Some(f), Some(t)) = (temperature(from), temperature(right)) {
        let celsius = match f {
            'C' => value,
            'F' => (value - 32.0) * 5.0 / 9.0,
            _ => value - 273.15,
        };
        let out = match t {
            'C' => celsius,
            'F' => celsius * 9.0 / 5.0 + 32.0,
            _ => celsius + 273.15,
        };
        let suffix = if t == 'K' {
            " K".to_owned()
        } else {
            format!(" °{t}")
        };
        return Some(format!(
            "{}{suffix}",
            format_number((out * 100.0).round() / 100.0)
        ));
    }

    let (f, t) = (unit(from)?, unit(right)?);
    if f.kind != t.kind {
        return None;
    }
    let out = value * f.factor / t.factor;
    let precise = if out.abs() >= 100.0 {
        (out * 100.0).round() / 100.0
    } else {
        (out * 10_000.0).round() / 10_000.0
    };
    Some(format!("{} {}", format_number(precise), t.names[0]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s() -> Search {
        Search::default()
    }

    #[test]
    fn urls_and_schemes() {
        assert_eq!(
            resolve("https://servo.org", &s(), false),
            Some(Target::Url("https://servo.org/".into()))
        );
        assert_eq!(
            resolve("haku://settings/", &s(), false),
            Some(Target::Url("haku://settings/".into()))
        );
        assert_eq!(
            resolve("servo.org", &s(), false),
            Some(Target::Url("https://servo.org/".into()))
        );
        assert_eq!(
            resolve("en.wikipedia.org/wiki/Rust", &s(), false),
            Some(Target::Url("https://en.wikipedia.org/wiki/Rust".into()))
        );
        assert_eq!(
            resolve("localhost:3000", &s(), false),
            Some(Target::Url("http://localhost:3000/".into()))
        );
        assert_eq!(
            resolve("192.168.1.1", &s(), false),
            Some(Target::Url("http://192.168.1.1/".into()))
        );
        assert_eq!(resolve("  ", &s(), false), None);
    }

    #[test]
    fn searches() {
        let t = resolve("rust borrow checker", &s(), false).unwrap();
        assert_eq!(t.url(), "https://duckduckgo.com/?q=rust+borrow+checker");
        let t = resolve("notes.txt", &s(), false).unwrap();
        assert!(matches!(t, Target::Search { .. }));
        let t = resolve("w Servo (software)", &s(), false).unwrap();
        assert_eq!(
            t.url(),
            "https://en.wikipedia.org/wiki/Special:Search?search=Servo+%28software%29"
        );
        let t = resolve("javascript:alert(1)", &s(), false).unwrap();
        assert!(
            matches!(t, Target::Search { .. }),
            "javascript: must never navigate"
        );
    }

    #[test]
    fn private_engine() {
        let mut search = s();
        search.private_engine = Some("startpage".into());
        let t = resolve("hello", &search, true).unwrap();
        assert!(t.url().starts_with("https://www.startpage.com/"));
    }

    #[test]
    fn keyword_shortcuts_can_be_disabled() {
        let mut search = s();
        search.keyword_shortcuts = false;
        let t = resolve("w rust", &search, false).unwrap();
        assert!(t.url().starts_with("https://duckduckgo.com/"));
    }

    #[test]
    fn calculator() {
        assert_eq!(calculate("2+2"), Some(4.0));
        assert_eq!(calculate("= (1 + 2) * 3 ^ 2"), Some(27.0));
        assert_eq!(calculate("10 / 4"), Some(2.5));
        assert_eq!(calculate("-3 x 2"), Some(-6.0));
        assert_eq!(calculate("1/0"), None);
        assert_eq!(calculate("42"), None);
        assert_eq!(calculate("rust 2+2"), None);
        assert_eq!(format_number(calculate("0.1+0.2").unwrap()), "0.3");
    }

    #[test]
    fn conversions() {
        assert_eq!(convert("10 km to mi").as_deref(), Some("6.2137 mi"));
        assert_eq!(convert("100 C in F").as_deref(), Some("212 °F"));
        assert_eq!(convert("32 °F to c").as_deref(), Some("0 °C"));
        assert_eq!(convert("1 gb to mib").as_deref(), Some("953.67 mib"));
        assert_eq!(convert("5 kg to km"), None);
        assert_eq!(convert("hello to world"), None);
    }
}
