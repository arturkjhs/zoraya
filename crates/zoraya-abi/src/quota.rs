//! The records of `job_set_limits` and `job_get_usage` (ADR-0261, ADR-0263).
//! Integers are little-endian; a number that does not apply is `NONE`.
//!
//! Limits, 24 bytes: `memory` u64 bytes, `processes` u32, `cpu` u32 (a
//! share of the machine in thousandths, ADR-0264: 1 to 1000), `threads` u32
//! (ADR-0307 item 2: 0 means no limit, so an old caller that leaves the word
//! zero sets none), then a reserved u32 that must be zero.
//!
//! Usage, 88 bytes: for memory and then for processes, `limit`, `used`, `peak`
//! and `refused` (four u64), then memory's `user`, `kernel` and `held` (three
//! u64). Then, from Q4, the CPU (ADR-0264 point 8): `limit` in thousandths,
//! `used_ns` since creation, `period_used` ns in the current period, `parked`
//! times (four u64, 120 bytes in all). Then, from stage 9, the threads
//! (ADR-0307 item 2): `limit`, `used`, `peak` and `refused` (four u64, 152
//! bytes in all). A buffer of 88 bytes is the first version: the kernel
//! writes what fits, and the decoder reads the CPU and thread words as zero
//! and no limit when they are missing. A longer buffer is accepted
//! and the extra bytes ignored, so later fields can be appended.
//!
//! The decoders are total.

use crate::Status;

/// A limit that is not set.
pub const NONE: u64 = u64::MAX;
const NONE32: u32 = u32::MAX;

/// Size of the limits record.
pub const LIMITS_SIZE: usize = 24;
/// Size of the usage record.
pub const USAGE_SIZE: usize = 88;
/// Size of the usage record with the CPU words.
pub const USAGE_SIZE_CPU: usize = 120;
/// Size of the usage record with the thread words (ADR-0307 item 2).
pub const USAGE_SIZE_THREADS: usize = 152;

/// Which limit refused a charge: the value register of an `OverQuota`
/// (ADR-0263 point 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaField {
    Memory = 1,
    Processes = 2,
    /// Live threads (ADR-0307 item 1).
    Threads = 3,
}

impl QuotaField {
    pub const fn from_raw(raw: u64) -> Option<QuotaField> {
        match raw {
            1 => Some(QuotaField::Memory),
            2 => Some(QuotaField::Processes),
            3 => Some(QuotaField::Threads),
            _ => None,
        }
    }
}

/// What a job may use (ADR-0261 point 1). `None` is "no limit of my own".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JobLimits {
    pub memory: Option<u64>,
    pub processes: Option<u32>,
    pub cpu: Option<u32>,
    /// Live threads of the job's subtree; `None` (0 on the wire) is no limit.
    pub threads: Option<u32>,
}

impl JobLimits {
    pub fn encode(&self) -> [u8; LIMITS_SIZE] {
        let mut out = [0; LIMITS_SIZE];
        out[..8].copy_from_slice(&self.memory.unwrap_or(NONE).to_le_bytes());
        out[8..12].copy_from_slice(&self.processes.unwrap_or(NONE32).to_le_bytes());
        out[12..16].copy_from_slice(&self.cpu.unwrap_or(NONE32).to_le_bytes());
        // Zero is "no limit", so `Some(0)` is not expressible: a limit of no
        // threads at all would refuse the first thread of every process.
        out[16..20].copy_from_slice(&self.threads.unwrap_or(0).to_le_bytes());
        out
    }

    /// `InvalidArgument` for a wrong size, a non-zero reserved word, or a
    /// memory limit that is `NONE` in all but name.
    pub fn decode(bytes: &[u8]) -> Result<JobLimits, Status> {
        if bytes.len() != LIMITS_SIZE || bytes[20..] != [0; 4] {
            return Err(Status::InvalidArgument);
        }
        let memory =
            u64::from_le_bytes(bytes[..8].try_into().map_err(|_| Status::InvalidArgument)?);
        let processes = u32::from_le_bytes(
            bytes[8..12]
                .try_into()
                .map_err(|_| Status::InvalidArgument)?,
        );
        let cpu = u32::from_le_bytes(
            bytes[12..16]
                .try_into()
                .map_err(|_| Status::InvalidArgument)?,
        );
        let threads = u32::from_le_bytes(
            bytes[16..20]
                .try_into()
                .map_err(|_| Status::InvalidArgument)?,
        );
        Ok(JobLimits {
            memory: (memory != NONE).then_some(memory),
            processes: (processes != NONE32).then_some(processes),
            cpu: (cpu != NONE32).then_some(cpu),
            threads: (threads != 0).then_some(threads),
        })
    }
}

