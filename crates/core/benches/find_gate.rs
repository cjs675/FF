//! Performance gate benchmark: fsearch vs portable filter (PLAN §7).
//!
//! Measures:
//! 1. Directory entry filtering: PortableFilter (substring) vs fsearch fuzzy scoring.
//! 2. Tree / codebase search: unindexed portable traversal vs indexed fsearch engine.
//! 3. Latency percentiles (p50, p90, max) and memory footprint (RSS).

use std::path::{Path, PathBuf};
use std::time::Instant;

use ff_core::find::{FileFinder, PortableFilter};
use ff_core::model::{Entry, EntryKind};

#[cfg(all(target_os = "macos", feature = "fsearch-macos"))]
use fsearch::query::fuzzy_score;

#[cfg(target_os = "macos")]
fn resident_memory_mb() -> f64 {
    use std::mem;
    let mut info: libc::mach_task_basic_info = unsafe { mem::zeroed() };
    let mut count = (mem::size_of::<libc::mach_task_basic_info>()
        / mem::size_of::<libc::natural_t>()) as libc::mach_msg_type_number_t;
    #[allow(deprecated)]
    let self_port = unsafe { libc::mach_task_self() };
    let kerr = unsafe {
        libc::task_info(
            self_port,
            libc::MACH_TASK_BASIC_INFO,
            &mut info as *mut _ as *mut libc::integer_t,
            &mut count,
        )
    };
    if kerr == libc::KERN_SUCCESS {
        info.resident_size as f64 / (1024.0 * 1024.0)
    } else {
        0.0
    }
}

#[cfg(not(target_os = "macos"))]
fn resident_memory_mb() -> f64 {
    0.0
}

struct Stats {
    p50_us: f64,
    p90_us: f64,
    mean_us: f64,
    max_us: f64,
}


fn compute_stats(mut times: Vec<f64>) -> Stats {
    if times.is_empty() {
        return Stats {
            p50_us: 0.0,
            p90_us: 0.0,
            mean_us: 0.0,
            max_us: 0.0,
        };
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let len = times.len();
    let p50 = times[len / 2];
    let p90 = times[(len * 9) / 10];
    let max = times[len - 1];
    let sum: f64 = times.iter().sum();
    Stats {
        p50_us: p50,
        p90_us: p90,
        mean_us: sum / (len as f64),
        max_us: max,
    }
}


fn generate_synthetic_entries(count: usize) -> Vec<Entry> {
    let mut out = Vec::with_capacity(count);
    let sample_names = [
        "Cargo.toml",
        "Cargo.lock",
        "PLAN.md",
        "README.md",
        "lib.rs",
        "main.rs",
        "shell.rs",
        "find.rs",
        "session.rs",
        "theme.rs",
        "watcher.rs",
        "engine.rs",
        "config.rs",
        "package-dmg.sh",
        "AppIcon.icns",
    ];
    for i in 0..count {
        let base = sample_names[i % sample_names.len()];
        let name = format!("{i}_{base}");
        out.push(Entry {
            path: PathBuf::from(&name),
            name,
            kind: if i % 4 == 0 {
                EntryKind::Dir
            } else {
                EntryKind::File
            },
        });
    }
    out
}

fn bench_directory_filtering() {
    println!("\n=== Benchmark 1: Directory In-Memory Filtering (10,000 entries) ===");
    let entries = generate_synthetic_entries(10_000);
    let queries = ["plan", "shell", "cargo", "appicon", "main", "nonexistent"];

    // 1. PortableFilter (Substring)
    let portable = PortableFilter;
    let mut portable_times = Vec::new();
    for _ in 0..200 {
        for q in &queries {
            let t0 = Instant::now();
            let _hits = portable.filter(&entries, q);
            portable_times.push(t0.elapsed().as_secs_f64() * 1_000_000.0);
        }
    }
    let p_stats = compute_stats(portable_times);
    println!(
        "PortableFilter (substring):  p50: {:6.2} µs | p90: {:6.2} µs | mean: {:6.2} µs | max: {:6.2} µs",
        p_stats.p50_us, p_stats.p90_us, p_stats.mean_us, p_stats.max_us
    );

    // 2. fsearch fuzzy scoring
    #[cfg(all(target_os = "macos", feature = "fsearch-macos"))]
    {
        let mut fsearch_times = Vec::new();
        for _ in 0..200 {
            for q in &queries {
                let q_bytes = q.as_bytes();
                let t0 = Instant::now();
                let _hits: Vec<&Entry> = entries
                    .iter()
                    .filter(|e| fuzzy_score(e.name.as_bytes(), q_bytes).is_some())
                    .collect();
                fsearch_times.push(t0.elapsed().as_secs_f64() * 1_000_000.0);
            }
        }
        let f_stats = compute_stats(fsearch_times);
        println!(
            "fsearch (SIMD fuzzy):       p50: {:6.2} µs | p90: {:6.2} µs | mean: {:6.2} µs | max: {:6.2} µs",
            f_stats.p50_us, f_stats.p90_us, f_stats.mean_us, f_stats.max_us
        );
    }
}

fn walk_portable(dir: &Path, query: &str, hits: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.to_lowercase().contains(query) {
            hits.push(path.clone());
        }
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            walk_portable(&path, query, hits);
        }
    }
}

