// NSPR's PR_ParseTimeString (nsprpub/pr/src/misc/prtime.c), as Gecko's CookieParser::GetExpiry
// calls it for a cookie's Expires attribute: default_to_gmt true, so a string without a zone is
// GMT and the local-time branch is never taken. The parser is a loose scan for a month name, a
// day, a year, HH:MM[:SS], a zone name or a numeric offset, in any order; what it does not know
// it skips. Ported line by line, so the odd strings sites write expire where Firefox has them.

#[derive(Clone, Copy, PartialEq)]
enum Zone {
    Unknown,
    Pst,
    Pdt,
    Mst,
    Mdt,
    Cst,
    Cdt,
    Est,
    Edt,
    Ast,
    Nst,
    Gmt,
    Bst,
    Met,
    Eet,
    Jst,
}

/// Microseconds since the epoch (PR_ImplodeTime of the parsed fields), or None where NSPR fails.
pub fn parse_time_string(string: &[u8]) -> Option<i64> {
    // C reads a NUL past the end; so does `at`
    let at = |i: usize| -> u8 { string.get(i).copied().unwrap_or(0) };
    let lower = |c: u8| c.to_ascii_lowercase();
    let is3 = |i: usize, b: u8, c: u8| lower(at(i + 1)) == b && lower(at(i + 2)) == c;
    let digit = |c: u8| c.is_ascii_digit();
    let num = |c: u8| (c - b'0') as i64;

    let mut dotw = false;
    let mut month: i64 = -1;
    let mut zone = Zone::Unknown;
    let mut zone_offset: i64 = -1;
    let mut dst_offset: i64 = 0;
    let mut date: i64 = -1;
    let mut year: i64 = -1;
    let mut hour: i64 = -1;
    let mut min: i64 = -1;
    let mut sec: i64 = -1;

    let mut rest = 0usize;
    let mut iterations = 0;
    while at(rest) != 0 {
        iterations += 1;
        if iterations > 1001 {
            return None;
        }
        let c = at(rest);
        match lower(c) {
            b'a' => {
                if month < 0 && is3(rest, b'p', b'r') {
                    month = 3;
                } else if zone == Zone::Unknown && is3(rest, b's', b't') {
                    zone = Zone::Ast;
                } else if month < 0 && is3(rest, b'u', b'g') {
                    month = 7;
                }
            }
            b'b' => {
                if zone == Zone::Unknown && is3(rest, b's', b't') {
                    zone = Zone::Bst;
                }
            }
            b'c' => {
                if zone == Zone::Unknown && is3(rest, b'd', b't') {
                    zone = Zone::Cdt;
                } else if zone == Zone::Unknown && is3(rest, b's', b't') {
                    zone = Zone::Cst;
                }
            }
            b'd' => {
                if month < 0 && is3(rest, b'e', b'c') {
                    month = 11;
                }
            }
            b'e' => {
                if zone == Zone::Unknown && is3(rest, b'd', b't') {
                    zone = Zone::Edt;
                } else if zone == Zone::Unknown && is3(rest, b'e', b't') {
                    zone = Zone::Eet;
                } else if zone == Zone::Unknown && is3(rest, b's', b't') {
                    zone = Zone::Est;
                }
            }
            b'f' => {
                if month < 0 && is3(rest, b'e', b'b') {
                    month = 1;
                } else if !dotw && is3(rest, b'r', b'i') {
                    dotw = true;
                }
            }
            b'g' => {
                if zone == Zone::Unknown && is3(rest, b'm', b't') {
                    zone = Zone::Gmt;
                }
            }
            b'j' => {
                if month < 0 && is3(rest, b'a', b'n') {
                    month = 0;
                } else if zone == Zone::Unknown && is3(rest, b's', b't') {
                    zone = Zone::Jst;
                } else if month < 0 && is3(rest, b'u', b'l') {
                    month = 6;
                } else if month < 0 && is3(rest, b'u', b'n') {
                    month = 5;
                }
            }
            b'm' => {
                if month < 0 && is3(rest, b'a', b'r') {
                    month = 2;
                } else if month < 0 && is3(rest, b'a', b'y') {
                    month = 4;
                } else if zone == Zone::Unknown && is3(rest, b'd', b't') {
                    zone = Zone::Mdt;
                } else if zone == Zone::Unknown && is3(rest, b'e', b't') {
                    zone = Zone::Met;
                } else if !dotw && is3(rest, b'o', b'n') {
                    dotw = true;
                } else if zone == Zone::Unknown && is3(rest, b's', b't') {
                    zone = Zone::Mst;
                }
            }
            b'n' => {
                if month < 0 && is3(rest, b'o', b'v') {
                    month = 10;
                } else if zone == Zone::Unknown && is3(rest, b's', b't') {
                    zone = Zone::Nst;
                }
            }
            b'o' => {
                if month < 0 && is3(rest, b'c', b't') {
                    month = 9;
                }
            }
            b'p' => {
                if zone == Zone::Unknown && is3(rest, b'd', b't') {
                    zone = Zone::Pdt;
                } else if zone == Zone::Unknown && is3(rest, b's', b't') {
                    zone = Zone::Pst;
                }
            }
            b's' => {
                if !dotw && is3(rest, b'a', b't') {
                    dotw = true;
                } else if month < 0 && is3(rest, b'e', b'p') {
                    month = 8;
                } else if !dotw && is3(rest, b'u', b'n') {
                    dotw = true;
                }
            }
            b't' => {
                if !dotw && (is3(rest, b'h', b'u') || is3(rest, b'u', b'e')) {
                    dotw = true;
                }
            }
            b'u' => {
                // UT is the same as GMT but UTx is not
                if zone == Zone::Unknown && lower(at(rest + 1)) == b't' && !at(rest + 2).is_ascii_alphabetic() {
                    zone = Zone::Gmt;
                }
            }
            b'w' => {
                if !dotw && is3(rest, b'e', b'd') {
                    dotw = true;
                }
            }
            b'+' | b'-' => 'sign: {
                if zone_offset != -1 {
                    // already got one
                    rest += 1;
                    break 'sign;
                }
                if zone != Zone::Unknown && zone != Zone::Gmt {
                    // GMT+0300 is legal, but PST+0300 is not
                    rest += 1;
                    break 'sign;
                }
                let sign = if c == b'+' { 1 } else { -1 };
                rest += 1;
                let mut end = rest;
                while digit(at(end)) {
                    end += 1;
                }
                if rest == end {
                    break 'sign;
                }
                zone_offset = match end - rest {
                    4 => (num(at(rest)) * 10 + num(at(rest + 1))) * 60 + num(at(rest + 2)) * 10 + num(at(rest + 3)),
                    2 => (num(at(rest)) * 10 + num(at(rest + 1))) * 60,
                    1 => num(at(rest)) * 60,
                    _ => break 'sign,
                };
                zone_offset *= sign;
                zone = Zone::Gmt;
            }
            b'0'..=b'9' => 'digits: {
                let mut end = rest + 1;
                while digit(at(end)) {
                    end += 1;
                }
                // end is now the first character after a range of digits
                if at(end) == b':' {
                    if hour >= 0 && min >= 0 {
                        break 'digits;
                    }
                    // HH:MM[:SS]
                    if end - rest > 2 {
                        break 'digits;
                    }
                    let mut tmp_hour = if end - rest == 2 { num(at(rest)) * 10 + num(at(rest + 1)) } else { num(at(rest)) };
                    end += 1;
                    rest = end;
                    while digit(at(end)) {
                        end += 1;
                    }
                    if end == rest || end - rest > 2 {
                        break 'digits;
                    }
                    let tmp_min = if end - rest == 2 { num(at(rest)) * 10 + num(at(rest + 1)) } else { num(at(rest)) };
                    rest = end;
                    if at(rest) == b':' {
                        rest += 1;
                    }
                    end = rest;
                    while digit(at(end)) {
                        end += 1;
                    }
                    let mut tmp_sec = -1;
                    if end == rest {
                        // no digits after the second colon: fine
                    } else if end - rest > 2 {
                        break 'digits;
                    } else if end - rest == 2 {
                        tmp_sec = num(at(rest)) * 10 + num(at(rest + 1));
                    } else {
                        tmp_sec = num(at(rest));
                    }
                    // an AM or PM directly after the time
                    if tmp_hour <= 12 {
                        let mut s = end;
                        while at(s) == b' ' || at(s) == b'\t' {
                            s += 1;
                        }
                        if lower(at(s)) == b'p' && lower(at(s + 1)) == b'm' {
                            tmp_hour = if tmp_hour == 12 { 12 } else { tmp_hour + 12 };
                        } else if tmp_hour == 12 && lower(at(s)) == b'a' && lower(at(s + 1)) == b'm' {
                            tmp_hour = 0;
                        }
                    }
                    hour = tmp_hour;
                    min = tmp_min;
                    sec = tmp_sec;
                    rest = end;
                    break 'digits;
                }
                if (at(end) == b'/' || at(end) == b'-') && digit(at(end + 1)) {
                    // 6/16/95, 16/6/95, 6-16-95, 16-6-95, 95-06-05
                    if month >= 0 {
                        break 'digits;
                    }
                    let mut s = rest;
                    let mut n1 = num(at(s));
                    s += 1;
                    if digit(at(s)) {
                        n1 = n1 * 10 + num(at(s));
                        s += 1;
                    }
                    if at(s) != b'/' && at(s) != b'-' {
                        break 'digits;
                    }
                    s += 1;
                    if !digit(at(s)) {
                        break 'digits;
                    }
                    let mut n2 = num(at(s));
                    s += 1;
                    if digit(at(s)) {
                        n2 = n2 * 10 + num(at(s));
                        s += 1;
                    }
                    if at(s) != b'/' && at(s) != b'-' {
                        break 'digits;
                    }
                    s += 1;
                    if !digit(at(s)) {
                        break 'digits;
                    }
                    let mut n3 = num(at(s));
                    s += 1;
                    if digit(at(s)) {
                        n3 = n3 * 10 + num(at(s));
                        s += 1;
                    }
                    if digit(at(s)) {
                        n3 = n3 * 10 + num(at(s));
                        s += 1;
                        if !digit(at(s)) {
                            break 'digits;
                        }
                        n3 = n3 * 10 + num(at(s));
                        s += 1;
                        if digit(at(s)) {
                            n3 = n3 * 10 + num(at(s));
                            s += 1;
                        }
                    }
                    if at(s).is_ascii_alphanumeric() {
                        break 'digits;
                    }
                    if n1 > 31 || n1 == 0 {
                        // YY/MM/DD
                        if n2 > 12 || n3 > 31 {
                            break 'digits;
                        }
                        year = n1;
                        if year < 70 {
                            year += 2000;
                        } else if year < 100 {
                            year += 1900;
                        }
                        month = n2 - 1;
                        date = n3;
                        rest = s;
                        break 'digits;
                    }
                    if n1 > 12 && n2 > 12 {
                        rest = s;
                        break 'digits;
                    }
                    if n3 < 70 {
                        n3 += 2000;
                    } else if n3 < 100 {
                        n3 += 1900;
                    }
                    if n1 > 12 {
                        date = n1;
                        month = n2 - 1;
                    } else {
                        month = n1 - 1;
                        date = n2;
                    }
                    year = n3;
                    rest = s;
                } else if at(end).is_ascii_alphabetic() {
                    // digits followed by letters
                } else if end - rest == 5 || end - rest == 4 {
                    if year < 0 {
                        year = (rest..end).fold(0, |a, i| a * 10 + num(at(i)));
                    }
                } else if end - rest == 2 {
                    let n = num(at(rest)) * 10 + num(at(rest + 1));
                    if date < 0 && n < 32 {
                        date = n;
                    } else if year < 0 {
                        year = if n < 70 { 2000 + n } else { 1900 + n };
                    }
                } else if end - rest == 1 && date < 0 {
                    date = num(at(rest));
                }
            }
            _ => {}
        }

        // skip to the end of this token, whether parsed or not
        while !matches!(at(rest), 0 | b' ' | b'\t' | b',' | b';' | b'-' | b'+' | b'/' | b'(' | b')' | b'[' | b']') {
            rest += 1;
        }
        loop {
            while matches!(at(rest), b' ' | b'\t' | b',' | b';' | b'/' | b'(' | b')' | b'[' | b']') {
                rest += 1;
            }
            // "-" is ignored at the start of a token if no year is parsed yet and it follows a
            // letter (the second "-" of "30-AUG-1966"), or if no digit follows it
            if at(rest) == b'-'
                && ((rest > 0 && at(rest - 1).is_ascii_alphabetic() && year < 0) || !digit(at(rest + 1)))
            {
                rest += 1;
                continue;
            }
            break;
        }
    }

    if zone != Zone::Unknown && zone_offset == -1 {
        let (z, d) = match zone {
            Zone::Pst => (-8 * 60, 0),
            Zone::Pdt => (-8 * 60, 60),
            Zone::Mst => (-7 * 60, 0),
            Zone::Mdt => (-7 * 60, 60),
            Zone::Cst => (-6 * 60, 0),
            Zone::Cdt => (-6 * 60, 60),
            Zone::Est => (-5 * 60, 0),
            Zone::Edt => (-5 * 60, 60),
            Zone::Ast => (-4 * 60, 0),
            Zone::Nst => (-3 * 60 - 30, 0),
            Zone::Gmt => (0, 0),
            Zone::Bst => (0, 60),
            Zone::Met => (60, 0),
            Zone::Eet => (120, 0),
            Zone::Jst => (9 * 60, 0),
            Zone::Unknown => (0, 0),
        };
        zone_offset = z;
        dst_offset = d;
    }
    if month < 0 || date == -1 || year == -1 || year > i16::MAX as i64 {
        return None;
    }
    // no zone: GMT (default_to_gmt)
    if zone_offset == -1 {
        zone_offset = 0;
    }
    // PR_ImplodeTime after PR_NormalizeTime: fields past their range carry into the next
    let days = days_from_civil(year, month + 1, 1) + date - 1;
    let secs = days * 86400 + hour.max(0) * 3600 + min.max(0) * 60 + sec.max(0) - zone_offset * 60 - dst_offset * 60;
    Some(secs * 1_000_000)
}

/// Days from 1970-01-01 to the given proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

#[cfg(test)]
mod tests {
    use super::parse_time_string as p;

    #[test]
    fn formats() {
        // Thu, 01 Jan 2026 00:00:00 GMT
        assert_eq!(p(b"Thu, 01 Jan 2026 00:00:00 GMT"), Some(1767225600 * 1_000_000));
        assert_eq!(p(b"Thursday, 01-Jan-26 00:00:00 GMT"), Some(1767225600 * 1_000_000));
        assert_eq!(p(b"Thu Jan  1 00:00:00 2026"), Some(1767225600 * 1_000_000));
        assert_eq!(p(b"01 Jan 2026 01:00 +0100"), Some(1767225600 * 1_000_000));
        assert_eq!(p(b"Jan 1 2026 12:00am"), Some(1767225600 * 1_000_000));
        assert_eq!(p(b"Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        assert_eq!(p(b"garbage"), None);
        assert_eq!(p(b""), None);
    }
}
