//! Availability accompanies raw scalars; zero is never a missing-value sentinel.

use crate::row::Field;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    Available,
    WarmingUp,
    PermissionDenied,
    Unsupported,
    ReadError,
}

impl Availability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::WarmingUp => "warming_up",
            Self::PermissionDenied => "permission_denied",
            Self::Unsupported => "unsupported",
            Self::ReadError => "read_error",
        }
    }

    pub fn from_error(error: &std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            std::io::ErrorKind::Unsupported => Self::Unsupported,
            _ => Self::ReadError,
        }
    }

    pub const fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Metric {
    Cpu,
    Memory,
    Rss,
    Virtual,
    Time,
    Priority,
    Threads,
}

impl Metric {
    pub const fn for_field(field: Field) -> Option<Self> {
        match field {
            Field::PercentCpu | Field::PercentNormCpu => Some(Self::Cpu),
            Field::PercentMem => Some(Self::Memory),
            Field::ResMem => Some(Self::Rss),
            Field::VirtMem => Some(Self::Virtual),
            Field::Time => Some(Self::Time),
            Field::Priority => Some(Self::Priority),
            Field::Nlwp => Some(Self::Threads),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MetricAvailability([Availability; 7]);

impl MetricAvailability {
    pub const fn all(status: Availability) -> Self {
        Self([status; 7])
    }

    pub fn get(&self, metric: Metric) -> Availability {
        self.0[metric as usize]
    }

    pub fn set(&mut self, metric: Metric, status: Availability) {
        self.0[metric as usize] = status;
    }

    pub fn field(&self, field: Field) -> Availability {
        Metric::for_field(field).map_or(Availability::Available, |m| self.get(m))
    }

    pub fn inherit_memory(&mut self, source: &Self) {
        for metric in [Metric::Memory, Metric::Rss, Metric::Virtual] {
            self.set(metric, source.get(metric));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::row::Row;

    #[test]
    fn production_rows_start_unread_while_fixtures_are_explicitly_available() {
        let row = Row::new(7);
        assert_eq!(
            row.availability.field(Field::PercentCpu),
            Availability::ReadError
        );
        let fixture = Row::default_for_test(7);
        assert_eq!(fixture.percent_cpu, 0.0);
        assert!(fixture.availability.field(Field::PercentCpu).is_available());
    }

    #[test]
    fn statuses_are_independent_and_preserve_reasons() {
        let mut fields = MetricAvailability::all(Availability::Available);
        fields.set(Metric::Cpu, Availability::WarmingUp);
        fields.set(Metric::Rss, Availability::PermissionDenied);
        fields.set(Metric::Priority, Availability::Unsupported);
        fields.set(Metric::Threads, Availability::ReadError);
        assert_eq!(fields.field(Field::PercentNormCpu), Availability::WarmingUp);
        assert!(fields.field(Field::Time).is_available());
        assert_eq!(fields.get(Metric::Rss).as_str(), "permission_denied");
        assert_eq!(fields.get(Metric::Priority).as_str(), "unsupported");
        assert_eq!(fields.get(Metric::Threads).as_str(), "read_error");
        assert_eq!(
            Availability::from_error(&std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            Availability::PermissionDenied
        );
    }
}
