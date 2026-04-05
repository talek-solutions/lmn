mod categorical;
mod latency;
mod numeric;
mod status_code;

pub use categorical::{CategoricalHistogram, CategoricalHistogramParams};
pub use latency::LatencyHistogram;
pub use numeric::{NumericHistogram, NumericHistogramParams};
pub use status_code::StatusCodeHistogram;
