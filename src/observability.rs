use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use serde::Serialize;
use tracing::{debug, info};

#[cfg(feature = "metrics-server")]
pub mod server;

#[derive(Debug, Default, Serialize, Clone)]
pub struct MetricsSnapshot {
    pub stages: BTreeMap<String, StageMetrics>,
    pub total_duration_ms: f64,
    pub quality_passes: u64,
    pub quality_failures: u64,
}

#[derive(Debug, Default, Serialize, Clone)]
pub struct StageMetrics {
    pub calls: u64,
    pub total_duration_ms: f64,
    pub max_duration_ms: f64,
}

#[derive(Debug, Default, Clone)]
pub struct MetricsCollector {
    inner: Arc<Mutex<MetricsSnapshot>>,
}

impl MetricsCollector {
    pub fn global() -> &'static MetricsCollector {
        static INSTANCE: Lazy<MetricsCollector> = Lazy::new(|| MetricsCollector {
            inner: Arc::new(Mutex::new(MetricsSnapshot::default())),
        });
        &INSTANCE
    }

    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(MetricsSnapshot::default())),
        }
    }

    pub fn start_stage(&self, stage_name: &str) -> StageTimer {
        StageTimer {
            stage: stage_name.to_string(),
            started_at: Instant::now(),
            collector: self.inner.clone(),
            recorded: false,
        }
    }

    pub fn record_total_duration(&self, duration: Duration) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.total_duration_ms = duration.as_secs_f64() * 1_000.0;
        }
    }

    pub fn record_quality_pass(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.quality_passes += 1;
        }
    }

    pub fn record_quality_failure(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.quality_failures += 1;
        }
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        self.inner.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn reset(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = MetricsSnapshot::default();
        }
    }
}

pub struct StageTimer {
    stage: String,
    started_at: Instant,
    collector: Arc<Mutex<MetricsSnapshot>>,
    recorded: bool,
}

impl StageTimer {
    fn record(&mut self) {
        if self.recorded {
            return;
        }
        let duration = self.started_at.elapsed();
        if let Ok(mut guard) = self.collector.lock() {
            let metrics = guard.stages.entry(self.stage.clone()).or_default();
            metrics.calls += 1;
            let duration_ms = duration.as_secs_f64() * 1_000.0;
            metrics.total_duration_ms += duration_ms;
            if duration_ms > metrics.max_duration_ms {
                metrics.max_duration_ms = duration_ms;
            }
        }
        debug!(
            stage = self.stage.as_str(),
            duration_ms = duration.as_secs_f64() * 1_000.0,
            "Stage duration recorded"
        );
        self.recorded = true;
    }
}

impl Drop for StageTimer {
    fn drop(&mut self) {
        self.record();
    }
}

pub fn log_snapshot(snapshot: &MetricsSnapshot) {
    info!(
        total_duration_ms = snapshot.total_duration_ms,
        stage_count = snapshot.stages.len(),
        quality_passes = snapshot.quality_passes,
        quality_failures = snapshot.quality_failures,
        "Pipeline metrics summary"
    );
    for (stage, metrics) in &snapshot.stages {
        info!(
            stage = stage.as_str(),
            calls = metrics.calls,
            total_ms = metrics.total_duration_ms,
            max_ms = metrics.max_duration_ms,
            "Stage metrics"
        );
    }
}

impl MetricsSnapshot {
    pub fn to_prometheus(&self) -> String {
        let mut output = String::new();
        output.push_str("# HELP bunker_quality_passes_total Total number of quality gate passes\n");
        output.push_str("# TYPE bunker_quality_passes_total counter\n");
        output.push_str(&format!(
            "bunker_quality_passes_total {}\n",
            self.quality_passes
        ));
        output.push_str(
            "# HELP bunker_quality_failures_total Total number of quality gate failures\n",
        );
        output.push_str("# TYPE bunker_quality_failures_total counter\n");
        output.push_str(&format!(
            "bunker_quality_failures_total {}\n",
            self.quality_failures
        ));
        output.push_str("# HELP bunker_stage_calls_total Stage invocation count\n");
        output.push_str("# TYPE bunker_stage_calls_total counter\n");
        output.push_str(
            "# HELP bunker_stage_duration_seconds_total Accumulated stage duration in seconds\n",
        );
        output.push_str("# TYPE bunker_stage_duration_seconds_total counter\n");
        output.push_str(
            "# HELP bunker_stage_duration_seconds_max Maximum stage duration in seconds\n",
        );
        output.push_str("# TYPE bunker_stage_duration_seconds_max gauge\n");
        for (stage, metrics) in &self.stages {
            output.push_str(&format!(
                "bunker_stage_calls_total{{stage=\"{}\"}} {}\n",
                stage, metrics.calls
            ));
            output.push_str(&format!(
                "bunker_stage_duration_seconds_total{{stage=\"{}\"}} {:.6}\n",
                stage,
                metrics.total_duration_ms / 1_000.0
            ));
            output.push_str(&format!(
                "bunker_stage_duration_seconds_max{{stage=\"{}\"}} {:.6}\n",
                stage,
                metrics.max_duration_ms / 1_000.0
            ));
        }
        output.push_str("# HELP bunker_pipeline_duration_seconds Total pipeline duration\n");
        output.push_str("# TYPE bunker_pipeline_duration_seconds gauge\n");
        output.push_str(&format!(
            "bunker_pipeline_duration_seconds {:.6}\n",
            self.total_duration_ms / 1_000.0
        ));
        output
    }
}
