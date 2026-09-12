//! Renders a launch alert.
//!
//! Unreadable values print as `unknown`, never as `0` — a trader reading
//! "Holders: 0" concludes something very different from "Holders: unknown",
//! and only one of those is true when an RPC call fails.

use alloy::primitives::Address;

use crate::risk::{Assessment, Signals};

#[derive(Debug, Clone)]
pub struct LaunchAlert {
    pub token: Address,
    pub symbol: Option<String>,
    pub pool: Address,
    pub signals: Signals,
    pub assessment: Assessment,
}

pub fn render(alert: &LaunchAlert) -> String {
    let mut out = String::new();

    let name = alert.symbol.as_deref().unwrap_or("unknown symbol");
    out.push_str(&format!("🚨 New token: {name}\n"));
    out.push_str(&format!("Address: {}\n", alert.token));
    out.push_str(&format!("Liquidity: {}\n", fmt_usd(alert.signals.liquidity_usd)));
    out.push_str(&format!("Holders: {}\n", fmt_u64(alert.signals.holder_count)));
    out.push_str(&format!("Buys: {}\n", fmt_u64(alert.signals.buys)));
    out.push_str(&format!("Sells: {}\n", fmt_u64(alert.signals.sells)));
    out.push_str(&format!(
        "Contract risk: {} ({}/100)\n",
        alert.assessment.band.label(),
        alert.assessment.score
    ));

    if !alert.assessment.reasons.is_empty() {
        out.push_str("Flags:\n");
        for reason in &alert.assessment.reasons {
            out.push_str(&format!("  - {reason}\n"));
        }
    }

    if !alert.assessment.unknowns.is_empty() {
        out.push_str("Could not verify:\n");
        for unknown in &alert.assessment.unknowns {
            out.push_str(&format!("  - {unknown}\n"));
        }
    }

    out.push_str(&format!("Pool: {}\n", alert.pool));
    out
}

fn fmt_usd(v: Option<f64>) -> String {
    match v {
        None => "unknown".to_string(),
        Some(v) if v >= 1_000_000.0 => format!("${:.1}m", v / 1_000_000.0),
        Some(v) if v >= 1_000.0 => format!("${:.0}k", v / 1_000.0),
        Some(v) => format!("${v:.0}"),
    }
}

fn fmt_u64(v: Option<u64>) -> String {
    match v {
        Some(v) => v.to_string(),
        None => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::{assess, Band};

    fn alert_with(signals: Signals) -> LaunchAlert {
        let assessment = assess(&signals);
        LaunchAlert {
            token: Address::repeat_byte(0xaa),
            symbol: Some("TEST".to_string()),
            pool: Address::repeat_byte(0xbb),
            signals,
            assessment,
        }
    }

    #[test]
    fn renders_the_expected_card_shape() {
        let signals = Signals {
            liquidity_usd: Some(42_000.0),
            holder_count: Some(87),
            top_holder_share: Some(0.05),
            top10_share: Some(0.3),
            creator_share: Some(0.01),
            lp_burned_or_locked: Some(true),
            buys: Some(132),
            sells: Some(41),
        };
        let out = render(&alert_with(signals));

        assert!(out.contains("🚨 New token: TEST"));
        assert!(out.contains("Liquidity: $42k"));
        assert!(out.contains("Holders: 87"));
        assert!(out.contains("Buys: 132"));
        assert!(out.contains("Sells: 41"));
        assert!(out.contains("Contract risk:"));
    }

    #[test]
    fn unknown_values_render_as_unknown_not_zero() {
        let signals = Signals {
            liquidity_usd: None,
            holder_count: None,
            buys: None,
            sells: None,
            ..Default::default()
        };
        let out = render(&alert_with(signals));

        assert!(out.contains("Liquidity: unknown"));
        assert!(out.contains("Holders: unknown"));
        assert!(out.contains("Buys: unknown"));
        assert!(out.contains("Sells: unknown"));
        assert!(!out.contains("Holders: 0"));
        assert!(out.contains("Could not verify:"));
    }

    #[test]
    fn zero_activity_renders_as_zero_not_unknown() {
        let signals = Signals {
            liquidity_usd: Some(5_000.0),
            holder_count: Some(2),
            top_holder_share: Some(0.5),
            lp_burned_or_locked: Some(true),
            buys: Some(0),
            sells: Some(0),
            ..Default::default()
        };
        let out = render(&alert_with(signals));
        assert!(out.contains("Buys: 0"));
        assert!(out.contains("Sells: 0"));
    }

    #[test]
    fn risky_launch_lists_its_flags() {
        let signals = Signals {
            liquidity_usd: Some(900.0),
            holder_count: Some(4),
            top_holder_share: Some(0.85),
            top10_share: Some(0.99),
            creator_share: Some(0.7),
            lp_burned_or_locked: Some(false),
            buys: Some(2),
            sells: Some(1),
        };
        let alert = alert_with(signals);
        assert_eq!(alert.assessment.band, Band::Critical);

        let out = render(&alert);
        assert!(out.contains("Flags:"));
        assert!(out.contains("LP not burned"));
    }

    #[test]
    fn missing_symbol_is_labelled() {
        let mut alert = alert_with(Signals::default());
        alert.symbol = None;
        assert!(render(&alert).contains("unknown symbol"));
    }

    #[test]
    fn formats_large_liquidity_in_millions() {
        let signals = Signals { liquidity_usd: Some(2_500_000.0), ..Default::default() };
        assert!(render(&alert_with(signals)).contains("Liquidity: $2.5m"));
    }

    #[test]
    fn formats_small_liquidity_in_dollars() {
        let signals = Signals { liquidity_usd: Some(420.0), ..Default::default() };
        assert!(render(&alert_with(signals)).contains("Liquidity: $420"));
    }
}
