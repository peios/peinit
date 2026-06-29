use super::model::{JobId, OperationId};
use super::uuid_v7::UuidV7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdAllocationError {
    CountOverflow { count: usize },
    SequenceExhausted { next_sequence: u64, count: usize },
    TimestampOverflow { observed_at_ns: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationIdAllocator {
    next_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobIdAllocator {
    next_sequence: u64,
}

impl OperationIdAllocator {
    pub fn new() -> Self {
        Self { next_sequence: 0 }
    }

    #[cfg(test)]
    pub(crate) fn with_next_sequence(next_sequence: u64) -> Self {
        Self { next_sequence }
    }

    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    pub fn allocate_batch(
        &mut self,
        count: usize,
        observed_at_ns: u64,
    ) -> Result<Vec<OperationId>, IdAllocationError> {
        allocate_batch(&mut self.next_sequence, count, observed_at_ns, OperationId)
    }
}

impl Default for OperationIdAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl JobIdAllocator {
    pub fn new() -> Self {
        Self { next_sequence: 0 }
    }

    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    pub fn allocate_batch(
        &mut self,
        count: usize,
        observed_at_ns: u64,
    ) -> Result<Vec<JobId>, IdAllocationError> {
        allocate_batch(&mut self.next_sequence, count, observed_at_ns, JobId)
    }
}

impl Default for JobIdAllocator {
    fn default() -> Self {
        Self::new()
    }
}

fn allocate_batch<T, F>(
    next_sequence: &mut u64,
    count: usize,
    observed_at_ns: u64,
    wrap: F,
) -> Result<Vec<T>, IdAllocationError>
where
    F: Fn(UuidV7) -> T,
{
    if count == 0 {
        return Ok(Vec::new());
    }
    let count_u64 = u64::try_from(count).map_err(|_| IdAllocationError::CountOverflow { count })?;
    next_sequence
        .checked_add(count_u64)
        .ok_or(IdAllocationError::SequenceExhausted {
            next_sequence: *next_sequence,
            count,
        })?;

    let mut ids = Vec::with_capacity(count);
    for offset in 0..count_u64 {
        ids.push(wrap(UuidV7::from_seed(
            observed_at_ns,
            *next_sequence + offset,
        )?));
    }
    *next_sequence += count_u64;
    Ok(ids)
}
