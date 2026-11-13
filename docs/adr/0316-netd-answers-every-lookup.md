# ADR-0316: `netd` answers every request that needs a lookup: a timed-out lookup says so, and clients wait for its bound

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0221, ADR-0222, ADR-0223, ADR-0225, ADR-0281, ADR-0330
- **Evidence:** CP-12

## Context
The architect's hand run (`make run-gui`, the Internet through SLIRP): right after boot, `ping google.com` printed
"ping: the network service did not answer"; `ping 142.250.204.174` then worked, and so did `ping google.com` after it.
Waiting a minute after boot changed nothing, so the lease was not the cause.

Measured, on a fresh boot whose first command is `ping google.com` (`--net-pcap`, times in seconds):

```
34.118415 ARP, Request who-has 10.0.2.3 tell 10.0.2.15
34.118419 ARP, Reply 10.0.2.3 is-at 52:55:0a:00:02:03
34.118493 IP 10.0.2.15.38022 > 10.0.2.3.53: A? google.com.
34.129880 IP 10.0.2.3.53 > 10.0.2.15.38022: A 142.251.141.174
46.933743 ARP, Request who-has 10.0.2.2 tell 10.0.2.15
46.933938 IP 10.0.2.15 > 142.251.141.174: ICMP echo request, id 23122, seq 0
```

The query was not lost: smoltcp keeps a datagram whose neighbour is unknown in its socket and sends it as soon as the
ARP reply fills its cache (75 µs here). The answer came 11 ms later, and nothing left for 12.8 s, the client's wait.
The cause is in `netd`'s loop: the answer is read inside a turn, after the stack's poll; the run of a `ping` by name
starts there, and the loop slept on the delay computed before the run, which with no other timer was none at all. The
run waited for something else to wake `netd`; the client gave up first, and its exit (the channel's close) woke the
loop, which ended the expired run unsent. The second `ping google.com` found the name in the cache and started its run
inside the request, before the turn's poll: that path never had the defect. The echo request of the run above (id
`0x5a52`) is the second command's.

Two smaller faults showed on the way. The first `fetch` by name of a boot sent its SYN 1.0 s after the answer: smoltcp
asks for one neighbour per second (the rate limit of its neighbour cache), and the lookup's own ARP request for the
DNS server had just used that second, so the gateway's waited. And the protocol could not say that a lookup timed
out: `Unresolvable` covered "no such name" and "no server answered" alike, and the clients' waits were constants of
their own (`ping` waited `2 * WAIT_NS` for a lookup, `connect` 25 s), equal to `netd`'s bound only by chance.

## Decision drivers
1. A client must tell a service that is gone from a network that is slow (CP-12: an error is a value at the request
   site, and silence is no value).
2. One definition of each bound: the resolver's give-up time and what the clients wait for are the same number.
3. A first command of a boot must work as the second does: the stack's own pace (smoltcp's rate limit, the
   resolver's retry at 1 s) must not decide whether a first lookup succeeds.

## Considered options

### Option A: The Unix baseline: the client waits as long as the resolver's own retries take
- **Description:** glibc and musl resolve in the process, with `timeout` and `attempts` from `resolv.conf`; a
  timeout is `EAI_AGAIN`. Here the client would only lengthen its wait.
- **Evidence for:** familiar; no protocol change.
- **Evidence against:** it leaves the cause (the lost wake-up) and the conflation of "no such name" with "timed out".
- **Cost:** none.
- **Precedent:** every libc.

### Option B: `netd` always answers, with a status for a timed-out lookup; clients wait for `netd`'s bound
- **Description:** the loop comes back at once when a run starts after its poll; a lookup no server answered within
  `LOOKUP_US` (8 s) is answered `status::TIMED_OUT` (`Resolved`, `PingReply`) or `reason::LOOKUP_TIMED_OUT` (`Failed`
  of a `connect`); `zoraya-proto-net` holds `LOOKUP_US`, `PING_INTERVAL_US`, `CONNECT_ATTEMPT_US` and
  `ping_answer_us`, which `netd` uses and every client's wait derives from, plus a margin. A client's "did not answer"
  then means a `netd` that is gone or stuck. `netd` also asks for the gateway and the DNS servers on the link with an
  ARP request when the interface is configured, and for the gateway when a lookup goes out (at most once in 30 s).
- **Evidence for:** the client says what happened ("the name lookup timed out", `EAI_AGAIN`, `ETIMEDOUT`); the bound is
  one number; the first lookup and the connection after it do not wait on smoltcp's second.
- **Evidence against:** `Code` widens to 9 values and `Reason` to 8 (wire change, all in this tree); a broadcast ARP
  request per configuration and per lookup at most every 30 s.
- **Cost:** small: a status, a reason, constants, a delay, an ARP frame builder.
- **Precedent:** Fuchsia's name lookup returns a distinct timeout error; resolvers keep "no such name" (NXDOMAIN) and
  "server failure or timeout" apart.