/// One field of a usage record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FieldUsage {
    pub limit: Option<u64>,
    pub used: u64,
    pub peak: u64,
    pub refused: u64,
}

/// `job_get_usage`'s record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JobUsage {
    pub memory: FieldUsage,
    pub processes: FieldUsage,
    /// Live threads, the first of each process included (ADR-0307 item 1).
    pub threads: FieldUsage,
    /// Memory in user frames.
    pub user: u64,
    /// Memory in kernel objects: `memory.used - user`.
    pub kernel: u64,
    /// Memory still charged to this job by objects kept alive from outside
    /// after its last process ended (ADR-0261 point 9).
    pub held: u64,
    /// The CPU ceiling and use (ADR-0264 point 8).
    pub cpu: CpuUsage,
}

/// The CPU part of a usage record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CpuUsage {
    /// The job's own ceiling in thousandths, if it has one.
    pub limit: Option<u32>,
    /// Nanoseconds run since creation by the job and everything below it.
    pub used_ns: u64,
    /// Nanoseconds run in the current period.
    pub period_used: u64,
    /// Times a thread was parked because this job was over budget.
    pub parked: u64,
}

fn put(out: &mut [u8], at: usize, value: u64) {
    out[at * 8..at * 8 + 8].copy_from_slice(&value.to_le_bytes());
}

fn get(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(at * 8..at * 8 + 8)?.try_into().ok()?,
    ))
}

impl JobUsage {
    pub fn encode(&self) -> [u8; USAGE_SIZE_THREADS] {
        let mut out = [0; USAGE_SIZE_THREADS];
        let words = [
            self.memory.limit.unwrap_or(NONE),
            self.memory.used,
            self.memory.peak,
            self.memory.refused,
            self.processes.limit.unwrap_or(NONE),
            self.processes.used,
            self.processes.peak,
            self.processes.refused,
            self.user,
            self.kernel,
            self.held,
            self.cpu.limit.map_or(NONE, u64::from),
            self.cpu.used_ns,
            self.cpu.period_used,
            self.cpu.parked,
            self.threads.limit.unwrap_or(NONE),
            self.threads.used,
            self.threads.peak,
            self.threads.refused,
        ];
        for (at, word) in words.into_iter().enumerate() {
            put(&mut out, at, word);
        }
        out
    }

