#![forbid(unsafe_code)]
// Fixed-layout timestamp parsing for perimeter-device logs. std only.
// Every policy applied while parsing is reported as a flag in `Policies`;
// the survey of formats and the zone table live in docs/timestamps.md.

use std::fmt::Write as _;

pub type EpochNanos = i64;

const NANOS_PER_SEC: i64 = 1_000_000_000;
const DAY_SECS: i64 = 86_400;
// A no-year timestamp more than this far after receipt belongs to the previous year.
const YEAR_ROLLOVER_SLACK: i64 = 7 * DAY_SECS * NANOS_PER_SEC;

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Policies(u8);

const POLICY_NAMES: [(u8, &str); 5] = [
    (1, "year_assumed"),
    (2, "tz_assumed"),
    (4, "zone_name_unknown"),
    (8, "zone_name_ambiguous"),
    (16, "receipt_fallback"),
];

impl Policies {
    pub const NONE: Policies = Policies(0);
    pub const YEAR_ASSUMED: Policies = Policies(1);
    pub const TZ_ASSUMED: Policies = Policies(2);
    pub const ZONE_NAME_UNKNOWN: Policies = Policies(4);
    pub const ZONE_NAME_AMBIGUOUS: Policies = Policies(8);
    pub const RECEIPT_FALLBACK: Policies = Policies(16);