### Option C: Clients do not wait at all: the reply to a lookup comes as an event on a channel of its own
- **Description:** `Resolve` and `Ping` reply at once with "pending", and the result comes later on another channel, as
  `connect`'s `Connected`/`Failed` do.
- **Evidence for:** no client wait to size.
- **Evidence against:** a second channel per request for what is one request and one reply (ADR-0055); the lost
  wake-up would remain.
- **Cost:** a protocol change for every lookup client.
- **Precedent:** Plan 9's `cs` answers on the same file.

## Recommendation
**Recommendation (not a decision): Option B.**

1. **No lost wake-up.** `Service::answer_lookups` takes the turn's lookup answers and its delay; when a run started
   there, the delay is zero, so the next turn sends the first request and counts the run's own time.
2. **A timed-out lookup is a status.** `status::TIMED_OUT` (8) for `Resolved` and `PingReply`, `reason::LOOKUP_TIMED_OUT`
   (7) for `Failed`; `Unresolvable` stays for no such name, no A record or a server that said it failed. `libposix`
   maps them to `EAI_AGAIN` and `ETIMEDOUT`. `ping` and `fetch` print "the name lookup timed out"; "the network
   service did not answer" is kept for no reply within the wait.
3. **One bound.** `zoraya_proto_net::LOOKUP_US` is the resolver's give-up time; `ping_answer_us(by_name, count,
   timeout)` is the longest a `ping` reply takes; `CONNECT_ATTEMPT_US` is one address's attempt. `ping` waits
   `ping_answer_us` plus 5 s, `connect` `LOOKUP_US + CONNECT_ATTEMPT_US` plus 5 s.
4. **Neighbours known before they are needed.** When the interface is configured (a lease or a static address),
   `netd` sends an ARP request for the gateway and each DNS server on the link; when a lookup goes out, for the
   gateway if its last request is older than 30 s (smoltcp forgets a neighbour 60 s after its last frame).
5. **The test.** `make test-net-lookup` boots the test image against `tools/test-dns`, which answers late or never;
   its `netd` (feature `net-test` only) takes the server's port from `/config/net-test-dns-port`, because port 53
   needs root on the host. The capture is checked by `zoraya-test-dns --check-pcap`.

**Tensions with accepted ADRs.**
- *ADR-0223 item 4* names `Unresolvable { reason: Local }`; the new status sits beside it and leaves the local rule as
  it is. Item 7's "a query in flight is a record with a deadline" is what makes the answer at the bound possible.
- *ADR-0222*'s messages are unchanged; only the ranges of `Code` and `Reason` grow.
- *ADR-0281 item 3*'s echo request is unchanged; `PING_INTERVAL_US` moves to the protocol crate.

### Effect of the vision
For target 1 (a person at the shell) the first command of a boot behaves as every later one; the error says what
failed. Targets 2 and 3: an agent or a desktop program that resolves gets `EAI_AGAIN` for a slow network instead of
`EAI_NONAME`, and can retry. Threads: none.

## Consequences
- **Positive.** The first `ping NAME` and `fetch` by name of a boot work; a slow server is reported as such; the clients'
  waits follow `netd`'s bound if it changes.
- **Negative.** A name with several addresses that all stay silent still takes `netd` one attempt each, longer than
  `connect`'s wait; the client then says `TimedOut` while `netd` still tries. ARP requests that smoltcp would not have
  sent.
- **Follow-up.** An overall bound on a `connect` by name over all its addresses; a lookup asked before the lease is
  there is answered `UNAVAILABLE` at once rather than waiting for it.

## Invariants
- **INV-0316-1.** A lookup no server answers is answered when the resolver gives up (`LOOKUP_US`), with
  `status::TIMED_OUT` for `resolve` and `ping` and `reason::LOOKUP_TIMED_OUT` for `connect`, never `Unresolvable`.
  *Check:* `cargo test -p zoraya-net` (a silent server), and `make test-net-lookup` (`ping` and `fetch` of a name the
  test server never answers print "the name lookup timed out").
- **INV-0316-2.** A `ping` by name whose answer comes over the wire sends its first echo request without waiting for
  another wake-up of `netd`'s loop. *Check:* a host test that drives the loop as `netd` does (woken only by a frame or
  its delay), and the capture of `make test-net-lookup` (the echo request and the SYN leave within 0.5 s of their
  answers).
- **INV-0316-3.** When the interface is configured, `netd` asks for the gateway and the DNS servers on the link once;
  the first query to the server is preceded by exactly one ARP request for it. *Check:* `cargo test -p zoraya-net` (the
  stack's probes) and the capture of `make test-net-lookup`.

## Open questions
- Whether a lookup asked before the first lease should wait for it (up to the lookup bound) rather than fail at once.
- Whether `connect` by name should have one bound over all its addresses, which its client's wait could then follow.

## Revisit triggers
- A resolver of its own process (ADR-0223's Option B): the bound and the status move with it.
- smoltcp exposing its neighbour cache, which would let the probes ask only for what is missing.

## Architect's decision
