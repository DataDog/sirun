use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize, Clone)]
#[serde(untagged)]
pub(crate) enum MetricValue {
    Str(String),
    Num(f64),
    Arr(Vec<HashMap<String, MetricValue>>),
}

impl MetricValue {
    pub(crate) fn as_f64(self) -> f64 {
        match self {
            Self::Num(x) => x,
            _ => panic!("not an f64"),
        }
    }
}

impl From<String> for MetricValue {
    fn from(string: String) -> Self {
        MetricValue::Str(string)
    }
}

macro_rules! num_type {
    ($type:ty) => {
        impl From<$type> for MetricValue {
            fn from(num: $type) -> Self {
                MetricValue::Num(num as f64)
            }
        }
    };
}
num_type!(i32);
num_type!(i64);
num_type!(f64);
