/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The search pop-up's conversions for selected text (Settings → Search engine → Search
//! pop-up): a price into your currency, a measurement into the other system, a time with a time
//! zone into your local time.

use std::collections::HashMap;

use crate::omnibox::format_number;

/// Currency symbols and codes recognised in selected text.
const SYMBOLS: &[(&str, &str)] = &[
    ("$", "USD"),
    ("€", "EUR"),
    ("£", "GBP"),
    ("¥", "JPY"),
    ("₹", "INR"),
    ("₩", "KRW"),
    ("R$", "BRL"),
    ("zł", "PLN"),
    ("kr", "SEK"),
];

/// The codes HakuCX converts between (the ECB reference rates published by frankfurter.app).
pub const CURRENCIES: &[&str] = &[
    "USD", "EUR", "GBP", "JPY", "CAD", "AUD", "NOK", "SEK", "DKK", "PLN", "BRL", "INR", "CNY",
    "CHF", "KRW", "MXN", "CZK", "HUF", "TRY", "NZD", "SGD", "HKD", "ZAR",
];

fn number(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() || !s.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    // "1,234.56", "1.234,56" and "12,5" all appear in prices.
    let cleaned: String = s
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '\'')
        .collect();
    let (dot, comma) = (cleaned.rfind('.'), cleaned.rfind(','));
    let normalized = match (dot, comma) {
        (Some(d), Some(c)) if c > d => cleaned.replace('.', "").replace(',', "."),
        (Some(_), Some(_)) => cleaned.replace(',', ""),
        (None, Some(c)) if cleaned.len() - c - 1 == 3 => cleaned.replace(',', ""),
        (None, Some(_)) => cleaned.replace(',', "."),
        _ => cleaned,
    };
    normalized.parse().ok().filter(|v: &f64| v.is_finite())
}

/// A price in the text: (amount, currency code).
pub fn parse_money(text: &str) -> Option<(f64, &'static str)> {
    let t = text.trim();
    if t.len() > 40 {
        return None;
    }
    // Symbol or code first ("$12", "USD 12"), or after ("12 €", "12 EUR", "12kr").
    for (symbol, code) in SYMBOLS {
        if let Some(rest) = t.strip_prefix(symbol) {
            return number(rest).map(|v| (v, *code));
        }
        if let Some(rest) = t.strip_suffix(symbol) {
            return number(rest).map(|v| (v, *code));
        }
    }
    let upper = t.to_ascii_uppercase();
    for code in CURRENCIES {
        if let Some(rest) = upper.strip_prefix(code) {
            return number(rest).map(|v| (v, *code));
        }
        if let Some(rest) = upper.strip_suffix(code) {
            return number(rest).map(|v| (v, *code));
        }
    }
    None
}

/// `amount` in `from` as `to`, with rates against one base currency (`rates[base] = 1`).
pub fn convert_money(
    amount: f64,
    from: &str,
    to: &str,
    rates: &HashMap<String, f64>,
) -> Option<String> {
    if from == to {
        return None;
    }
    let (rf, rt) = (rates.get(from)?, rates.get(to)?);
    let out = amount / rf * rt;
    let decimals = if to == "JPY" || to == "KRW" { 0.0 } else { 2.0 };
    let p = 10f64.powf(decimals);
    Some(format!("{} {to}", format_number((out * p).round() / p)))
}

/// The usual counterpart of a unit in the other measuring system.
const COUNTERPARTS: &[(&str, &str)] = &[
    ("km", "mi"),
    ("mi", "km"),
    ("m", "ft"),
    ("ft", "m"),
    ("cm", "in"),
    ("in", "cm"),
    ("mm", "in"),
    ("kg", "lb"),
    ("lb", "kg"),
    ("g", "oz"),
    ("oz", "g"),
    ("l", "gal"),
    ("gal", "l"),
    ("km/h", "mph"),
    ("mph", "km/h"),
    ("°c", "°f"),
    ("°f", "°c"),
];

/// A measurement in the text, converted to the other system ("5 km" -> "3.1069 mi").
pub fn convert_units(text: &str) -> Option<String> {
    let t = text.trim().to_lowercase();
    if t.len() > 30 {
        return None;
    }
    let split = t.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | ',' | '-')))?;
    let (value, unit) = (t[..split].trim(), t[split..].trim());
    number(value)?;
    let unit = match unit {
        "kilometers" | "kilometres" | "kilometer" => "km",
        "miles" | "mile" => "mi",
        "meters" | "metres" | "meter" | "metre" => "m",
        "feet" | "foot" => "ft",
        "inches" | "inch" | "\"" => "in",
        "kilograms" | "kilogram" | "kgs" => "kg",
        "pounds" | "pound" | "lbs" => "lb",
        "grams" | "gram" => "g",
        "ounces" | "ounce" => "oz",
        "liters" | "litres" | "liter" | "litre" => "l",
        "gallons" | "gallon" => "gal",
        "kph" | "kmh" => "km/h",
        "°c" | "c" | "celsius" => "°c",
        "°f" | "f" | "fahrenheit" => "°f",
        other => other,
    };
    let target = COUNTERPARTS.iter().find(|(f, _)| *f == unit)?.1;
    let strip = |u: &str| u.trim_start_matches('°').to_owned();
    crate::omnibox::convert(&format!("{value} {} to {}", strip(unit), strip(target)))
}