    pub fn contains(self, other: Policies) -> bool {
        self.0 & other.0 == other.0
    }
    pub fn union(self, other: Policies) -> Policies {
        Policies(self.0 | other.0)
    }
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
    pub fn names(self) -> impl Iterator<Item = &'static str> {
        POLICY_NAMES
            .iter()
            .filter(move |(bit, _)| self.0 & bit != 0)
            .map(|(_, name)| *name)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Timestamp {
    pub epoch_nanos: EpochNanos,
    pub policies: Policies,
}

#[derive(Clone, Copy, Debug)]
pub struct Context {
    pub receipt_epoch_nanos: EpochNanos,
    pub default_offset_secs: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Format {
    Auto,
    Rfc3339,
    Syslog,
    Ctime,
    EpochSecs,
    EpochMillis,
    EpochMicros,
    EpochNanos,
    Strftime(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatError {
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeError {
    Empty,
    NoMatch,
    OutOfRange,
}

impl TimeError {
    pub fn reason(self) -> &'static str {
        match self {
            TimeError::Empty => "empty",
            TimeError::NoMatch => "no_match",
            TimeError::OutOfRange => "out_of_range",
        }
    }
}

const DIRECTIVES: &[u8] = b"YymdebBHMSfzZjspIn%";

impl Format {
    pub fn from_spec(spec: &str) -> Result<Format, FormatError> {
        Ok(match spec {
            "auto" => Format::Auto,
            "rfc3339" => Format::Rfc3339,
            "syslog" => Format::Syslog,
            "ctime" => Format::Ctime,
            "epoch" => Format::EpochSecs,
            "epoch_ms" => Format::EpochMillis,
            "epoch_us" => Format::EpochMicros,
            "epoch_ns" => Format::EpochNanos,
            s if s.contains('%') => {
                validate_layout(s)?;
                Format::Strftime(s.to_string())
            }
            s => {
                return Err(FormatError {
                    message: format!("unknown timestamp format spec `{s}`"),
                });
            }
        })
    }
}

fn validate_layout(layout: &str) -> Result<(), FormatError> {
    let mut it = layout.bytes();
    while let Some(b) = it.next() {
        if b != b'%' {
            continue;
        }
        match it.next() {
            Some(d) if DIRECTIVES.contains(&d) => {}
            Some(d) => {
                return Err(FormatError {
                    message: format!(
                        "unsupported strftime directive `%{}` in `{layout}`",
                        d as char
                    ),
                });
            }
            None => {
                return Err(FormatError {
                    message: format!("dangling `%` at end of `{layout}`"),
                });
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- calendar

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Civil {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub nanos: u32,
}

// Howard Hinnant's days_from_civil / civil_from_days (proleptic Gregorian).
fn days_from_civil(y: i32, m: u8, d: u8) -> i64 {
    let y = i64::from(y) - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i32, u8, u8) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y as i32, m as u8, d as u8)
}

fn is_leap(y: i32) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

fn days_in_month(y: i32, m: u8) -> u8 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => 28 + u8::from(is_leap(y)),
        _ => 0,
    }
}

pub fn civil_from_epoch(nanos: EpochNanos) -> Civil {
    let secs = nanos.div_euclid(NANOS_PER_SEC);
    let sub = nanos.rem_euclid(NANOS_PER_SEC) as u32;
    let sod = secs.rem_euclid(DAY_SECS);
    let (year, month, day) = civil_from_days(secs.div_euclid(DAY_SECS));
    Civil {
        year,
        month,
        day,
        hour: (sod / 3600) as u8,
        minute: (sod % 3600 / 60) as u8,
        second: (sod % 60) as u8,
        nanos: sub,
    }
}

// None when any field is out of range (year outside 1970..=9999, Feb 30, hour 24, ...)
// or the instant does not fit in i64 nanos (after 2262-04-11T23:47:16Z).
pub fn epoch_from_civil(c: &Civil, offset_secs: i32) -> Option<EpochNanos> {
    let valid = (1970..=9999).contains(&c.year)
        && (1..=12).contains(&c.month)
        && (1..=days_in_month(c.year, c.month)).contains(&c.day)
        && c.hour < 24
        && c.minute < 60
        && c.second <= 60
        && c.nanos < 1_000_000_000;
    if !valid {
        return None;
    }
    let secs = days_from_civil(c.year, c.month, c.day) * DAY_SECS
        + i64::from(c.hour) * 3600
        + i64::from(c.minute) * 60
        + i64::from(c.second)
        - i64::from(offset_secs);
    secs.checked_mul(NANOS_PER_SEC)?
        .checked_add(i64::from(c.nanos))
}

pub fn epoch_millis(nanos: EpochNanos) -> i64 {
    nanos.div_euclid(1_000_000)
}

pub fn format_rfc3339(nanos: EpochNanos, out: &mut String) {
    let c = civil_from_epoch(nanos);
    let _ = write!(
        out,
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        c.year,
        c.month,
        c.day,
        c.hour,
        c.minute,
        c.second,
        c.nanos / 1_000_000
    );
}

// ---------------------------------------------------------------- tables

const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];
const WEEKDAYS: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

// (name, offset seconds, ambiguous). The pick for each ambiguous name is recorded
// in docs/timestamps.md; the parser flags ZONE_NAME_AMBIGUOUS whenever one is used.
const ZONES: &[(&str, i32, bool)] = &[
    ("UTC", 0, false),
    ("GMT", 0, false),
    ("Z", 0, false),
    ("UT", 0, false),
    ("WET", 0, false),
    ("WEST", 3600, false),
    ("BST", 3600, true),
    ("CET", 3600, false),
    ("CEST", 7200, false),
    ("MET", 3600, false),
    ("MEST", 7200, false),
    ("WAT", 3600, false),
    ("EET", 7200, false),
    ("EEST", 10800, false),
    ("SAST", 7200, false),
    ("CAT", 7200, false),
    ("MSK", 10800, false),
    ("EAT", 10800, false),
    ("GST", 14400, true),
    ("PKT", 18000, false),
    ("IST", 19800, true),
    ("ICT", 25200, false),
    ("WIB", 25200, false),
    ("HKT", 28800, false),
    ("SGT", 28800, false),
    ("MYT", 28800, false),
    ("PHT", 28800, false),
    ("AWST", 28800, false),
    ("JST", 32400, false),
    ("KST", 32400, false),
    ("ACST", 34200, false),
    ("ACDT", 37800, false),
    ("AEST", 36000, false),
    ("AEDT", 39600, false),
    ("NZST", 43200, false),
    ("NZDT", 46800, false),
    ("BRT", -10800, false),
    ("ART", -10800, false),
    ("ADT", -10800, false),
    ("NST", -12600, false),
    ("NDT", -9000, false),
    ("AST", -14400, true),
    ("EST", -18000, false),
    ("EDT", -14400, false),
    ("CST", -21600, true),
    ("CDT", -18000, true),
    ("MST", -25200, false),
    ("MDT", -21600, false),
    ("PST", -28800, false),
    ("PDT", -25200, false),
    ("AKST", -32400, false),
    ("AKDT", -28800, false),
    ("HST", -36000, false),
];

// Auto order after epoch/rfc3339/syslog/ctime. Each layout starts with a token the
// earlier parsers reject, so order among them only matters for speed.
const AUTO_LAYOUTS: [&str; 4] = [
    "%Y/%m/%d %H:%M:%S",    // PAN-OS CSV receive_time / generated_time
    "%d/%b/%Y:%H:%M:%S %z", // Apache / nginx common log (brackets stripped first)
    "%d%b%Y %H:%M:%S",      // Check Point fw log
    "%Y-%m-%d %H:%M:%S %Z", // SonicWall time=, Sophos date/time/timezone composed
];

// ---------------------------------------------------------------- cursor

#[derive(Clone, Copy)]
struct Cur<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Cur<'a> {
    fn new(s: &'a [u8]) -> Self {
        Cur { s, i: 0 }
    }
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn eof(&self) -> bool {
        self.i >= self.s.len()
    }
    fn rest(&self) -> &'a [u8] {
        self.s.get(self.i..).unwrap_or(&[])
    }
    fn lit(&mut self, b: u8) -> Option<()> {
        (self.peek()? == b).then(|| self.i += 1)
    }
    fn take_while(&mut self, f: impl Fn(u8) -> bool) -> &'a [u8] {
        let start = self.i;
        while self.peek().is_some_and(&f) {
            self.i += 1;
        }
        &self.s[start..self.i]
    }
    // one-or-more ASCII whitespace
    fn ws(&mut self) -> Option<()> {
        (!self.take_while(|b| b.is_ascii_whitespace()).is_empty()).then_some(())
    }
    fn skip_ws(&mut self) {
        self.take_while(|b| b.is_ascii_whitespace());
    }
    fn digits(&mut self, min: usize, max: usize) -> Option<u32> {
        let start = self.i;
        let mut v: u32 = 0;
        while self.i - start < max {
            match self.peek() {
                Some(b) if b.is_ascii_digit() => {
                    v = v * 10 + u32::from(b - b'0');
                    self.i += 1;
                }
                _ => break,
            }
        }
        (self.i - start >= min).then_some(v)
    }
    fn fixed(&mut self, n: usize) -> Option<u32> {
        self.digits(n, n)
    }
    // digits after the separator, scaled to nanos; digits beyond nine are truncated
    fn fraction(&mut self) -> Option<u32> {
        let d = self.take_while(|b| b.is_ascii_digit());
        if d.is_empty() {
            return None;
        }
        let mut v: u32 = 0;
        let mut n = 0;
        for &b in d.iter().take(9) {
            v = v * 10 + u32::from(b - b'0');
            n += 1;
        }
        while n < 9 {
            v *= 10;
            n += 1;
        }
        Some(v)
    }
    fn alpha(&mut self) -> &'a [u8] {
        self.take_while(|b| b.is_ascii_alphabetic())
    }
    // an alphabetic run of three or more letters that prefixes a name in `table`
    fn name_in(&mut self, table: &[&str]) -> Option<usize> {
        let save = *self;
        let a = self.alpha();
        if a.len() >= 3
            && let Some(i) = table
                .iter()
                .position(|n| n.len() >= a.len() && n.as_bytes()[..a.len()].eq_ignore_ascii_case(a))
        {
            return Some(i);
        }
        *self = save;
        None
    }
    fn month(&mut self) -> Option<u8> {
        self.name_in(&MONTHS).map(|i| i as u8 + 1)
    }
    fn ampm(&mut self) -> Option<bool> {
        let a = self.alpha();
        if a.eq_ignore_ascii_case(b"am") {
            Some(false)
        } else if a.eq_ignore_ascii_case(b"pm") {
            Some(true)
        } else {
            None
        }
    }
    // Z | z | +HH:MM | +HHMM | +HH
    fn offset(&mut self) -> Option<i32> {
        match self.peek()? {
            b'Z' | b'z' => {
                self.i += 1;
                Some(0)
            }
            sign @ (b'+' | b'-') => {
                self.i += 1;
                let h = self.fixed(2)?;
                let m = if self.lit(b':').is_some() {
                    self.fixed(2)?
                } else {
                    self.fixed(2).unwrap_or(0)
                };
                let v = (h * 3600 + m * 60) as i32;
                (h < 24 && m < 60).then_some(if sign == b'-' { -v } else { v })
            }
            _ => None,
        }
    }
    // zone abbreviations are 1..=5 letters; longer runs are hostnames, not zones
    fn zone_name(&mut self) -> Option<&'a [u8]> {
        let a = self.alpha();
        (1..=5).contains(&a.len()).then_some(a)
    }
}