    /// Total: any bytes give a record or `None`.
    pub fn decode(bytes: &[u8]) -> Option<JobUsage> {
        let limit = |v: u64| (v != NONE).then_some(v);
        Some(JobUsage {
            memory: FieldUsage {
                limit: limit(get(bytes, 0)?),
                used: get(bytes, 1)?,
                peak: get(bytes, 2)?,
                refused: get(bytes, 3)?,
            },
            processes: FieldUsage {
                limit: limit(get(bytes, 4)?),
                used: get(bytes, 5)?,
                peak: get(bytes, 6)?,
                refused: get(bytes, 7)?,
            },
            user: get(bytes, 8)?,
            kernel: get(bytes, 9)?,
            held: get(bytes, 10)?,
            cpu: CpuUsage {
                limit: get(bytes, 11)
                    .and_then(|v| (v != NONE).then_some(v))
                    .and_then(|v| u32::try_from(v).ok()),
                used_ns: get(bytes, 12).unwrap_or(0),
                period_used: get(bytes, 13).unwrap_or(0),
                parked: get(bytes, 14).unwrap_or(0),
            },
            // Missing words (the 88 and 120 byte versions) read as zero and
            // no limit.
            threads: FieldUsage {
                limit: get(bytes, 15).and_then(limit),
                used: get(bytes, 16).unwrap_or(0),
                peak: get(bytes, 17).unwrap_or(0),
                refused: get(bytes, 18).unwrap_or(0),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_round_trip_and_none_is_none() {
        for l in [
            JobLimits::default(),
            JobLimits {
                memory: Some(512 << 20),
                processes: Some(32),
                cpu: Some(250),
                threads: Some(96),
            },
            JobLimits {
                memory: Some(0),
                processes: Some(0),
                cpu: None,
                threads: None,
            },
        ] {
            assert_eq!(JobLimits::decode(&l.encode()), Ok(l));
        }
    }

    #[test]
    fn zero_threads_in_the_old_reserved_word_is_no_limit() {
        // INV-0307-2: a record from before threads, with zeros in bytes 16 to
        // 24, means no thread limit.
        let mut bytes = JobLimits {
            memory: Some(1),
            processes: Some(2),
            cpu: Some(3),
            threads: None,
        }
        .encode();
        assert_eq!(bytes[16..], [0; 8]);
        assert_eq!(JobLimits::decode(&bytes).unwrap().threads, None);
        bytes[16] = 5;
        assert_eq!(JobLimits::decode(&bytes).unwrap().threads, Some(5));
        bytes[23] = 1;
        assert_eq!(JobLimits::decode(&bytes), Err(Status::InvalidArgument));
    }

    #[test]
    fn limits_decoder_is_total() {
        for n in 0..40 {
            let bytes = [0xA5u8; 40];
            let _ = JobLimits::decode(&bytes[..n]);
        }
        let mut bytes = JobLimits::default().encode();
        bytes[20] = 1;
        assert_eq!(JobLimits::decode(&bytes), Err(Status::InvalidArgument));
        assert_eq!(
            JobLimits::decode(&bytes[..23]),
            Err(Status::InvalidArgument)
        );
    }

    #[test]
    fn usage_round_trips_and_a_short_buffer_is_refused() {
        let u = JobUsage {
            memory: FieldUsage {
                limit: Some(100),
                used: 60,
                peak: 70,
                refused: 2,
            },
            processes: FieldUsage {
                limit: None,
                used: 3,
                peak: 4,
                refused: 0,
            },
            threads: FieldUsage {
                limit: Some(96),
                used: 9,
                peak: 11,
                refused: 1,
            },
            user: 40,
            kernel: 20,
            held: 5,
            cpu: CpuUsage {
                limit: Some(250),
                used_ns: 123_456_789,
                period_used: 4_000_000,
                parked: 7,
            },
        };
        let bytes = u.encode();
        assert_eq!(JobUsage::decode(&bytes), Some(u));
        // INV-0307-2: the 88 and 120 byte records are still accepted, and the
        // thread words read as zero and no limit.
        let cpu = JobUsage::decode(&bytes[..USAGE_SIZE_CPU]).expect("120 bytes");
        assert_eq!(cpu.threads, FieldUsage::default());
        assert_eq!(cpu.cpu, u.cpu);
        assert_eq!(JobUsage::decode(&bytes[..USAGE_SIZE - 1]), None);
        // The first version's record: the CPU words are missing, read as none.
        let first = JobUsage::decode(&bytes[..USAGE_SIZE]).expect("88 bytes");
        assert_eq!(first.cpu, CpuUsage::default());
        assert_eq!(first.held, 5);
        let mut longer = [0u8; USAGE_SIZE_THREADS + 16];
        longer[..USAGE_SIZE_THREADS].copy_from_slice(&bytes);
        assert_eq!(JobUsage::decode(&longer), Some(u));
    }

    #[test]
    fn quota_fields_have_stable_values() {
        assert_eq!(QuotaField::from_raw(1), Some(QuotaField::Memory));
        assert_eq!(QuotaField::from_raw(2), Some(QuotaField::Processes));
        assert_eq!(QuotaField::from_raw(3), Some(QuotaField::Threads));
        assert_eq!(QuotaField::from_raw(0), None);
        assert_eq!(QuotaField::from_raw(4), None);
    }
}
