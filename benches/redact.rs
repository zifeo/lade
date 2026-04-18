#[allow(dead_code)]
#[path = "../src/redact.rs"]
mod redact;

const DEFAULT_FMT: &str = "${{}:-REDACTED}";

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::collections::HashMap;
use std::io::{self, Read, Write};

struct NullWriter(usize);
impl Write for NullWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct FixedReader {
    data: Vec<u8>,
    pos: usize,
}
impl FixedReader {
    fn new(data: Vec<u8>) -> Self {
        Self { data, pos: 0 }
    }
}
impl Read for FixedReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.data.len() {
            return Ok(0);
        }
        let n = buf.len().min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// Build a secrets map with `n` entries, each with a 32-byte value that does
/// NOT appear in the payload (no-match / hot-DFA case).
fn no_match_secrets(n: usize) -> HashMap<String, String> {
    (0..n)
        .map(|i| {
            let name = format!("SECRET_{i:04}");
            // Values are hex strings that will never appear in the ASCII payload.
            let value = format!("deadbeef{i:024x}");
            (name, value)
        })
        .collect()
}

/// Build secrets so that exactly ~1 in 100 bytes of the payload is a match.
fn match_secrets(density_marker: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("HOT_SECRET".to_string(), density_marker.to_string());
    m
}

/// Payload: repeating ASCII printable bytes (no embedded secrets for no-match,
/// or a marker injected every 100 bytes for the match-density case).
fn payload_no_match(size: usize) -> Vec<u8> {
    (0..size).map(|i| (b'a' + (i % 26) as u8)).collect()
}

fn payload_with_matches(size: usize, marker: &str) -> Vec<u8> {
    let period = 100usize;
    let mut out = Vec::with_capacity(size);
    let padding: Vec<u8> = (0..period).map(|i| (b'a' + (i % 26) as u8)).collect();
    let marker_bytes = marker.as_bytes();
    while out.len() < size {
        let remaining = size - out.len();
        let pad_len = period.min(remaining);
        out.extend_from_slice(&padding[..pad_len]);
        if out.len() < size {
            let m_len = marker_bytes.len().min(size - out.len());
            out.extend_from_slice(&marker_bytes[..m_len]);
        }
    }
    out.truncate(size);
    out
}

fn bench_no_match(c: &mut Criterion) {
    let sizes = [1024usize, 1024 * 1024, 64 * 1024 * 1024];
    let secret_counts = [1usize, 10, 100];

    let mut group = c.benchmark_group("no_match");
    for &size in &sizes {
        for &n in &secret_counts {
            let secrets = no_match_secrets(n);
            let redactor = redact::Redactor::new(&secrets, DEFAULT_FMT).unwrap();
            let data = payload_no_match(size);
            group.throughput(Throughput::Bytes(size as u64));
            group.bench_with_input(
                BenchmarkId::new(format!("{n}_secrets"), size),
                &size,
                |b, _| {
                    b.iter(|| {
                        let reader = FixedReader::new(data.clone());
                        let mut sink = NullWriter(0);
                        redactor.stream(reader, &mut sink).unwrap();
                    });
                },
            );
        }

        // Baseline: raw io::copy with no redaction.
        let data = payload_no_match(size);
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::new("baseline_io_copy", size), &size, |b, _| {
            b.iter(|| {
                let mut reader = FixedReader::new(data.clone());
                let mut sink = NullWriter(0);
                io::copy(&mut reader, &mut sink).unwrap();
            });
        });
    }
    group.finish();
}

fn bench_match_density(c: &mut Criterion) {
    // 1% match density: marker injected every ~100 bytes.
    let marker = "xSECRETVALUE32BYTESxxxxxxxxxxxxxxx"; // 34 bytes
    let sizes = [1024usize, 1024 * 1024, 64 * 1024 * 1024];

    let mut group = c.benchmark_group("match_density_1pct");
    for &size in &sizes {
        let secrets = match_secrets(marker);
        let redactor = redact::Redactor::new(&secrets, DEFAULT_FMT).unwrap();
        let data = payload_with_matches(size, marker);
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                let reader = FixedReader::new(data.clone());
                let mut sink = NullWriter(0);
                redactor.stream(reader, &mut sink).unwrap();
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_no_match, bench_match_density);
criterion_main!(benches);