/// Time zone abbreviations and their UTC offsets in minutes.
const ZONES: &[(&str, i32)] = &[
    ("UTC", 0),
    ("GMT", 0),
    ("BST", 60),
    ("CET", 60),
    ("CEST", 120),
    ("EET", 120),
    ("EEST", 180),
    ("MSK", 180),
    ("IST", 330),
    ("SGT", 480),
    ("CST", -360),
    ("CDT", -300),
    ("EST", -300),
    ("EDT", -240),
    ("MST", -420),
    ("MDT", -360),
    ("PST", -480),
    ("PDT", -420),
    ("AKST", -540),
    ("HST", -600),
    ("JST", 540),
    ("KST", 540),
    ("AEST", 600),
    ("AEDT", 660),
    ("NZST", 720),
    ("NZDT", 780),
];

/// A time with a time zone ("3pm EST", "15:30 UTC", "9:00 a.m. PT"), as minutes after
/// midnight in that zone, and the zone's offset.
pub fn parse_time(text: &str) -> Option<(i32, i32)> {
    let t = text.trim().to_ascii_uppercase().replace('.', "");
    if t.len() > 24 {
        return None;
    }
    let (time, zone) = t.rsplit_once(' ')?;
    let offset = ZONES.iter().find(|(z, _)| *z == zone)?.1;
    let time = time.trim();
    let (clock, pm) = if let Some(c) = time.strip_suffix("PM") {
        (c.trim(), Some(true))
    } else if let Some(c) = time.strip_suffix("AM") {
        (c.trim(), Some(false))
    } else {
        (time, None)
    };
    let (h, m) = match clock.split_once(':') {
        Some((h, m)) => (h.parse::<i32>().ok()?, m.parse::<i32>().ok()?),
        None => (clock.parse::<i32>().ok()?, 0),
    };
    if !(0..60).contains(&m) {
        return None;
    }
    let h = match pm {
        Some(_) if !(1..=12).contains(&h) => return None,
        Some(true) => h % 12 + 12,
        Some(false) => h % 12,
        None if (0..24).contains(&h) => h,
        None => return None,
    };
    Some((h * 60 + m, offset))
}

/// The time in a local zone (`local_offset` minutes from UTC), with a day note when it falls
/// on another day there.
pub fn convert_time(minutes: i32, offset: i32, local_offset: i32) -> String {
    let local = minutes - offset + local_offset;
    let day = local.div_euclid(24 * 60);
    let m = local.rem_euclid(24 * 60);
    let note = match day {
        0 => "",
        1 => " (next day)",
        -1 => " (previous day)",
        _ => "",
    };
    format!("{:02}:{:02} your time{note}", m / 60, m % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money() {
        assert_eq!(parse_money("$12.50"), Some((12.5, "USD")));
        assert_eq!(parse_money("12,50 €"), Some((12.5, "EUR")));
        assert_eq!(parse_money("€1.234,56"), Some((1234.56, "EUR")));
        assert_eq!(parse_money("1,299 USD"), Some((1299.0, "USD")));
        assert_eq!(parse_money("NOK 450"), Some((450.0, "NOK")));
        assert_eq!(parse_money("hello"), None);
        let rates: HashMap<String, f64> = [("EUR", 1.0), ("USD", 1.10), ("JPY", 160.0)]
            .iter()
            .map(|(k, v)| (k.to_string(), *v))
            .collect();
        assert_eq!(
            convert_money(11.0, "USD", "EUR", &rates).as_deref(),
            Some("10 EUR")
        );
        assert_eq!(
            convert_money(1.0, "EUR", "JPY", &rates).as_deref(),
            Some("160 JPY")
        );
        assert_eq!(convert_money(1.0, "EUR", "EUR", &rates), None);
        assert_eq!(convert_money(1.0, "EUR", "GBP", &rates), None);
    }

    #[test]
    fn units() {
        assert_eq!(convert_units("5 km").as_deref(), Some("3.1069 mi"));
        assert_eq!(convert_units("10 pounds").as_deref(), Some("4.5359 kg"));
        assert_eq!(convert_units("72°F").as_deref(), Some("22.22 °C"));
        assert_eq!(convert_units("6 ft").as_deref(), Some("1.8288 m"));
        assert_eq!(convert_units("five km"), None);
        assert_eq!(convert_units("5 parsecs"), None);
    }

    #[test]
    fn times() {
        assert_eq!(parse_time("3pm EST"), Some((15 * 60, -300)));
        assert_eq!(parse_time("9:30 a.m. PDT"), Some((9 * 60 + 30, -420)));
        assert_eq!(parse_time("15:00 UTC"), Some((15 * 60, 0)));
        assert_eq!(parse_time("12am GMT"), Some((0, 0)));
        assert_eq!(parse_time("25:00 UTC"), None);
        assert_eq!(parse_time("3pm"), None);
        // 3 pm in New York is 21:00 in Oslo (UTC+1 in winter).
        assert_eq!(convert_time(15 * 60, -300, 60), "21:00 your time");
        assert_eq!(convert_time(23 * 60, 0, 120), "01:00 your time (next day)");
        assert_eq!(convert_time(60, 120, 0), "23:00 your time (previous day)");
    }
}