fn to_i64(digits: &[u8]) -> Option<i64> {
    digits.iter().try_fold(0i64, |v, &b| {
        v.checked_mul(10)?.checked_add(i64::from(b - b'0'))
    })
}

// ---------------------------------------------------------------- broken-down time

#[derive(Clone, Copy, Default)]
enum Zone<'a> {
    #[default]
    None,
    Offset(i32),
    Name(&'a [u8]),
}

#[derive(Default)]
struct Parts<'a> {
    year: Option<i32>,
    month: u8,
    day: u8,
    yday: Option<u16>,
    hour: u8,
    minute: u8,
    second: u8,
    nanos: u32,
    pm: Option<bool>,
    zone: Zone<'a>,
    epoch: Option<EpochNanos>, // already in nanos; wins over every other field
}

// HH:MM:SS with optional .fraction or ,fraction
fn hms(c: &mut Cur<'_>, p: &mut Parts<'_>) -> Option<()> {
    p.hour = c.digits(1, 2)? as u8;
    c.lit(b':')?;
    p.minute = c.fixed(2)? as u8;
    c.lit(b':')?;
    p.second = c.fixed(2)? as u8;
    if matches!(c.peek(), Some(b'.' | b',')) {
        c.i += 1;
        p.nanos = c.fraction()?;
    }
    Some(())
}

