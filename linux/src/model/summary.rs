//! Run progress and the result summary of a core `batch`.

use serde_json::Value;
use std::path::PathBuf;

/// Latest progress the core reported.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProgressSnapshot {
    pub done: usize,
    pub total: usize,
    pub current: String,
}

impl ProgressSnapshot {
    pub fn fraction(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.done.min(self.total) as f64) / (self.total as f64)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub source: String,
    pub detail: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunSummary {
    pub converted: usize,
    pub resumed: usize,
    pub copied: usize,
    pub skipped: usize,
    pub failed: usize,
    pub cancelled: bool,
    pub report: Option<PathBuf>,
    pub failures: Vec<Failure>,
}

impl RunSummary {
    /// Read the `batch` reply (`counts`, `cancelled`, `report`,
    /// `report_data.files`).
    pub fn from_result(v: &Value) -> RunSummary {
        let n = |k: &str| v["counts"][k].as_u64().unwrap_or(0) as usize;
        let failures = v["report_data"]["files"]
            .as_array()
            .map(|files| {
                files
                    .iter()
                    .filter(|f| f["status"] == "failed")
                    .map(|f| Failure {
                        source: f["source"].as_str().unwrap_or("").to_string(),
                        detail: f["detail"].as_str().unwrap_or("").to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        RunSummary {
            converted: n("converted"),
            resumed: n("resumed"),
            copied: n("copied"),
            skipped: n("skipped"),
            failed: n("failed"),
            cancelled: v["cancelled"] == true,
            report: v["report"].as_str().map(PathBuf::from),
            failures,
        }
    }

    /// Results written or verified in the output folder.
    pub fn produced(&self) -> usize {
        self.converted + self.resumed + self.copied
    }

    pub fn ok(&self) -> bool {
        self.failed == 0 && !self.cancelled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_batch_reply() {
        let v = json!({
            "counts": {"converted": 3, "resumed": 1, "copied": 0, "skipped": 2, "failed": 1},
            "cancelled": false,
            "report": "/out/report-x.json",
            "report_data": {"files": [
                {"status": "converted", "source": "/in/a.jpg"},
                {"status": "failed", "source": "/in/b.jpg", "detail": "封面时间无效"}
            ]}
        });
        let s = RunSummary::from_result(&v);
        assert_eq!(s.produced(), 4);
        assert_eq!(s.skipped, 2);
        assert!(!s.ok());
        assert_eq!(s.report, Some(PathBuf::from("/out/report-x.json")));
        assert_eq!(
            s.failures,
            vec![Failure {
                source: "/in/b.jpg".into(),
                detail: "封面时间无效".into()
            }]
        );
    }

    #[test]
    fn empty_reply_is_zero() {
        let s = RunSummary::from_result(&json!({}));
        assert_eq!(s, RunSummary::default());
        assert!(s.ok());
    }

    #[test]
    fn progress_fraction_is_clamped() {
        let p = ProgressSnapshot {
            done: 5,
            total: 4,
            current: String::new(),
        };
        assert_eq!(p.fraction(), 1.0);
        assert_eq!(ProgressSnapshot::default().fraction(), 0.0);
    }
}
