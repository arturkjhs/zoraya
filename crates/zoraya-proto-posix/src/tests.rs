use super::*;
use zoraya_protocol::testing::exercise;
use zoraya_protocol::{Message, decode, encode};

/// Encodes the message, decodes it back, compares, and feeds the decoder
/// every damaged form of it.
macro_rules! round_trip {
    ($t:ty => $m:expr) => {{
        let m = $m;
        let mut out = [0u8; 4096];
        let length = encode(&m, 7, &mut out).unwrap();
        exercise(&out[..length], |b| {
            let _ = decode::<$t>(b, <$t as Message<'_>>::HANDLES);
        });
        let (header, back) = decode::<$t>(&out[..length], <$t as Message<'_>>::HANDLES).unwrap();
        assert_eq!(header.transaction, 7);
        assert_eq!(back, m);
    }};
}

#[test]
fn every_message_round_trips_and_decodes_totally() {
    round_trip!(SpawnPrepare => SpawnPrepare {
        group: Below(group::JOIN),
        pgid: 12,
        ignored: 1 << 20,
        fork: Below(1)
    });
    round_trip!(SpawnPrepared => SpawnPrepared {
        pid: 13,
        pgid: 12,
        sid: 2
    });
    round_trip!(SpawnPrepareIn => SpawnPrepareIn { ignored: 0 });
    round_trip!(SpawnPreparedIn => SpawnPreparedIn {
        pid: 3,
        pgid: 3,
        sid: 0
    });
    round_trip!(SpawnCommit => SpawnCommit { pid: 13 });
    round_trip!(ReportStats<'_> => ReportStats {
        counters: Data(&[1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0]),
        calls: Data(&[0x81, 0x01])
    });
    round_trip!(ReportStatsReply => ReportStatsReply { status: Below(status::OK) });
    round_trip!(SpawnCommitReply => SpawnCommitReply {
        status: Below(status::OK)
    });
    round_trip!(SpawnAbort => SpawnAbort { pid: 13 });
    round_trip!(SpawnAbortReply => SpawnAbortReply {
        status: Below(status::NO_SUCH_PROCESS)
    });
    round_trip!(ExecCommit => ExecCommit {});
    round_trip!(ExecCommitReply => ExecCommitReply {
        status: Below(status::OK)
    });
    round_trip!(Reap => Reap { pid: 13 });
    round_trip!(Reaped => Reaped {
        status: Below(status::OK),
        code: 0,
        reason: 1,
        detail: 2,
        kind: 1
    });
    round_trip!(SetPgid => SetPgid { pid: 13, pgid: 12 });
    round_trip!(SetPgidReply => SetPgidReply {
        status: Below(status::NOT_PERMITTED)
    });
    round_trip!(Lookup => Lookup {
        what: Below(lookup::STOP_SIGNAL),
        pid: 13
    });
    round_trip!(LookupReply => LookupReply {
        status: Below(status::OK),
        value: 20
    });
    round_trip!(SetStopDisposition => SetStopDisposition {
        signal: 20,
        disposition: Below(stop::HANDLER)
    });
    round_trip!(SetStopDispositionReply => SetStopDispositionReply {
        status: Below(status::OK)
    });
    round_trip!(Kill => Kill {
        target: Below(target::GROUP),
        id: 12,
        signal: 2
    });
    round_trip!(KillReply => KillReply {
        status: Below(status::OK)
    });
    round_trip!(Tcsetpgrp => Tcsetpgrp { pgid: 12 });
    round_trip!(TcsetpgrpReply => TcsetpgrpReply {
        status: Below(status::NO_TERMINAL)
    });
    round_trip!(Tcgetpgrp => Tcgetpgrp {});
    round_trip!(TcgetpgrpReply => TcgetpgrpReply {
        status: Below(status::OK),
        pgid: 12
    });
    round_trip!(StopSelf => StopSelf { signal: 21 });
    round_trip!(StopSelfReply => StopSelfReply {
        status: Below(status::OK)
    });
    round_trip!(Statistics => Statistics {});
    round_trip!(StatisticsReply => StatisticsReply {
        prepares: 1,
        commits: 2,
        aborts: 3,
        execs: 4,
        reaps: 5,
        kills: 6,
        others: 7,
        refused: 8,
        live: 9,
        next_pid: 10
    });
}

#[test]
fn out_of_range_fields_are_refused_by_the_encoder_and_the_decoder() {
    let mut out = [0; 256];
    let bad = SpawnPrepare {
        group: Below(group::COUNT),
        pgid: 0,
        ignored: 0,
        fork: Below(0),
    };
    assert!(encode(&bad, 1, &mut out).is_err());
    let good = Kill {
        target: Below(target::ALL),
        id: 0,
        signal: 9,
    };
    let n = encode(&good, 1, &mut out).unwrap();
    // The target is the body's first word.
    out[zoraya_protocol::HEADER_SIZE] = target::COUNT as u8;
    assert!(decode::<Kill>(&out[..n], 0).is_err());
}

#[test]
fn a_message_with_the_wrong_handle_count_is_refused() {
    let mut out = [0; 256];
    let n = encode(&SpawnCommit { pid: 3 }, 1, &mut out).unwrap();
    assert!(decode::<SpawnCommit>(&out[..n], 0).is_err());
    assert!(decode::<SpawnCommit>(&out[..n], 1).is_ok());
}

#[test]
fn the_ordinals_fit_the_statistics() {
    assert!((Statistics::ORDINAL as usize) < ORDINALS);
}