// YYYY-MM-DD[T ]HH:MM:SS[.frac][ ][Z|±HH:MM|±HHMM]
fn rfc3339(s: &[u8]) -> Option<Parts<'_>> {
    let mut c = Cur::new(s);
    let year = c.fixed(4)? as i32;
    c.lit(b'-')?;
    let month = c.fixed(2)? as u8;
    c.lit(b'-')?;
    let day = c.fixed(2)? as u8;
    let mut p = Parts {
        year: Some(year),
        month,
        day,
        ..Default::default()
    };
    match c.peek()? {
        b'T' | b't' => c.i += 1,
        b if b.is_ascii_whitespace() => c.skip_ws(),
        _ => return None,
    }
    hms(&mut c, &mut p)?;
    c.skip_ws();
    if !c.eof() {
        p.zone = Zone::Offset(c.offset()?);
    }
    c.eof().then_some(p)
}

// Mon d HH:MM:SS | Mon dd YYYY HH:MM:SS | Mon dd HH:MM:SS YYYY, optional fraction,
// optional trailing zone name or offset. A leading '*' or '.' (Cisco IOS clock
// state) is skipped.
fn syslog(s: &[u8]) -> Option<Parts<'_>> {
    let mut c = Cur::new(s);
    let mut p = Parts::default();
    if matches!(c.peek(), Some(b'*' | b'.')) {
        c.i += 1;
    }
    p.month = c.month()?;
    c.ws()?;
    p.day = c.digits(1, 2)? as u8;
    c.ws()?;
    let save = c;
    match c.fixed(4).and_then(|y| c.ws().map(|()| y)) {
        Some(y) => p.year = Some(y as i32),
        None => c = save,
    }
    hms(&mut c, &mut p)?;
    for _ in 0..2 {
        let save = c;
        if c.ws().is_none() {
            break;
        }
        match c.peek() {
            Some(b) if b.is_ascii_digit() && p.year.is_none() => p.year = Some(c.fixed(4)? as i32),
            Some(b'+' | b'-') => p.zone = Zone::Offset(c.offset()?),
            Some(b) if b.is_ascii_alphabetic() => p.zone = Zone::Name(c.zone_name()?),
            _ => {
                c = save;
                break;
            }
        }
    }
    c.eof().then_some(p)
}

