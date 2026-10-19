//! Formatting LLM spend for the Usage page and summaries.

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
