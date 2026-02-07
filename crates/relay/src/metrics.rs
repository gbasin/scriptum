use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
};

use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EndpointMetricKey {
    endpoint: String,
    method: String,
}

pub struct RelayMetrics {
    request_duration_count: Mutex<HashMap<EndpointMetricKey, u64>>,
    request_duration_sum_ms: Mutex<HashMap<EndpointMetricKey, u64>>,
    request_errors_total: Mutex<HashMap<EndpointMetricKey, u64>>,
    request_rate_total: Mutex<HashMap<EndpointMetricKey, u64>>,
    ws_duration_count: Mutex<HashMap<String, u64>>,
    ws_duration_sum_ms: Mutex<HashMap<String, u64>>,
    ws_errors_total: Mutex<HashMap<String, u64>>,
    ws_rate_total: Mutex<HashMap<String, u64>>,
    daemon_recovery_time_ms: AtomicU64,
    git_sync_jobs_total: Mutex<HashMap<String, u64>>,
    outbox_depth: Mutex<HashMap<String, i64>>,
    sequence_gap_count: AtomicU64,
    sync_ack_latency_ms: AtomicU64,
}

const GIT_SYNC_STATES: [&str; 4] = ["queued", "running", "completed", "failed"];
const UNKNOWN_WORKSPACE_LABEL: &str = "unknown";
static GLOBAL_METRICS: OnceLock<Arc<RelayMetrics>> = OnceLock::new();

impl Default for RelayMetrics {
    fn default() -> Self {
        let mut git_sync_jobs_total = HashMap::new();
        for state in GIT_SYNC_STATES {
            git_sync_jobs_total.insert(state.to_string(), 0);
        }

        Self {
            request_duration_count: Mutex::new(HashMap::new()),
            request_duration_sum_ms: Mutex::new(HashMap::new()),
            request_errors_total: Mutex::new(HashMap::new()),
            request_rate_total: Mutex::new(HashMap::new()),
            ws_duration_count: Mutex::new(HashMap::new()),
            ws_duration_sum_ms: Mutex::new(HashMap::new()),
            ws_errors_total: Mutex::new(HashMap::new()),
            ws_rate_total: Mutex::new(HashMap::new()),
            daemon_recovery_time_ms: AtomicU64::new(0),
            git_sync_jobs_total: Mutex::new(git_sync_jobs_total),
            outbox_depth: Mutex::new(HashMap::new()),
            sequence_gap_count: AtomicU64::new(0),
            sync_ack_latency_ms: AtomicU64::new(0),
        }
    }
}

pub fn set_global_metrics(metrics: Arc<RelayMetrics>) {
    let _ = GLOBAL_METRICS.set(metrics);
}

fn global_metrics() -> Option<&'static Arc<RelayMetrics>> {
    GLOBAL_METRICS.get()
}

pub fn observe_sync_ack_latency_ms(latency_ms: u64) {
    if let Some(metrics) = global_metrics() {
        metrics.observe_sync_ack_latency_ms(latency_ms);
    }
}

pub fn record_ws_request(endpoint: &str, is_error: bool, latency_ms: u64) {
    if let Some(metrics) = global_metrics() {
        metrics.record_ws_request(endpoint, is_error, latency_ms);
    }
}

pub fn increment_sequence_gap_count() {
    if let Some(metrics) = global_metrics() {
        metrics.increment_sequence_gap_count();
    }
}

pub fn set_outbox_depth(outbox_depth: i64) {
    if let Some(metrics) = global_metrics() {
        metrics.set_outbox_depth(outbox_depth);
    }
}

pub fn set_outbox_depth_for_workspace(workspace_id: Uuid, outbox_depth: i64) {
    if let Some(metrics) = global_metrics() {
        metrics.set_outbox_depth_for_workspace(workspace_id, outbox_depth);
    }
}

pub fn increment_git_sync_jobs_total() {
    if let Some(metrics) = global_metrics() {
        metrics.increment_git_sync_jobs_total();
    }
}

pub fn increment_git_sync_jobs_total_for_state(state: &str) {
    if let Some(metrics) = global_metrics() {
        metrics.increment_git_sync_jobs_total_for_state(state);
    }
}

impl RelayMetrics {
    pub fn record_http_request(&self, method: &str, path: &str, status_code: u16, latency_ms: u64) {
        let key = EndpointMetricKey {
            endpoint: normalize_endpoint(path),
            method: method.to_ascii_uppercase(),
        };

        increment_counter(&self.request_rate_total, &key, 1);
        increment_counter(&self.request_duration_sum_ms, &key, latency_ms);
        increment_counter(&self.request_duration_count, &key, 1);
        if status_code >= 400 {
            increment_counter(&self.request_errors_total, &key, 1);
        }
    }

