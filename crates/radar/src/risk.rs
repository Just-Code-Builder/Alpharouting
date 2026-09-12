//! Risk scoring for a freshly launched token.
//!
//! Design rule that matters most here: **an unknown signal is never treated
//! as a safe one.** A scanner that reports "Low risk" because it failed to
//! read the liquidity is worse than useless — it launders ignorance as
//! reassurance. Unknown critical signals therefore floor the reported band
//! at `Medium` and are listed explicitly in `unknowns`.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Band {
    Low,
    Medium,
    High,
    Critical,
}

impl Band {
    pub fn label(self) -> &'static str {
        match self {
            Band::Low => "Low",
            Band::Medium => "Medium",
            Band::High => "High",
            Band::Critical => "Critical",
        }
    }
}

/// Everything the scanner managed to observe. `None` means "could not
/// determine", which is materially different from a zero value.
#[derive(Debug, Clone, Default)]
pub struct Signals {
    /// Quote-side liquidity in the pool, in USD.
    pub liquidity_usd: Option<f64>,
    pub holder_count: Option<u64>,
    /// Largest single holder's share of supply, 0.0..=1.0, with the pool
    /// itself and burn addresses excluded.
    pub top_holder_share: Option<f64>,
    pub top10_share: Option<f64>,
    /// Share still held by the deploying wallet.
    pub creator_share: Option<f64>,
    /// Whether LP tokens are burned or time-locked. `Some(false)` means the
    /// deployer can still pull liquidity.
    pub lp_burned_or_locked: Option<bool>,
    pub buys: Option<u64>,
    pub sells: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Assessment {
    /// 0..=100, higher is riskier.
    pub score: u32,
    pub band: Band,
    /// Human-readable findings that drove the score.
    pub reasons: Vec<String>,
    /// Signals that could not be determined.
    pub unknowns: Vec<String>,
}

/// Critical signals: if any of these is unknown we cannot honestly call a
/// token low-risk, regardless of how the others scored.
const CRITICAL_UNKNOWN_FLOOR: Band = Band::Medium;

pub fn assess(signals: &Signals) -> Assessment {
    let mut score: u32 = 0;
    let mut reasons = Vec::new();
    let mut unknowns = Vec::new();

    match signals.liquidity_usd {
        Some(liq) if liq < 5_000.0 => {
            score += 30;
            reasons.push(format!("liquidity only ${:.0} — trivially drainable", liq));
        }
        Some(liq) if liq < 25_000.0 => {
            score += 15;
            reasons.push(format!("thin liquidity (${:.0})", liq));
        }
        Some(liq) if liq < 100_000.0 => {
            score += 5;
            reasons.push(format!("moderate liquidity (${:.0})", liq));
        }
        Some(_) => {}
        None => unknowns.push("liquidity could not be read".to_string()),
    }

    match signals.holder_count {
        Some(n) if n < 25 => {
            score += 20;
            reasons.push(format!("only {n} holders"));
        }
        Some(n) if n < 100 => {
            score += 10;
            reasons.push(format!("{n} holders — still very early"));
        }
        Some(_) => {}
        None => unknowns.push("holder count could not be read".to_string()),
    }

    match signals.top_holder_share {
        Some(s) if s > 0.50 => {
            score += 30;
            reasons.push(format!("one wallet holds {:.0}% of supply", s * 100.0));
        }
        Some(s) if s > 0.25 => {
            score += 20;
            reasons.push(format!("top holder has {:.0}% of supply", s * 100.0));
        }
        Some(s) if s > 0.10 => {
            score += 10;
            reasons.push(format!("top holder has {:.0}% of supply", s * 100.0));
        }
        Some(_) => {}
        None => unknowns.push("holder distribution could not be read".to_string()),
    }

    // Only meaningful once there are more than 10 holders: below that, the
    // top 10 trivially hold ~100% of supply and the signal is just the
    // holder count restated. Scoring both double-counts one fact.
    if let (Some(s), Some(n)) = (signals.top10_share, signals.holder_count) {
        if n > 10 && s > 0.80 {
            score += 15;
            reasons.push(format!("top 10 of {n} wallets hold {:.0}%", s * 100.0));
        }
    }

    if let Some(s) = signals.creator_share {
        if s > 0.20 {
            score += 20;
            reasons.push(format!("deployer still holds {:.0}%", s * 100.0));
        }
    }

    match signals.lp_burned_or_locked {
        Some(false) => {
            score += 25;
            reasons.push("LP not burned or locked — deployer can pull liquidity".to_string());
        }
        Some(true) => {}
        None => unknowns.push("LP lock/burn status could not be determined".to_string()),
    }

    match (signals.buys, signals.sells) {
        (Some(0), Some(0)) => {
            score += 5;
            reasons.push("no trading activity yet".to_string());
        }
        (Some(b), Some(s)) if b > 0 && s > b.saturating_mul(3) => {
            score += 10;
            reasons.push(format!("sell pressure: {s} sells vs {b} buys"));
        }
        (None, _) | (_, None) => unknowns.push("swap activity could not be read".to_string()),
        _ => {}
    }

    let score = score.min(100);
    let mut band = band_for(score);

    // An unknown critical signal caps how good the rating may look.
    let critical_unknown = signals.liquidity_usd.is_none()
        || signals.top_holder_share.is_none()
        || signals.lp_burned_or_locked.is_none();
    if critical_unknown && band < CRITICAL_UNKNOWN_FLOOR {
        band = CRITICAL_UNKNOWN_FLOOR;
    }

    Assessment { score, band, reasons, unknowns }
}

fn band_for(score: u32) -> Band {
    match score {
        0..=19 => Band::Low,
        20..=44 => Band::Medium,
        45..=69 => Band::High,
        _ => Band::Critical,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy() -> Signals {
        Signals {
            liquidity_usd: Some(250_000.0),
            holder_count: Some(1_500),
            top_holder_share: Some(0.04),
            top10_share: Some(0.25),
            creator_share: Some(0.01),
            lp_burned_or_locked: Some(true),
            buys: Some(400),
            sells: Some(180),
        }
    }

    #[test]
    fn healthy_token_scores_low() {
        let a = assess(&healthy());
        assert_eq!(a.band, Band::Low);
        assert_eq!(a.score, 0);
        assert!(a.unknowns.is_empty());
    }

    #[test]
    fn tiny_liquidity_and_concentration_scores_critical() {
        let signals = Signals {
            liquidity_usd: Some(1_200.0),
            holder_count: Some(9),
            top_holder_share: Some(0.72),
            top10_share: Some(0.95),
            creator_share: Some(0.60),
            lp_burned_or_locked: Some(false),
            buys: Some(3),
            sells: Some(1),
        };
        let a = assess(&signals);
        assert_eq!(a.band, Band::Critical);
        assert_eq!(a.score, 100); // saturated
        assert!(a.reasons.iter().any(|r| r.contains("72%")));
        assert!(a.reasons.iter().any(|r| r.contains("LP not burned")));
    }

    #[test]
    fn unknown_critical_signals_never_report_low() {
        // Everything observable looks fine, but liquidity is unreadable.
        let signals = Signals { liquidity_usd: None, ..healthy() };
        let a = assess(&signals);
        assert_eq!(a.band, Band::Medium, "unknown liquidity must not read as Low");
        assert!(a.unknowns.iter().any(|u| u.contains("liquidity")));
    }

    #[test]
    fn unknown_lp_status_never_reports_low() {
        let signals = Signals { lp_burned_or_locked: None, ..healthy() };
        assert_eq!(assess(&signals).band, Band::Medium);
    }

    #[test]
    fn unknown_holder_distribution_never_reports_low() {
        let signals = Signals { top_holder_share: None, ..healthy() };
        assert_eq!(assess(&signals).band, Band::Medium);
    }

    #[test]
    fn all_signals_unknown_is_medium_not_low() {
        let a = assess(&Signals::default());
        assert_eq!(a.band, Band::Medium);
        assert_eq!(a.unknowns.len(), 5);
    }

    #[test]
    fn unknown_floor_does_not_downgrade_a_worse_band() {
        // Critical on the signals we could read; the unknown must not pull
        // the rating *down* to Medium.
        let signals = Signals {
            liquidity_usd: None,
            holder_count: Some(3),
            top_holder_share: Some(0.9),
            top10_share: Some(0.99),
            creator_share: Some(0.9),
            lp_burned_or_locked: Some(false),
            buys: Some(1),
            sells: Some(9),
        };
        assert_eq!(assess(&signals).band, Band::Critical);
    }

    #[test]
    fn no_activity_is_flagged_separately_from_unknown_activity() {
        let quiet = Signals { buys: Some(0), sells: Some(0), ..healthy() };
        let a = assess(&quiet);
        assert!(a.reasons.iter().any(|r| r.contains("no trading activity")));
        assert!(a.unknowns.is_empty());

        let unread = Signals { buys: None, sells: None, ..healthy() };
        let b = assess(&unread);
        assert!(b.unknowns.iter().any(|u| u.contains("swap activity")));
    }

    #[test]
    fn top10_share_is_not_scored_when_there_are_fewer_than_ten_holders() {
        // 6 holders necessarily means the top 10 hold 100% — scoring it would
        // double-count the low holder count.
        let signals = Signals {
            liquidity_usd: Some(60_000.0),
            holder_count: Some(6),
            top_holder_share: Some(0.30),
            top10_share: Some(1.0),
            creator_share: Some(0.22),
            lp_burned_or_locked: Some(true),
            buys: Some(3),
            sells: Some(1),
        };
        let a = assess(&signals);
        assert!(
            !a.reasons.iter().any(|r| r.contains("top 10")),
            "top-10 concentration is redundant below 10 holders"
        );
        // 5 (liquidity) + 20 (holders) + 20 (top holder) + 20 (deployer) = 65
        assert_eq!(a.score, 65);
        assert_eq!(a.band, Band::High);
    }

    #[test]
    fn top10_share_is_scored_once_the_holder_base_is_wide_enough() {
        let signals = Signals {
            holder_count: Some(500),
            top10_share: Some(0.92),
            liquidity_usd: Some(250_000.0),
            top_holder_share: Some(0.05),
            creator_share: Some(0.0),
            lp_burned_or_locked: Some(true),
            buys: Some(50),
            sells: Some(20),
        };
        let a = assess(&signals);
        assert!(a.reasons.iter().any(|r| r.contains("top 10 of 500 wallets")));
        assert_eq!(a.score, 15);
    }

    #[test]
    fn heavy_sell_pressure_is_flagged() {
        let signals = Signals { buys: Some(10), sells: Some(90), ..healthy() };
        let a = assess(&signals);
        assert!(a.reasons.iter().any(|r| r.contains("sell pressure")));
    }
}