// Www Mon d HH:MM:SS YYYY (weekday validated, remainder is syslog)
fn ctime(s: &[u8]) -> Option<Parts<'_>> {
    let mut c = Cur::new(s);
    c.name_in(&WEEKDAYS)?;
    c.ws()?;
    syslog(c.rest())
}

// digits[.digits]; `unit` is nanos per integer unit, None picks by magnitude
fn epoch(s: &[u8], unit: Option<i64>) -> Result<Parts<'static>, TimeError> {
    let mut c = Cur::new(s);
    let int = c.take_while(|b| b.is_ascii_digit());
    if int.is_empty() {
        return Err(TimeError::NoMatch);
    }
    let frac = if c.lit(b'.').is_some() {
        let f = c.take_while(|b| b.is_ascii_digit());
        if f.is_empty() {
            return Err(TimeError::NoMatch);
        }
        f
    } else {
        &[][..]
    };
    if !c.eof() {
        return Err(TimeError::NoMatch);
    }
    let v = to_i64(int).ok_or(TimeError::OutOfRange)?;
    let unit = unit.unwrap_or(if v < 100_000_000_000 {
        NANOS_PER_SEC
    } else if v < 100_000_000_000_000 {
        1_000_000
    } else if v < 100_000_000_000_000_000 {
        1_000
    } else {
        1
    });
    let mut sub = 0i64;
    let mut scale = unit;
    for &b in frac {
        if scale == 1 {
            break;
        }
        scale /= 10;
        sub += i64::from(b - b'0') * scale;
    }
    let nanos = v
        .checked_mul(unit)
        .and_then(|n| n.checked_add(sub))
        .ok_or(TimeError::OutOfRange)?;
    Ok(Parts {
        epoch: Some(nanos),
        ..Default::default()
    })
}

fn strftime<'a>(layout: &[u8], s: &'a [u8]) -> Option<Parts<'a>> {
    let mut c = Cur::new(s);
    let mut l = Cur::new(layout);
    let mut p = Parts::default();
    while let Some(b) = l.peek() {
        l.i += 1;
        if b == b'%' {
            let d = l.peek()?;
            l.i += 1;
            match d {
                b'Y' => p.year = Some(c.fixed(4)? as i32),
                b'y' => {
                    let v = c.fixed(2)? as i32;
                    let base = if v >= 69 { 1900 } else { 2000 };
                    p.year = Some(base + v);
                }
                b'm' => p.month = c.digits(1, 2)? as u8,
                b'd' => p.day = c.digits(1, 2)? as u8,
                b'e' => {
                    c.skip_ws();
                    p.day = c.digits(1, 2)? as u8;
                }
                b'b' | b'B' => p.month = c.month()?,
                b'H' | b'I' => p.hour = c.digits(1, 2)? as u8,
                b'M' => p.minute = c.digits(1, 2)? as u8,
                b'S' => p.second = c.digits(1, 2)? as u8,
                b'f' => p.nanos = c.fraction()?,
                b'z' => p.zone = Zone::Offset(c.offset()?),
                b'Z' => p.zone = Zone::Name(c.zone_name()?),
                b'j' => p.yday = Some(c.digits(1, 3)? as u16),
                b's' => {
                    let d = c.take_while(|b| b.is_ascii_digit());
                    p.epoch = Some(to_i64(d)?.checked_mul(NANOS_PER_SEC)?);
                }
                b'p' => p.pm = Some(c.ampm()?),
                b'n' => c.ws()?,
                b'%' => c.lit(b'%')?,
                _ => return None,
            }
        } else if b.is_ascii_whitespace() {
            l.skip_ws();
            c.ws()?;
        } else {
            c.lit(b)?;
        }
    }
    c.eof().then_some(p)
}