    pub fn record_ws_request(&self, endpoint: &str, is_error: bool, latency_ms: u64) {
        let normalized_endpoint = normalize_ws_endpoint(endpoint);
        increment_label_counter(&self.ws_rate_total, &normalized_endpoint, 1);
        increment_label_counter(&self.ws_duration_sum_ms, &normalized_endpoint, latency_ms);
        increment_label_counter(&self.ws_duration_count, &normalized_endpoint, 1);
        if is_error {
            increment_label_counter(&self.ws_errors_total, &normalized_endpoint, 1);
        }
    }

    pub fn set_daemon_recovery_time_ms(&self, value: u64) {
        self.daemon_recovery_time_ms.store(value, Ordering::SeqCst);
    }

    pub fn observe_sync_ack_latency_ms(&self, value: u64) {
        self.sync_ack_latency_ms.store(value, Ordering::SeqCst);
    }

    pub fn set_outbox_depth(&self, value: i64) {
        self.set_outbox_depth_by_label(UNKNOWN_WORKSPACE_LABEL, value);
    }

    pub fn set_outbox_depth_for_workspace(&self, workspace_id: Uuid, value: i64) {
        self.set_outbox_depth_by_label(&workspace_id.to_string(), value);
    }

    pub fn increment_git_sync_jobs_total(&self) {
        self.increment_git_sync_jobs_total_for_state("completed");
    }

    pub fn increment_git_sync_jobs_total_for_state(&self, state: &str) {
        let mut guard = self.git_sync_jobs_total.lock().expect("metrics map lock poisoned");
        let normalized = normalize_git_sync_state(state);
        let value = guard.entry(normalized).or_insert(0);
        *value = value.saturating_add(1);
    }

    pub fn increment_sequence_gap_count(&self) {
        self.sequence_gap_count.fetch_add(1, Ordering::SeqCst);
    }

    pub fn render_prometheus(&self) -> String {
        let mut output = String::new();

        output.push_str("# HELP relay_request_rate_total Total HTTP requests by endpoint.\n");
        output.push_str("# TYPE relay_request_rate_total counter\n");
        append_counter_lines(&mut output, "relay_request_rate_total", &self.request_rate_total);

        output.push_str(
            "# HELP relay_request_errors_total Total HTTP error responses by endpoint.\n",
        );
        output.push_str("# TYPE relay_request_errors_total counter\n");
        append_counter_lines(&mut output, "relay_request_errors_total", &self.request_errors_total);

        output.push_str("# HELP relay_request_duration_ms_sum Sum of HTTP request latency in milliseconds by endpoint.\n");
        output.push_str("# TYPE relay_request_duration_ms_sum counter\n");
        append_counter_lines(
            &mut output,
            "relay_request_duration_ms_sum",
            &self.request_duration_sum_ms,
        );

        output.push_str("# HELP relay_request_duration_ms_count Count of HTTP request latency samples by endpoint.\n");
        output.push_str("# TYPE relay_request_duration_ms_count counter\n");
        append_counter_lines(
            &mut output,
            "relay_request_duration_ms_count",
            &self.request_duration_count,
        );

        output.push_str("# HELP relay_ws_rate_total Total websocket messages by endpoint.\n");
        output.push_str("# TYPE relay_ws_rate_total counter\n");
        append_label_counter_lines(&mut output, "relay_ws_rate_total", &self.ws_rate_total);

        output
            .push_str("# HELP relay_ws_errors_total Total websocket message errors by endpoint.\n");
        output.push_str("# TYPE relay_ws_errors_total counter\n");
        append_label_counter_lines(&mut output, "relay_ws_errors_total", &self.ws_errors_total);

        output.push_str("# HELP relay_ws_duration_ms_sum Sum of websocket message latency in milliseconds by endpoint.\n");
        output.push_str("# TYPE relay_ws_duration_ms_sum counter\n");
        append_label_counter_lines(
            &mut output,
            "relay_ws_duration_ms_sum",
            &self.ws_duration_sum_ms,
        );

        output.push_str(
            "# HELP relay_ws_duration_ms_count Count of websocket latency samples by endpoint.\n",
        );
        output.push_str("# TYPE relay_ws_duration_ms_count counter\n");
        append_label_counter_lines(
            &mut output,
            "relay_ws_duration_ms_count",
            &self.ws_duration_count,
        );

        output.push_str(
            "# HELP sync_ack_latency_ms Last observed sync ack latency in milliseconds.\n",
        );
        output.push_str("# TYPE sync_ack_latency_ms gauge\n");
        output.push_str(&format!(
            "sync_ack_latency_ms {}\n",
            self.sync_ack_latency_ms.load(Ordering::SeqCst)
        ));

        output.push_str("# HELP outbox_depth Current daemon outbox depth per workspace.\n");
        output.push_str("# TYPE outbox_depth gauge\n");
        append_outbox_depth_lines(&mut output, &self.outbox_depth);

        output.push_str(
            "# HELP daemon_recovery_time_ms Relay startup recovery time in milliseconds.\n",
        );
        output.push_str("# TYPE daemon_recovery_time_ms gauge\n");
        output.push_str(&format!(
            "daemon_recovery_time_ms {}\n",
            self.daemon_recovery_time_ms.load(Ordering::SeqCst)
        ));

        output.push_str("# HELP git_sync_jobs_total Total git sync jobs processed by state.\n");
        output.push_str("# TYPE git_sync_jobs_total counter\n");
        append_git_sync_job_lines(&mut output, &self.git_sync_jobs_total);

        output.push_str("# HELP sequence_gap_count Total detected sequence gaps.\n");
        output.push_str("# TYPE sequence_gap_count counter\n");
        output.push_str(&format!(
            "sequence_gap_count {}\n",
            self.sequence_gap_count.load(Ordering::SeqCst)
        ));

        output
    }

