use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub(crate) enum MetricValue {
    Str(String),
    Num(f64),
    Arr(Vec<MetricValue>),
    Map(MetricMap),
}

impl MetricValue {
    pub(crate) fn as_f64(self) -> f64 {
        match self {
            Self::Num(x) => x,
            _ => panic!("not an f64"),
        }
    }

    pub(crate) fn as_map_mut(&mut self) -> &mut MetricMap {
        match self {
            Self::Map(x) => x,
            _ => panic!("not a map"),
        }
    }

    pub(crate) fn as_map(&self) -> &MetricMap {
        match self {
            Self::Map(x) => x,
            _ => panic!("not a map"),
        }
    }

    pub(crate) fn as_string(self) -> String {
        match self {
            Self::Str(x) => x.clone(),
            _ => panic!("not a string"),
        }
    }

    pub(crate) fn as_vec(self) -> Vec<MetricValue> {
        match self {
            Self::Arr(x) => x.clone(),
            _ => panic!("not a string"),
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

pub(crate) type MetricMap = HashMap<String, MetricValue>;
