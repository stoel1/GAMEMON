use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorStatus { Available, Unsupported, PermissionDenied, Unavailable, Invalid, Error }

#[derive(Debug, Clone)]
pub struct SensorReading<T> {
    pub value: Option<T>,
    pub unit: &'static str,
    pub source: String,
    pub status: SensorStatus,
    pub sampled_at: SystemTime,
}

impl<T> SensorReading<T> {
    pub fn available(value: T, unit: &'static str, source: impl Into<String>) -> Self {
        Self { value: Some(value), unit, source: source.into(), status: SensorStatus::Available, sampled_at: SystemTime::now() }
    }
    pub fn unavailable(unit: &'static str, source: impl Into<String>, status: SensorStatus) -> Self {
        Self { value: None, unit, source: source.into(), status, sampled_at: SystemTime::now() }
    }
}