fn auto(s: &[u8]) -> Result<Parts<'_>, TimeError> {
    let s = match s {
        [b'[', inner @ .., b']'] => inner,
        _ => s,
    };
    if s.first().is_some_and(u8::is_ascii_digit)
        && s.iter().all(|b| b.is_ascii_digit() || *b == b'.')
    {
        return epoch(s, None);
    }
    rfc3339(s)
        .or_else(|| syslog(s))
        .or_else(|| ctime(s))
        .or_else(|| AUTO_LAYOUTS.iter().find_map(|l| strftime(l.as_bytes(), s)))
        .ok_or(TimeError::NoMatch)
}

fn resolve(p: Parts, ctx: &Context) -> Result<Timestamp, TimeError> {
    if let Some(e) = p.epoch {
        return e
            .checked_add(i64::from(p.nanos))
            .map(|epoch_nanos| Timestamp {
                epoch_nanos,
                policies: Policies::NONE,
            })
            .ok_or(TimeError::OutOfRange);
    }
    let (offset, mut policies) = match p.zone {
        Zone::None => (ctx.default_offset_secs, Policies::TZ_ASSUMED),
        Zone::Offset(o) => (o, Policies::NONE),
        Zone::Name(n) => match ZONES
            .iter()
            .find(|(z, _, _)| z.as_bytes().eq_ignore_ascii_case(n))
        {
            Some(&(_, o, false)) => (o, Policies::NONE),
            Some(&(_, o, true)) => (o, Policies::ZONE_NAME_AMBIGUOUS),
            None => (ctx.default_offset_secs, Policies::ZONE_NAME_UNKNOWN),
        },
    };
    let hour = match p.pm {
        Some(pm) => p.hour % 12 + if pm { 12 } else { 0 },
        None => p.hour,
    };
    let build = |year: i32| -> Option<EpochNanos> {
        let (month, day) = match p.yday {
            Some(yd) => {
                if yd == 0 || yd > 365 + u16::from(is_leap(year)) {
                    return None;
                }
                let (_, m, d) = civil_from_days(days_from_civil(year, 1, 1) + i64::from(yd) - 1);
                (m, d)
            }
            None => (p.month, p.day),
        };
        let c = Civil {
            year,
            month,
            day,
            hour,
            minute: p.minute,
            second: p.second,
            nanos: p.nanos,
        };
        epoch_from_civil(&c, offset)
    };
    let epoch_nanos = match p.year {
        Some(y) => build(y),
        None => {
            policies = policies.union(Policies::YEAR_ASSUMED);
            let local = ctx
                .receipt_epoch_nanos
                .saturating_add(i64::from(offset) * NANOS_PER_SEC);
            let y = civil_from_epoch(local).year;
            match build(y) {
                Some(e) if e <= ctx.receipt_epoch_nanos.saturating_add(YEAR_ROLLOVER_SLACK) => {
                    Some(e)
                }
                // more than 7 days in the future, or a Feb 29 the receipt year lacks
                _ => build(y - 1),
            }
        }
    };
    epoch_nanos
        .map(|epoch_nanos| Timestamp {
            epoch_nanos,
            policies,
        })
        .ok_or(TimeError::OutOfRange)
}

