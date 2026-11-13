//! Batch queue, bounded scheduling, cancellation, scan delivery and output decisions.
//! Single-item workflows and output integrity have independent owners.
use crate::engine::live::{pairing as android, to_android, transfer};
use crate::engine::options::ConversionOptions;
use crate::engine::paths::{self, json as s};
use crate::engine::scanrow::{self, ScanRow, SourceVersion};
use crate::engine::{failure, io_error, uuid, write_json, Native, StopToken, Timings};
use crate::Result;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::time::Instant;

// Compatibility exports: one implementation, existing CLI/host/test paths retained.
pub use crate::engine::live::to_apple::convert;
pub use crate::engine::output::{
    candidate_video_role, expected_names, is_candidate_photo_role, output_names, publish,
    rename_dir, restore_names, verify, Stage, WorkDir, REVISION,
};

/// Worker count: 4 for scanning, 2 for converting, at most half the CPUs.
pub fn worker_limit(scanning: bool) -> usize {
    let cpus = std::thread::available_parallelism().map_or(2, |n| n.get());
    (if scanning { 4 } else { 2 }).min(cpus / 2).max(1)
}

/// Memory admitted at once across workers (Swift: physical memory / 8,
/// at least 256 MiB). Hosts without that figure use 1 GiB.
///
/// Fixed workers with weighted admission: a large input waits until enough
/// budget is free (and runs alone if needed). `receive` is serialized.
/// Returns (peak workers in flight, peak reserved bytes).
pub fn perform<T: Send>(
    files: &[PathBuf],
    limit: usize,
    heavy: bool,
    token: &StopToken,
    work: impl Fn(usize) -> T + Sync,
    receive: impl FnMut(usize, T) + Send,
) -> (usize, u64) {
    if files.is_empty() {
        return (0, 0);
    }
    let budget = crate::platform::memory_budget();
    let factor = if heavy { 24 } else { 12 };
    let costs: Vec<u64> = files
        .iter()
        .map(|f| {
            let size = std::fs::metadata(f).map_or(budget, |m| m.len());
            (size.min(budget) * factor).clamp(64 << 20, budget)
        })
        .collect();
    struct State {
        next: usize,
        used: u64,
        running: usize,
        peak: usize,
        peak_bytes: u64,
    }
    let state = Mutex::new(State {
        next: 0,
        used: 0,
        running: 0,
        peak: 0,
        peak_bytes: 0,
    });
    let changed = Condvar::new();
    let receive = Mutex::new(receive);
    let workers = limit.min(files.len()).max(1);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let (index, cost) = {
                    let mut st = state.lock().unwrap_or_else(|p| p.into_inner());
                    while st.next < files.len()
                        && !token.is_cancelled()
                        && st.used > 0
                        && st.used + costs[st.next] > budget
                    {
                        st = changed
                            .wait_timeout(st, std::time::Duration::from_millis(100))
                            .unwrap_or_else(|p| p.into_inner())
                            .0;
                    }
                    if st.next >= files.len() || token.is_cancelled() {
                        return;
                    }
                    let index = st.next;
                    st.next += 1;
                    st.used += costs[index];
                    st.running += 1;
                    st.peak = st.peak.max(st.running);
                    st.peak_bytes = st.peak_bytes.max(st.used);
                    (index, costs[index])
                };
                let value = work(index);
                (receive.lock().unwrap_or_else(|p| p.into_inner()))(index, value);
                let mut st = state.lock().unwrap_or_else(|p| p.into_inner());
                st.used -= cost;
                st.running -= 1;
                changed.notify_all();
            });
        }
    });
    let st = state.into_inner().unwrap_or_else(|p| p.into_inner());
    (st.peak, st.peak_bytes)
}

// ---------------------------------------------------------------------------
// Scan and run
// ---------------------------------------------------------------------------