fn bench_tree_search() {
    println!("\n=== Benchmark 2: Deep Tree Codebase Search ===");
    let root = Path::new("/Users/chrissilva/environment/repos");
    if !root.exists() {
        println!("Skipping: repos path does not exist");
        return;
    }

    let initial_rss = resident_memory_mb();
    println!("Base process RSS: {:.2} MB", initial_rss);

    let test_queries = ["cargo", "engine", "session", "watcher", "readme"];

    // 1. Portable unindexed walk
    println!("\nRunning unindexed portable filesystem walk (baseline)...");
    let mut portable_walk_times = Vec::new();
    for q in &test_queries {
        let t0 = Instant::now();
        let mut hits = Vec::new();
        walk_portable(root, q, &mut hits);
        let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
        portable_walk_times.push(elapsed_ms);
        println!("  Portable walk '{q}': {:6.2} ms ({} matches)", elapsed_ms, hits.len());
    }

    // 2. fsearch indexed engine
    #[cfg(all(target_os = "macos", feature = "fsearch-macos"))]
    {
        println!("\nStarting fsearch engine on macOS...");
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/chrissilva".into());
        let engine_dir = fsearch::default_dir(&home);
        let t_start = Instant::now();
        let engine = fsearch::Engine::start(fsearch::Options {
            dir: engine_dir,
            home: home.clone(),
            skip: None,
        });

        match engine {
            Ok(engine) => {
                println!("  Engine initialized in {:.2} ms", t_start.elapsed().as_secs_f64() * 1000.0);
                let post_init_rss = resident_memory_mb();
                println!("  Memory RSS with engine: {:.2} MB (delta: +{:.2} MB)", post_init_rss, post_init_rss - initial_rss);

                // Wait briefly for live stream replay if loading
                for _ in 0..50 {
                    if engine.status().ready {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }

                let status = engine.status();
                println!("  Engine status: ready={}, entries={}, dirs={}", status.ready, status.entries, status.dirs);

                // Warm up query
                if let Ok(warm) = fsearch::query::Query::parse("test", &home) {
                    let _ = engine.search(&warm);
                }

                let mut fsearch_times = Vec::new();
                for q in &test_queries {
                    let parsed = fsearch::query::Query::parse(q, &home);
                    match parsed {
                        Ok(query) => {
                            // Run multiple iterations for steady state
                            let mut it_times = Vec::new();
                            let mut matches = 0;
                            for _ in 0..5 {
                                let t0 = Instant::now();
                                if let Ok(res) = engine.search(&query) {
                                    it_times.push(t0.elapsed().as_secs_f64() * 1000.0);
                                    matches = res.len();
                                }
                            }
                            if !it_times.is_empty() {
                                it_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
                                let median_ms = it_times[it_times.len() / 2];
                                fsearch_times.push(median_ms);
                                println!("  fsearch '{q}' (median of 5): {:6.3} ms ({} matches across {} indexed entries)", median_ms, matches, status.entries);
                            }
                        }
                        Err(e) => println!("  Failed to parse query '{q}': {e}"),
                    }
                }


                if !fsearch_times.is_empty() && !portable_walk_times.is_empty() {
                    let avg_portable: f64 = portable_walk_times.iter().sum::<f64>() / (portable_walk_times.len() as f64);
                    let avg_fsearch: f64 = fsearch_times.iter().sum::<f64>() / (fsearch_times.len() as f64);
                    let speedup = avg_portable / avg_fsearch;
                    println!("\n=== Summary Performance Gate Results ===");
                    println!("Average portable unindexed walk: {:6.2} ms", avg_portable);
                    println!("Average fsearch indexed lookup:   {:6.3} ms", avg_fsearch);
                    println!("Speedup margin:                   {:6.1}x", speedup);
                    println!("Memory footprint delta:           +{:.2} MB", post_init_rss - initial_rss);
                    if speedup >= 5.0 {
                        println!("GATE RESULT: PASS (speedup {:.1}x exceeds README target margin)", speedup);
                    } else {
                        println!("GATE RESULT: NOTICE (speedup {:.1}x)", speedup);
                    }
                }
            }
            Err(e) => {
                println!("  Could not start fsearch engine: {e}");
            }
        }
    }
}

fn main() {
    println!("============================================================");
    println!("  FF — Search Benchmark Gate (PLAN §7 Verification)");
    println!("============================================================");

    bench_directory_filtering();
    bench_tree_search();

    println!("\nBenchmark gate complete.\n");
}