    fn set_outbox_depth_by_label(&self, workspace_label: &str, value: i64) {
        let mut guard = self.outbox_depth.lock().expect("metrics map lock poisoned");
        guard.insert(workspace_label.to_string(), value.max(0));
    }
}

fn normalize_endpoint(path: &str) -> String {
    if path.is_empty() {
        return "/".to_string();
    }

    let mut normalized_segments = Vec::new();
    for segment in path.split('/').filter(|segment| !segment.is_empty()) {
        if uuid::Uuid::parse_str(segment).is_ok() {
            normalized_segments.push("{uuid}".to_string());
            continue;
        }

        if segment.chars().all(|character| character.is_ascii_digit()) {
            normalized_segments.push("{number}".to_string());
            continue;
        }

        normalized_segments.push(segment.to_string());
    }

    if normalized_segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", normalized_segments.join("/"))
    }
}

fn normalize_git_sync_state(state: &str) -> String {
    let normalized = state.trim().to_ascii_lowercase();
    if GIT_SYNC_STATES.contains(&normalized.as_str()) {
        normalized
    } else {
        "unknown".to_string()
    }
}

fn normalize_ws_endpoint(endpoint: &str) -> String {
    let normalized = endpoint.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        "unknown".to_string()
    } else {
        normalized
    }
}

fn increment_counter(
    map: &Mutex<HashMap<EndpointMetricKey, u64>>,
    key: &EndpointMetricKey,
    delta: u64,
) {
    let mut guard = map.lock().expect("metrics map lock poisoned");
    let value = guard.entry(key.clone()).or_insert(0);
    *value = value.saturating_add(delta);
}

fn increment_label_counter(map: &Mutex<HashMap<String, u64>>, label: &str, delta: u64) {
    let mut guard = map.lock().expect("metrics map lock poisoned");
    let value = guard.entry(label.to_string()).or_insert(0);
    *value = value.saturating_add(delta);
}

fn append_counter_lines(
    output: &mut String,
    metric_name: &str,
    map: &Mutex<HashMap<EndpointMetricKey, u64>>,
) {
    let guard = map.lock().expect("metrics map lock poisoned");
    let mut entries: Vec<_> = guard.iter().collect();
    entries.sort_by(|(left_key, _), (right_key, _)| {
        left_key
            .method
            .cmp(&right_key.method)
            .then_with(|| left_key.endpoint.cmp(&right_key.endpoint))
    });

    for (key, value) in entries {
        output.push_str(&format!(
            "{metric_name}{{method=\"{}\",endpoint=\"{}\"}} {value}\n",
            escape_label_value(&key.method),
            escape_label_value(&key.endpoint),
        ));
    }
}

