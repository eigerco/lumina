use instant::SystemTime;
use tendermint::Time;

// rely on instant to provide time for wasm32
pub(crate) fn now() -> Time {
    // this will panic if `::now()` is before UNIX_EPOCH, which shouldn't happen
    let since_epoch = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();

    // i64 can represent longer time range than is supported by tendermint anyway (year 9999,
    // limit comes from protobuf)
    let secs = since_epoch.as_secs().try_into().unwrap();
    let nanos = since_epoch.subsec_nanos();

    Time::from_unix_timestamp(secs, nanos).unwrap()
}