/// Swift `NativeBatch.scan(sources:direction:…)`.
pub fn scan(
    native: &dyn Native,
    sources: &[PathBuf],
    direction: &str,
    token: &StopToken,
    concurrency: Option<usize>,
    mut rows_ready: impl FnMut(&[ScanRow]) + Send,
    mut progress: impl FnMut(usize, usize) + Send,
) -> Result<Vec<ScanRow>> {
    let (base, entries) = if direction == "to-android" {
        android::selection(sources)?
    } else {
        paths::selection(sources)?
    };
    let relatives: Vec<String> = entries
        .iter()
        .map(|e| paths::relative(e, &base))
        .collect::<Result<_>>()?;
    let mut rows: BTreeMap<usize, ScanRow> = BTreeMap::new();
    let mut pending: Vec<ScanRow> = Vec::new();
    let mut last = Instant::now() - std::time::Duration::from_secs(1);
    let total = entries.len();
    let mut completed = 0;
    perform(
        &entries,
        concurrency.unwrap_or_else(|| worker_limit(true)),
        false,
        token,
        |i| match direction {
            "to-android" => android::scan_file_with_native(native, &entries[i], &relatives[i]),
            "honor-to-google" => {
                super::live::honor_to_google::scan_file(native, &entries[i], &relatives[i])
            }
            "android-to-android" => transfer::scan_file(native, &entries[i], &relatives[i], None),
            _ => scanrow::scan_file(native, &entries[i], &relatives[i], None),
        },
        |i, row| {
            completed += 1;
            pending.push(row.clone());
            rows.insert(i, row);
            if completed == total || last.elapsed().as_secs_f64() >= 0.1 {
                rows_ready(&pending);
                pending.clear();
                last = Instant::now();
            }
            progress(completed, total);
        },
    );
    if !pending.is_empty() {
        rows_ready(&pending);
    }
    Ok(rows.into_values().collect())
}

/// What to do with an output folder that already exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExistingOutput {
    Resume,
    Skip,
    Overwrite,
    Cancel,
}

/// Swift `BatchProgress`.
#[derive(Clone, Debug)]
pub struct Progress {
    pub index: usize,
    pub total: usize,
    pub relative: String,
    pub status: String,
    pub detail: String,
}

/// Swift `BatchOutcome`.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub counts: BTreeMap<String, usize>,
    pub report: Option<PathBuf>,
    pub report_data: Value,
    pub cancelled: bool,
}

pub struct RunRequest<'a> {
    pub sources: &'a [PathBuf],
    pub output: &'a Path,
    pub options: &'a ConversionOptions,
    /// Rows from an earlier scan; reused while a file's version is unchanged.
    pub scan_rows: Option<&'a [ScanRow]>,
    pub concurrency: Option<usize>,
}

