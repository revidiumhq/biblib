use biblib::dedupe::Deduplicator;
use biblib::{Citation, Date};
use std::hint::black_box;
use std::time::{Duration, Instant};

const DEFAULT_RECORD_COUNT: usize = 750;
const SAMPLE_COUNT: usize = 15;

fn make_metadata_rejected_records(count: usize) -> Vec<Citation> {
    (0..count)
        .map(|idx| Citation {
            title: format!(
                "Effect of oral glutamine supplementation on radiation esophagitis in study cohort {idx:04}"
            ),
            journal: Some(format!("Synthetic Journal {idx:04}")),
            date: Some(Date {
                year: 2024,
                month: None,
                day: None,
            }),
            ..Default::default()
        })
        .collect()
}

fn median_duration(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn benchmark(label: &str, deduplicator: &Deduplicator, citations: &[Citation]) {
    for _ in 0..2 {
        let groups = deduplicator.find_duplicates(black_box(citations));
        assert_eq!(groups.len(), citations.len());
        black_box(groups);
    }

    let samples = (0..SAMPLE_COUNT)
        .map(|_| {
            let started = Instant::now();
            let groups = deduplicator.find_duplicates(black_box(citations));
            let elapsed = started.elapsed();
            assert_eq!(groups.len(), citations.len());
            black_box(groups);
            elapsed
        })
        .collect();

    println!("{label}: {:?}", median_duration(samples));
}

fn main() {
    let record_count = std::env::var("BIBLIB_BENCH_RECORDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_RECORD_COUNT);
    let citations = make_metadata_rejected_records(record_count);
    let candidate_pairs = record_count.saturating_mul(record_count.saturating_sub(1)) / 2;

    println!(
        "metadata-rejected fuzzy workload: {record_count} records, {candidate_pairs} candidate pairs"
    );
    benchmark(
        "sequential",
        &Deduplicator::builder().parallel(false).build(),
        &citations,
    );
    benchmark(
        "parallel",
        &Deduplicator::builder().parallel(true).build(),
        &citations,
    );
}
