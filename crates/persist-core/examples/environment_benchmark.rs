use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;
use std::time::{Duration, Instant};

use persist_core::shell_state::{
    create_identity, write_atomic, EnvironmentPolicy, EnvironmentSnapshot, ShellLaunchEnvironment,
    ShellStateEnvelope,
};

const RUNS: u32 = 1000;

fn main() -> persist_core::Result<()> {
    let root = std::env::temp_dir().join(format!("persistshell-m55-bench-{}", std::process::id()));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .map_err(|source| persist_core::PersistError::Io {
            operation: "create benchmark directory",
            source,
        })?;
    bench_writes(&root, 1, "cwd-only", None)?;
    bench_writes(&root, 2, "typical-16", Some(snapshot(16, 48)?))?;
    bench_writes(&root, 3, "near-64k", Some(snapshot(7, 8192)?))?;
    bench_merge()?;
    fs::remove_dir_all(root).map_err(|source| persist_core::PersistError::Io {
        operation: "remove benchmark directory",
        source,
    })?;
    Ok(())
}

fn snapshot(count: usize, value_bytes: usize) -> persist_core::Result<EnvironmentSnapshot> {
    let names = (0..count)
        .map(|index| format!("M55_{index:03}"))
        .collect::<Vec<_>>();
    let policy = EnvironmentPolicy::new(&names, 128, 64 * 1024)?;
    let current = names
        .into_iter()
        .map(|name| (name, "x".repeat(value_bytes)))
        .collect::<BTreeMap<_, _>>();
    EnvironmentSnapshot::capture(&policy, None, current)
}

fn bench_writes(
    root: &Path,
    session_id: u32,
    label: &str,
    environment: Option<EnvironmentSnapshot>,
) -> persist_core::Result<()> {
    let identity = create_identity(root, session_id)?;
    let mut total = Duration::ZERO;
    let mut max = Duration::ZERO;
    let mut failures = 0_u32;
    for sequence in 1..=RUNS {
        let state = match &environment {
            Some(snapshot) => ShellStateEnvelope::new_v2(
                session_id,
                identity.incarnation(),
                u64::from(sequence),
                "/srv/work".into(),
                snapshot.clone(),
            )?,
            None => ShellStateEnvelope::new(
                session_id,
                identity.incarnation(),
                u64::from(sequence),
                "/srv/work".into(),
            )?,
        };
        let started = Instant::now();
        if write_atomic(&identity, &state).is_err() {
            failures += 1;
        }
        let elapsed = started.elapsed();
        total += elapsed;
        max = max.max(elapsed);
    }
    let size = fs::metadata(identity.path())
        .map_err(|source| persist_core::PersistError::Io {
            operation: "read benchmark state metadata",
            source,
        })?
        .len();
    println!(
        "{label}: runs={RUNS} total_ms={:.3} mean_us={:.3} max_us={:.3} size={} failures={failures}",
        total.as_secs_f64() * 1000.0,
        total.as_secs_f64() * 1_000_000.0 / f64::from(RUNS),
        max.as_secs_f64() * 1_000_000.0,
        size
    );
    Ok(())
}

fn bench_merge() -> persist_core::Result<()> {
    let saved = (0..16)
        .map(|index| (format!("M55_{index:03}"), "value".to_owned()))
        .collect::<Vec<_>>();
    let started = Instant::now();
    for _ in 0..RUNS {
        let _ = ShellLaunchEnvironment::new(
            saved.clone(),
            vec!["M55_999".into()],
            vec![("TERM".into(), "xterm-256color".into())],
            vec![("PERSIST_SESSION_ID".into(), "1".into())],
        )?;
    }
    let total = started.elapsed();
    println!(
        "restore-merge: runs={RUNS} total_ms={:.3} mean_us={:.3}",
        total.as_secs_f64() * 1000.0,
        total.as_secs_f64() * 1_000_000.0 / f64::from(RUNS)
    );
    Ok(())
}