/// Swift `NativeBatch.run(sources:…)`.
pub fn run(
    native: &dyn Native,
    req: &RunRequest,
    token: &StopToken,
    existing_output: Option<&(dyn Fn(&Path, &Path) -> ExistingOutput + Sync)>,
    mut progress: impl FnMut(&Progress) + Send,
) -> Result<Outcome> {
    let started = Instant::now();
    let options = req.options;
    let destination = paths::normalized(req.output);
    paths::validate_output(req.sources, &destination)?;
    let (base, entries) = if options.direction == "to-android" {
        android::selection(req.sources)?
    } else {
        paths::selection(req.sources)?
    };
    let enumerated = crate::engine::uptime(started);
    if let Some(rows) = req.scan_rows {
        let a: HashSet<PathBuf> = rows.iter().map(|r| paths::normalized(&r.source)).collect();
        let b: HashSet<PathBuf> = entries.iter().cloned().collect();
        if a != b {
            return failure("来源清单已变化，请重新扫描后转换");
        }
    }
    let relatives: Vec<String> = entries
        .iter()
        .map(|e| paths::relative(e, &base))
        .collect::<Result<_>>()?;
    let snapshot: HashMap<PathBuf, &ScanRow> = req
        .scan_rows
        .unwrap_or(&[])
        .iter()
        .map(|r| (paths::normalized(&r.source), r))
        .collect();
    let mut static_indices: HashSet<usize> = HashSet::new();
    let mut hdr_indices: HashSet<usize> = HashSet::new();
    let mut inspected: BTreeMap<usize, ScanRow> = BTreeMap::new();
    {
        perform(
            &entries,
            worker_limit(true),
            false,
            token,
            |i| {
                if let Some(cached) = snapshot.get(&entries[i]) {
                    if cached.version.is_some()
                        && cached.version == SourceVersion::read(&entries[i])
                    {
                        return (*cached).clone();
                    }
                }
                if options.direction == "to-android" {
                    android::scan_file_with_native(native, &entries[i], &relatives[i])
                } else if options.direction == "honor-to-google" {
                    super::live::honor_to_google::scan_file(native, &entries[i], &relatives[i])
                } else if options.direction == "android-to-android" {
                    transfer::scan_file(native, &entries[i], &relatives[i], None)
                } else {
                    scanrow::scan_file(native, &entries[i], &relatives[i], None)
                }
            },
            |i, row| {
                if row.is_static_hdr() && row.burst.is_none() {
                    hdr_indices.insert(i);
                }
                if row.confirmed_static && !row.is_static_hdr() && row.burst.is_none() {
                    static_indices.insert(i);
                }
                inspected.insert(i, row);
            },
        );
    }
    // Group work is admitted to the same queue as live/HDR items. A group leader
    // owns the transaction; per-member results are emitted only after publication.
    let plans = if options.burst_plans.is_empty() {
        crate::engine::burst::plans(&inspected.values().cloned().collect::<Vec<_>>())
    } else {
        options.burst_plans.clone()
    };
    let by_group: HashMap<String, Value> = plans
        .into_iter()
        .filter_map(|p| p["id"].as_str().map(|id| (id.to_owned(), p.clone())))
        .collect();
    let mut grouped: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, row) in &inspected {
        if let Some(id) = row.burst.as_ref().and_then(|v| v["group_id"].as_str()) {
            grouped.entry(id.into()).or_default().push(*i);
        }
    }
    let mut burst_jobs: HashMap<usize, (Value, Vec<usize>)> = HashMap::new();
    let mut followers = HashSet::new();
    for (id, indices) in grouped {
        if let Some(plan) = by_group.get(&id) {
            followers.extend(indices.iter().skip(1).copied());
            burst_jobs.insert(indices[0], (plan.clone(), indices));
        }
    }
    let planned = crate::engine::uptime(started);
    let total = entries.len() - static_indices.len();
    let mut results: BTreeMap<usize, Value> = BTreeMap::new();
    let mut counts: BTreeMap<String, usize> =
        ["converted", "resumed", "copied", "skipped", "failed"]
            .iter()
            .map(|k| (k.to_string(), 0))
            .collect();
    let mut sorted_static: Vec<usize> = static_indices.iter().copied().collect();
    sorted_static.sort_unstable();
    for i in &sorted_static {
        results.insert(*i, json!({"status": "skipped", "classification": "static", "source": s(&entries[*i]), "detail": "静态照片，已跳过"}));
        *counts.entry("skipped".into()).or_default() += 1;
        progress(&Progress {
            index: 0,
            total,
            relative: relatives[*i].clone(),
            status: "static".into(),
            detail: "静态照片，已跳过".into(),
        });
    }
    let active: Vec<usize> = (0..entries.len())
        .filter(|i| !static_indices.contains(i) && !followers.contains(i))
        .collect();
    let suffix = if options.direction != "to-apple" {
        ".motionphoto"
    } else if options.output_mode == "auxiliary-only" {
        ".auxiliary-export"
    } else {
        ".livephoto"
    };
    let targets: Vec<PathBuf> = active
        .iter()
        .map(|i| {
            if let Some((plan, _)) = burst_jobs.get(i) {
                let parent = Path::new(&relatives[*i]).parent().unwrap_or(Path::new(""));
                let make = crate::engine::burst::source_make(plan);
                let evidence =
                    crate::engine::burst::filename(&paths::file_name(&entries[*i]), make);
                let label = evidence
                    .as_ref()
                    .and_then(|v| v["source_group"].as_str())
                    .unwrap_or("burst");
                let namespace = crate::sha256_hex(plan["id"].to_string().as_bytes());
                return destination.join(parent).join(format!(
                    "{}-{}.burst",
                    label,
                    &namespace[..8]
                ));
            }
            destination.join(format!(
                "{}{}",
                relatives[*i],
                if hdr_indices.contains(i) {
                    ".static-hdr"
                } else {
                    suffix
                }
            ))
        })
        .collect();
    let folded: HashSet<String> = targets
        .iter()
        .map(|t| t.to_string_lossy().to_lowercase())
        .collect();
    if folded.len() != targets.len() {
        return failure("多个来源映射到同一输出路径，请分批转换");
    }
    if !active.is_empty() {
        std::fs::create_dir_all(&destination).map_err(|e| io_error(&destination, e))?;
    }
    let decision_lock = Mutex::new(());
    let workers = req.concurrency.unwrap_or_else(|| worker_limit(false));
    let heavy = options.cover_index.is_some()
        || !options.cover_selections.is_empty()
        || options.output_mode == "livephoto+candidates";
    let active_files: Vec<PathBuf> = active.iter().map(|i| entries[*i].clone()).collect();
    // Frames are processed sequentially; reserve for the largest member, not
    // merely the leader's size, while retaining the shared bounded scheduler.
    let admission_files: Vec<PathBuf> = active
        .iter()
        .map(|i| {
            burst_jobs
                .get(i)
                .and_then(|(_, indices)| {
                    indices.iter().max_by_key(|j| {
                        std::fs::metadata(&entries[**j]).map_or(u64::MAX, |m| m.len())
                    })
                })
                .map(|j| entries[*j].clone())
                .unwrap_or_else(|| entries[*i].clone())
        })
        .collect();
    let mut completed = 0usize;
    let (peak, reserved) = perform(
        &admission_files,
        workers,
        heavy,
        token,
        |offset| {
            let file = &active_files[offset];
            let target = &targets[offset];
            let mut overwrite = false;
            let mut wait = 0.0;
            if target.exists() {
                if let Some(decide) = existing_output {
                    let waiting = Instant::now();
                    let decision = {
                        let _g = decision_lock.lock().unwrap_or_else(|p| p.into_inner());
                        if token.is_cancelled() {
                            ExistingOutput::Cancel
                        } else {
                            decide(file, target)
                        }
                    };
                    wait = waiting.elapsed().as_secs_f64();
                    match decision {
                        ExistingOutput::Skip => {
                            return json!({"status": "skipped", "source": s(file), "output": s(target), "detail": "输出已存在，已按选择跳过；已有文件未改动", "decision_wait_seconds": wait})
                        }
                        ExistingOutput::Overwrite => overwrite = true,
                        ExistingOutput::Resume => {}
                        ExistingOutput::Cancel => token.cancel(),
                    }
                }
            }
            if token.is_cancelled() {
                return json!({"status": "cancelled"});
            }
            let began = Instant::now();
            let timings = Timings::default();
            let outcome = if let Some((plan, indices)) = burst_jobs.get(&active[offset]) {
                let id = plan["id"].as_str().unwrap_or("");
                let choice = options.burst_choices.get(id);
                let selected: Vec<_> = indices.iter().map(|i| entries[*i].clone()).collect();
                let invalid_choice = choice.is_some_and(|c| c["signature"] != plan["signature"]);
                if options.direction != "to-apple" {
                    failure("连拍目前仅支持 Apple 目标")
                } else if options.output_mode != "livephoto"
                    || options.video_track != "primary"
                    || options.cover_index.is_some()
                {
                    failure("连拍不支持实况封面或视频轨选项")
                } else if invalid_choice {
                    failure("连拍成员版本变化，代表帧或独立导出确认已失效")
                } else {
                    crate::engine::burst::convert_with_policy(
                        native,
                        plan,
                        &selected,
                        crate::engine::burst::Choice {
                            representative: choice.and_then(|c| c["representative"].as_str()),
                            independent: choice.is_some_and(|c| c["independent"] == true),
                        },
                        target,
                        token,
                        overwrite,
                        options.legacy_hdr,
                    )
                }
            } else if hdr_indices.contains(&active[offset]) {
                crate::engine::static_hdr::convert(
                    native, file, target, options, token, overwrite, &timings,
                )
            } else {
                match options.direction.as_str() {
                    "honor-to-google" => super::live::honor_to_google::convert(
                        native, file, target, options, token, overwrite, &timings,
                    ),
                    "android-to-android" => {
                        transfer::convert(native, file, target, options, token, overwrite, &timings)
                    }
                    "to-android" => to_android::convert(
                        native, file, target, options, token, overwrite, &timings,
                    ),
                    _ => convert(native, file, target, options, token, overwrite, &timings),
                }
            };
            let mut result = outcome.unwrap_or_else(
                |e| json!({"status": "failed", "source": s(file), "error": e.0, "detail": e.0}),
            );
            if [Some("converted"), Some("resumed")].contains(&result["status"].as_str())
                && !result["verification"].is_object()
            {
                if let Ok(manifest) = verify(target) {
                    result["verification"] = manifest;
                }
            }
            result["stage_seconds"] = timings.seconds();
            result["conversion_seconds"] = json!(began.elapsed().as_secs_f64());
            result["decision_wait_seconds"] = json!(wait);
            result
        },
        |offset, result| {
            let status = result["status"].as_str().unwrap_or("failed").to_string();
            if status == "cancelled"
                || (token.is_cancelled()
                    && !["converted", "resumed", "skipped"].contains(&status.as_str()))
            {
                return;
            }
            let index = active[offset];
            let indices = burst_jobs
                .get(&index)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| vec![index]);
            for index in indices {
                let mut detail = result["detail"].as_str().unwrap_or("").to_string();
                let mut item = result.clone();
                item["source"] = s(&entries[index]);
                if let Some(member) = result["burst"]["members"]
                    .as_array()
                    .and_then(|v| v.iter().find(|m| m["source"] == s(&entries[index])))
                {
                    if member["hdr"].is_object() {
                        item["hdr_summary"] = result["hdr"].clone();
                        item["hdr"] = member["hdr"].clone();
                        detail =
                            format!("{}；本帧 {}", detail, super::hdr::summary(&member["hdr"]));
                        item["detail"] = json!(detail);
                    }
                }
                results.insert(index, item);
                let count_kind = if result["processing_kind"] == "original-copy"
                    && ["converted", "resumed"].contains(&status.as_str())
                {
                    "copied".to_string()
                } else {
                    status.clone()
                };
                *counts.entry(count_kind).or_default() += 1;
                completed += 1;
                progress(&Progress {
                    index: completed,
                    total,
                    relative: relatives[index].clone(),
                    status: status.clone(),
                    detail,
                });
            }
        },
    );
    let stamp = iso8601_now().replace(':', "-");
    let report = options
        .export_data_files
        .then(|| destination.join(format!("report-{stamp}-{}.json", &uuid()[..6])));
    let metrics = json!({
        "enumeration_seconds": enumerated,
        "planning_seconds": planned - enumerated,
        "total_seconds": crate::engine::uptime(started),
        "worker_limit": workers,
        "peak_in_flight": peak,
        "peak_reserved_bytes": reserved,
        "static_skipped": static_indices.len(),
        "conversion_entries": total,
        "completed_entries": completed,
    });
    let data = json!({
        "counts": counts,
        "cancelled": token.is_cancelled(),
        "files": results.into_values().collect::<Vec<_>>(),
        "performance": metrics,
    });
    if let Some(r) = &report {
        std::fs::create_dir_all(&destination).map_err(|e| io_error(&destination, e))?;
        write_json(&data, r)?;
    }
    Ok(Outcome {
        counts,
        report,
        report_data: data,
        cancelled: token.is_cancelled(),
    })
}

/// UTC time as `YYYY-MM-DDTHH:MM:SSZ` (Swift `ISO8601DateFormatter`).
pub fn iso8601_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}
