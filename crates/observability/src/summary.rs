use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplaySummary {
    pub total: usize,
    pub sim_passed: usize,
    pub sim_failed: usize,
    pub success: usize,
    pub reverted: usize,
    pub gas_rejected: usize,
    pub mean_latency_ms: f64,
    pub total_realized_profit_usdc: f64,
}

impl ReplaySummary {
    pub fn success_rate(&self) -> f64 {
        if self.total == 0 { 0.0 } else { (self.success as f64) / (self.total as f64) }
    }
}

/// Renders a summary as Telegram HTML. Telegram only supports `<b>` and
/// `<code>` — no custom CSS, no nested tags.
pub fn render_for_telegram(s: &ReplaySummary) -> String {
    format!(
        concat!(
            "<b>Execution Observability</b>\n",
            "Events: <code>{total}</code>\n",
            "Sim passed: <code>{sim_passed}</code>  ",
            "Sim failed: <code>{sim_failed}</code>\n",
            "Gas-gate rejected: <code>{gas_rejected}</code>\n",
            "Broadcast - Success: <code>{success}</code>  ",
            "Reverted: <code>{reverted}</code>\n",
            "Success rate: <code>{rate:.2}%</code>\n",
            "Mean latency: <code>{lat:.1} ms</code>\n",
            "Realised profit: <code>${profit:.2}</code>",
        ),
        total       = s.total,
        sim_passed  = s.sim_passed,
        sim_failed  = s.sim_failed,
        gas_rejected = s.gas_rejected,
        success     = s.success,
        reverted    = s.reverted,
        rate        = s.success_rate() * 100.0,
        lat         = s.mean_latency_ms,
        profit      = s.total_realized_profit_usdc,
    )
}

impl ReplaySummary {
    pub fn render_for_telegram(&self) -> String { render_for_telegram(self) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ReplaySummary {
        ReplaySummary {
            total: 100, sim_passed: 60, sim_failed: 20,
            success: 30, reverted: 10, gas_rejected: 5,
            mean_latency_ms: 250.5,
            total_realized_profit_usdc: 1234.56,
        }
    }

    #[test]
    fn success_rate_basic() {
        assert!((sample().success_rate() - 0.30).abs() < 1e-9);
    }

    #[test]
    fn success_rate_handles_empty() {
        let s = ReplaySummary { total: 0, sim_passed: 0, sim_failed: 0,
            success: 0, reverted: 0, gas_rejected: 0,
            mean_latency_ms: 0.0, total_realized_profit_usdc: 0.0 };
        assert_eq!(s.success_rate(), 0.0);
    }

    #[test]
    fn render_for_telegram_includes_all_fields() {
        let rendered = render_for_telegram(&sample());
        assert!(rendered.contains("<b>Execution Observability</b>"));
        assert!(rendered.contains("<code>100</code>"));
        assert!(rendered.contains("30.00%"));
        assert!(rendered.contains("250.5 ms"));
        assert!(rendered.contains("$1234.56"));
    }

    #[test]
    fn render_method_equivalent_to_free_function() {
        let s = sample();
        assert_eq!(s.render_for_telegram(), render_for_telegram(&s));
    }

    #[test]
    fn render_for_telegram_empty_summary() {
        let s = ReplaySummary { total: 0, sim_passed: 0, sim_failed: 0,
            success: 0, reverted: 0, gas_rejected: 0,
            mean_latency_ms: 0.0, total_realized_profit_usdc: 0.0 };
        let rendered = render_for_telegram(&s);
        assert!(rendered.contains("0.00%"));
        assert!(rendered.contains("$0.00"));
    }
}
