//! Bounded, local-only stage timings. No addresses, contacts or message contents.
use std::collections::BTreeMap;
use serde::{Serialize,Deserialize};
use crate::{store::Store,Result};
#[derive(Default,Serialize,Deserialize)]
struct Samples(BTreeMap<String,Vec<u64>>);
pub(crate) fn record(store:&Store,stage:&str,millis:u64)->Result<()> {
    let mut samples:Samples=store.load_setting("latency_v1")?.and_then(|v|serde_json::from_slice(&v).ok()).unwrap_or_default();
    let values=samples.0.entry(stage.into()).or_default();if values.len()>=64{values.remove(0);}values.push(millis.min(3_600_000));
    store.save_setting("latency_v1",&serde_json::to_vec(&samples)?)
}
pub(crate) fn report(store:&Store)->Result<serde_json::Value>{
    let samples:Samples=store.load_setting("latency_v1")?.and_then(|v|serde_json::from_slice(&v).ok()).unwrap_or_default();
    let mut report=serde_json::Map::new();
    for (stage,values) in samples.0 {if values.is_empty(){continue;}let last=*values.last().unwrap();let mut sorted=values;sorted.sort_unstable();let n=sorted.len();
        report.insert(stage,serde_json::json!({"count":n,"last_ms":last,"p50_ms":sorted[(n-1)/2],"p95_ms":sorted[(n*95).div_ceil(100)-1]}));}
    Ok(serde_json::Value::Object(report))
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn timings_are_bounded_and_report_percentiles(){let store=Store::open(":memory:",b"timing").unwrap();for n in 0..100{record(&store,"connection",n).unwrap();}let report=report(&store).unwrap();assert_eq!(report["connection"]["count"],64);assert_eq!(report["connection"]["last_ms"],99);assert_eq!(report["connection"]["p95_ms"],96);}
}
