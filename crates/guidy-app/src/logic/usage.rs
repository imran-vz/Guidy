//! Formatting AI usage (cloud spend, local model work) for the Usage page
//! and summaries.

use guidy_core::llm::usage::MeterTotals;

/// `$1.23`; amounts under a dollar keep two significant digits (`$0.012`,
/// `$0.0042`).
pub fn format_usd(usd: f64) -> String {
    if usd <= 0.0 {
        "$0.00".into()
    } else if usd < 1.0 {
        let decimals = ((-usd.log10()).floor() as usize + 2).max(2);
        format!("${usd:.decimals$}")
    } else {
        format!("${usd:.2}")
    }
}

/// `950`, `12.3k`, `4.1M`.
pub fn format_tokens(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => trim_zero(format!("{:.1}", n as f64 / 1e3)) + "k",
        _ => trim_zero(format!("{:.1}", n as f64 / 1e6)) + "M",
    }
}

/// Audio or compute time: `45 s`, `12 min`, `3 h 12 min`.
pub fn format_duration(ms: u64) -> String {
    let secs = (ms + 500) / 1000;
    match secs {
        0..=59 => format!("{secs} s"),
        60..=3_599 => format!("{} min", (secs + 30) / 60),
        _ => {
            let mins = (secs + 30) / 60;
            match mins % 60 {
                0 => format!("{} h", mins / 60),
                m => format!("{} h {m} min", mins / 60),
            }
        }
    }
}

/// A short wait: `350 ms`, `1.2 s`.
pub fn format_wait(ms: u64) -> String {
    if ms < 1_000 {
        format!("{ms} ms")
    } else {
        trim_zero(format!("{:.1}", ms as f64 / 1e3)) + " s"
    }
}

/// How many times real time: `24×`, `3.5×`.
pub fn format_speedup(x: f64) -> String {
    format!("{}×", one_decimal_below_ten(x))
}

/// Tokens per second: `28 tok/s`, `7.5 tok/s`.
pub fn format_tokens_per_second(x: f64) -> String {
    format!("{} tok/s", one_decimal_below_ten(x))
}

fn one_decimal_below_ten(x: f64) -> String {
    if x < 10.0 {
        trim_zero(format!("{x:.1}"))
    } else {
        format!("{x:.0}")
    }
}

fn trim_zero(s: String) -> String {
    s.strip_suffix(".0").map(str::to_string).unwrap_or(s)
}

/// The cost of a job (`~` marks registry estimates), `None` when nothing
/// was recorded or no call could be priced.
pub fn format_cost(u: &MeterTotals) -> Option<String> {
    let usd = u.cost_usd.filter(|_| u.calls > 0)?;
    Some(format!(
        "{}{}",
        if u.estimated { "~" } else { "" },
        format_usd(usd)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_and_tokens_read_well() {
        assert_eq!(format_usd(0.0), "$0.00");
        assert_eq!(format_usd(0.0042), "$0.0042");
        assert_eq!(format_usd(0.000123), "$0.00012");
        assert_eq!(format_usd(0.0123), "$0.012");
        assert_eq!(format_usd(0.25), "$0.25");
        assert_eq!(format_usd(1.234), "$1.23");
        assert_eq!(format_tokens(950), "950");
        assert_eq!(format_tokens(12_345), "12.3k");
        assert_eq!(format_tokens(2_000), "2k");
        assert_eq!(format_tokens(4_100_000), "4.1M");
    }

    #[test]
    fn local_work_reads_well() {
        assert_eq!(format_duration(0), "0 s");
        assert_eq!(format_duration(44_600), "45 s");
        assert_eq!(format_duration(12 * 60_000 + 10_000), "12 min");
        assert_eq!(format_duration(3 * 3_600_000), "3 h");
        assert_eq!(format_duration(3 * 3_600_000 + 12 * 60_000), "3 h 12 min");
        assert_eq!(format_wait(350), "350 ms");
        assert_eq!(format_wait(1_200), "1.2 s");
        assert_eq!(format_wait(2_000), "2 s");
        assert_eq!(format_speedup(24.4), "24×");
        assert_eq!(format_speedup(3.46), "3.5×");
        assert_eq!(format_tokens_per_second(28.2), "28 tok/s");
        assert_eq!(format_tokens_per_second(7.0), "7 tok/s");
    }

    #[test]
    fn job_cost() {
        let u = MeterTotals {
            calls: 2,
            input_tokens: 12_300,
            output_tokens: 850,
            cost_usd: Some(0.0123),
            estimated: true,
        };
        assert_eq!(format_cost(&u).unwrap(), "~$0.012");
        let reported = MeterTotals {
            estimated: false,
            ..u
        };
        assert_eq!(format_cost(&reported).unwrap(), "$0.012");
        assert_eq!(format_cost(&MeterTotals::default()), None);
    }
}