fn append_outbox_depth_lines(output: &mut String, map: &Mutex<HashMap<String, i64>>) {
    let guard = map.lock().expect("metrics map lock poisoned");
    if guard.is_empty() {
        output.push_str(&format!("outbox_depth{{workspace_id=\"{UNKNOWN_WORKSPACE_LABEL}\"}} 0\n"));
        return;
    }

    let mut entries: Vec<_> = guard.iter().collect();
    entries
        .sort_by(|(left_workspace, _), (right_workspace, _)| left_workspace.cmp(right_workspace));
    for (workspace_id, value) in entries {
        output.push_str(&format!(
            "outbox_depth{{workspace_id=\"{}\"}} {value}\n",
            escape_label_value(workspace_id),
        ));
    }
}

fn append_label_counter_lines(
    output: &mut String,
    metric_name: &str,
    map: &Mutex<HashMap<String, u64>>,
) {
    let guard = map.lock().expect("metrics map lock poisoned");
    if guard.is_empty() {
        return;
    }

    let mut entries: Vec<_> = guard.iter().collect();
    entries.sort_by(|(left, _), (right, _)| left.cmp(right));

    for (label, value) in entries {
        output.push_str(&format!(
            "{metric_name}{{endpoint=\"{}\"}} {value}\n",
            escape_label_value(label),
        ));
    }
}

fn append_git_sync_job_lines(output: &mut String, map: &Mutex<HashMap<String, u64>>) {
    let guard = map.lock().expect("metrics map lock poisoned");
    let mut entries: Vec<_> = guard.iter().collect();
    entries.sort_by(|(left_state, _), (right_state, _)| left_state.cmp(right_state));

    for (state, value) in entries {
        output.push_str(&format!(
            "git_sync_jobs_total{{state=\"{}\"}} {value}\n",
            escape_label_value(state),
        ));
    }
}

fn escape_label_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\n', "\\n").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::RelayMetrics;

    #[test]
    fn render_prometheus_includes_red_and_custom_metrics() {
        let metrics = RelayMetrics::default();
        metrics.record_http_request("GET", "/v1/workspaces/123", 200, 15);
        metrics.record_http_request("GET", "/v1/workspaces/123", 500, 25);
        metrics.record_ws_request("yjs_update", false, 11);
        metrics.record_ws_request("yjs_update", true, 19);
        metrics.observe_sync_ack_latency_ms(77);
        metrics.set_outbox_depth_for_workspace(
            Uuid::parse_str("00000000-0000-0000-0000-000000000001").expect("uuid is valid"),
            6,
        );
        metrics.set_daemon_recovery_time_ms(1234);
        metrics.increment_git_sync_jobs_total_for_state("queued");
        metrics.increment_git_sync_jobs_total_for_state("failed");
        metrics.increment_git_sync_jobs_total_for_state("not-a-real-state");
        metrics.increment_sequence_gap_count();
        metrics.increment_sequence_gap_count();

        let rendered = metrics.render_prometheus();

        assert!(rendered.contains("relay_request_rate_total"));
        assert!(rendered.contains("relay_request_errors_total"));
        assert!(rendered.contains("relay_request_duration_ms_sum"));
        assert!(rendered.contains("relay_request_duration_ms_count"));
        assert!(rendered.contains("relay_ws_rate_total"));
        assert!(rendered.contains("relay_ws_errors_total"));
        assert!(rendered.contains("relay_ws_duration_ms_sum"));
        assert!(rendered.contains("relay_ws_duration_ms_count"));
        assert!(rendered.contains("relay_ws_rate_total{endpoint=\"yjs_update\"} 2"));
        assert!(rendered.contains("relay_ws_errors_total{endpoint=\"yjs_update\"} 1"));
        assert!(rendered.contains("sync_ack_latency_ms 77"));
        assert!(rendered
            .contains("outbox_depth{workspace_id=\"00000000-0000-0000-0000-000000000001\"} 6"));
        assert!(rendered.contains("daemon_recovery_time_ms 1234"));
        assert!(rendered.contains("git_sync_jobs_total{state=\"queued\"} 1"));
        assert!(rendered.contains("git_sync_jobs_total{state=\"running\"} 0"));
        assert!(rendered.contains("git_sync_jobs_total{state=\"completed\"} 0"));
        assert!(rendered.contains("git_sync_jobs_total{state=\"failed\"} 1"));
        assert!(rendered.contains("git_sync_jobs_total{state=\"unknown\"} 1"));
        assert!(rendered.contains("sequence_gap_count 2"));
        assert!(rendered.contains("endpoint=\"/v1/workspaces/{number}\""));
    }
}