pub fn parse(input: &[u8], format: &Format, ctx: &Context) -> Result<Timestamp, TimeError> {
    let s = std::str::from_utf8(input)
        .map_err(|_| TimeError::NoMatch)?
        .trim_matches(|c: char| c.is_ascii_whitespace())
        .as_bytes();
    if s.is_empty() {
        return Err(TimeError::Empty);
    }
    let parts = match format {
        Format::Auto => auto(s),
        Format::Rfc3339 => rfc3339(s).ok_or(TimeError::NoMatch),
        Format::Syslog => syslog(s).ok_or(TimeError::NoMatch),
        Format::Ctime => ctime(s).ok_or(TimeError::NoMatch),
        Format::EpochSecs => epoch(s, Some(NANOS_PER_SEC)),
        Format::EpochMillis => epoch(s, Some(1_000_000)),
        Format::EpochMicros => epoch(s, Some(1_000)),
        Format::EpochNanos => epoch(s, Some(1)),
        Format::Strftime(layout) => strftime(layout.as_bytes(), s).ok_or(TimeError::NoMatch),
    }?;
    resolve(parts, ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Walk every day from 1970-01-01 to the last day representable in i64 nanos with a
    // naive counter and check both directions of the calendar math against it.
    #[test]
    fn civil_epoch_round_trip_every_day() {
        let (mut y, mut m, mut d) = (1970, 1u8, 1u8);
        let mut day = 0i64;
        loop {
            let nanos = day * DAY_SECS * NANOS_PER_SEC + 45_296 * NANOS_PER_SEC + 123_456_789;
            let c = civil_from_epoch(nanos);
            assert_eq!((c.year, c.month, c.day), (y, m, d), "day {day}");
            assert_eq!(
                (c.hour, c.minute, c.second, c.nanos),
                (12, 34, 56, 123_456_789)
            );
            assert_eq!(epoch_from_civil(&c, 0), Some(nanos), "day {day}");
            assert_eq!(days_from_civil(y, m, d), day);
            d += 1;
            if d > days_in_month(y, m) {
                d = 1;
                m += 1;
                if m > 12 {
                    m = 1;
                    y += 1;
                }
            }
            day += 1;
            if y == 2262 && m == 4 && d == 12 {
                break;
            }
        }
        assert_eq!(day, 106_752);
    }

    #[test]
    fn offsets_and_negative_nanos() {
        let c = Civil {
            year: 1970,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            nanos: 0,
        };
        assert_eq!(epoch_from_civil(&c, 19_800), Some(-19_800 * NANOS_PER_SEC));
        assert_eq!(epoch_from_civil(&c, -3600), Some(3600 * NANOS_PER_SEC));
        let mut s = String::new();
        format_rfc3339(-1, &mut s);
        assert_eq!(s, "1969-12-31T23:59:59.999Z");
        assert_eq!(epoch_millis(-1), -1);
        assert_eq!(epoch_millis(1_999_999), 1);
        s.clear();
        format_rfc3339(i64::MAX, &mut s);
        assert_eq!(s, "2262-04-11T23:47:16.854Z");
    }

    #[test]
    fn policies_names_and_ops() {
        let p = Policies::YEAR_ASSUMED.union(Policies::ZONE_NAME_AMBIGUOUS);
        assert!(p.contains(Policies::YEAR_ASSUMED));
        assert!(!p.contains(Policies::TZ_ASSUMED));
        assert!(Policies::NONE.is_empty() && !p.is_empty());
        assert_eq!(
            p.names().collect::<Vec<_>>(),
            ["year_assumed", "zone_name_ambiguous"]
        );
        assert_eq!(
            Policies::RECEIPT_FALLBACK.names().collect::<Vec<_>>(),
            ["receipt_fallback"]
        );
        assert_eq!(Policies::default(), Policies::NONE);
    }

    #[test]
    fn format_spec() {
        assert_eq!(Format::from_spec("auto"), Ok(Format::Auto));
        assert_eq!(Format::from_spec("epoch_ms"), Ok(Format::EpochMillis));
        assert_eq!(
            Format::from_spec("%Y-%m-%d"),
            Ok(Format::Strftime("%Y-%m-%d".into()))
        );
        assert!(Format::from_spec("%Y-%Q").is_err());
        assert!(Format::from_spec("%Y-%").is_err());
        assert!(Format::from_spec("bogus").is_err());
        assert_eq!(TimeError::OutOfRange.reason(), "out_of_range");
    }
}
