//! The records of `job_set_limits` and `job_get_usage` (ADR-0261, ADR-0263).
//! Integers are little-endian; a number that does not apply is `NONE`.
//!
//! Limits, 24 bytes: `memory` u64 bytes, `processes` u32, `cpu` u32 (a
//! share in thousandths of a period, ADR-0264; the kernel refuses one until
//! the CPU ceiling exists), then a reserved u64 that must be zero.
//!
//! Usage, 88 bytes: for memory and then for processes, `limit`, `used`, `peak`
//! and `refused` (four u64), then memory's `user`, `kernel` and `held` (three
//! u64). A longer buffer is accepted by the decoder and the extra bytes are
//! ignored, so later fields can be appended.
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

/// Which limit refused a charge: the value register of an `OverQuota`
/// (ADR-0263 point 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaField {
    Memory = 1,
    Processes = 2,
}

impl QuotaField {
    pub const fn from_raw(raw: u64) -> Option<QuotaField> {
        match raw {
            1 => Some(QuotaField::Memory),
            2 => Some(QuotaField::Processes),
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
}

impl JobLimits {
    pub fn encode(&self) -> [u8; LIMITS_SIZE] {
        let mut out = [0; LIMITS_SIZE];
        out[..8].copy_from_slice(&self.memory.unwrap_or(NONE).to_le_bytes());
        out[8..12].copy_from_slice(&self.processes.unwrap_or(NONE32).to_le_bytes());
        out[12..16].copy_from_slice(&self.cpu.unwrap_or(NONE32).to_le_bytes());
        out
    }

    /// `InvalidArgument` for a wrong size, a non-zero reserved word, or a
    /// memory limit that is `NONE` in all but name.
    pub fn decode(bytes: &[u8]) -> Result<JobLimits, Status> {
        if bytes.len() != LIMITS_SIZE || bytes[16..] != [0; 8] {
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
        Ok(JobLimits {
            memory: (memory != NONE).then_some(memory),
            processes: (processes != NONE32).then_some(processes),
            cpu: (cpu != NONE32).then_some(cpu),
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
    /// Memory in user frames.
    pub user: u64,
    /// Memory in kernel objects: `memory.used - user`.
    pub kernel: u64,
    /// Memory still charged to this job by objects kept alive from outside
    /// after its last process ended (ADR-0261 point 9).
    pub held: u64,
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
    pub fn encode(&self) -> [u8; USAGE_SIZE] {
        let mut out = [0; USAGE_SIZE];
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
            },
            JobLimits {
                memory: Some(0),
                processes: Some(0),
                cpu: None,
            },
        ] {
            assert_eq!(JobLimits::decode(&l.encode()), Ok(l));
        }
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
            user: 40,
            kernel: 20,
            held: 5,
        };
        let bytes = u.encode();
        assert_eq!(JobUsage::decode(&bytes), Some(u));
        assert_eq!(JobUsage::decode(&bytes[..USAGE_SIZE - 1]), None);
        let mut longer = [0u8; USAGE_SIZE + 16];
        longer[..USAGE_SIZE].copy_from_slice(&bytes);
        assert_eq!(JobUsage::decode(&longer), Some(u));
    }

    #[test]
    fn quota_fields_have_stable_values() {
        assert_eq!(QuotaField::from_raw(1), Some(QuotaField::Memory));
        assert_eq!(QuotaField::from_raw(2), Some(QuotaField::Processes));
        assert_eq!(QuotaField::from_raw(0), None);
        assert_eq!(QuotaField::from_raw(3), None);
    }
}
