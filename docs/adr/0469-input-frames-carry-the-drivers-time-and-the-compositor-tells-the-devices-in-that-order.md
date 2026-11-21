# ADR-0469: Input frames carry the driver's time, and the compositor tells the devices' waiting frames in that order

- **Status:** proposed
- **Date:** 2026-11-21
- **Depends on:** ADR-0092, ADR-0093, ADR-0094, ADR-0467
- **Evidence:** alternatives-postmortem.md#genode

## Context

`make test-gui GUI_RUNS="files-manage:auto:2"` failed on `main` (CI run 44859568963, the full run of `add6681b`, and the
candidate run 44860464670) at step 22, `mclick shift 409 316`: the file manager printed `status 4 items · c.txt selected (4 B)`,
a plain click on c.txt, where a Shift-click selects three rows. The runner sends Shift down, waits 400 ms (`MODIFIER_SETTLE`),
sends the press and the release, waits 400 ms and sends Shift up; the keyboard and the tablet are two virtio-input devices, two
`inputd` processes and two channels into `compd`. At scale 2 on a slow runner `compd` was more than 400 ms behind (its log shows
frames of up to a second), so both channels held their frames when a round began.

ADR-0467 (amended 2026-11-21) ends a device's round at its first key, button or wheel, so that the devices stay in order across a
backlog, but a round served the devices in the order `port_wait` returned their packets. With the tablet's packet first, the press
was told before the keyboard's Shift down, which had come 400 ms earlier. `input::Frame` carried no time, so the compositor had
nothing to order two devices' frames by: ADR-0467 recorded this as its first limit, and the review of `e5d1c3c3` as its finding 1.
The host tests' fake served the keyboard (slot 0) first, so the case was never met there. The other order is wrong too: served
keyboard first, a click followed by Shift and a key gives the press the Shift that came after it.

**Known:** both drivers read the same monotonic clock (`clock_monotonic`); a driver wakes on its device's interrupt and sends one
frame per `SYN_REPORT`; virtio-input's event carries no time of its own (virtio 1.2's `virtio_input_event` is a type, a code and
a value). **Assumed:** a driver runs within a few milliseconds of its interrupt, far less than the 400 ms between the runner's
Shift and its click, while the compositor may be a second behind.

## Decision drivers

1. **A modifier and a click on two devices are one gesture**: the order the person made them in is what the client must hear,
   however far behind the compositor is (ADR-0094, the routing of input to the focused window).
2. **The compositor is the one that orders input** (ADR-0093): a driver only translates (ADR-0092); it must not have to know about
   other devices.
3. **No new trust**: an input driver may already send any key (ADR-0092); whatever it sends as a time must not let it do more than
   delay what comes after it.
4. **Small and reversible**: one field in one message; the bounds of ADR-0467 (32 frames of a device a round, a key ends the round)
   stay.

## Considered options

### Option A: the kernel stamps each event, as Linux's evdev does (the baseline)
- **Description:** Linux's `struct input_event` carries the time the kernel's driver reported it; libinput and the compositors
  merge devices by it. Here the drivers are processes, so the kernel would stamp the interrupt and give the time with it.
- **Evidence for:** the time closest to the device; the order of interrupts, not of the drivers' runs.
- **Evidence against:** a new kernel interface (a time in the interrupt object or the port packet) for one consumer; a driver
  frames several events per interrupt and would still have to carry the time to the compositor.
- **Cost:** a kernel ABI change, larger than the problem (estimate).

### Option B: the driver stamps each frame with the time it woke for it; the compositor tells the earliest first (recommended)
- **Description:** `input::Frame` gains `time: u64`, the monotonic clock in nanoseconds when the driver woke for the interrupt
  whose frames it is sending. Each round the compositor reads the next frame of every input device (its head), and tells the
  earliest head (of equal times, the first device's), then reads that device's next, until a frame tells a key, changes a button
  or turns a wheel (ADR-0467's end of the round, now for all devices at once), or the earliest head's device has had 32 frames
  this round, or no device has one. A head read and not told waits for the next round, which does not wait on the port. A device's
  own frames keep the order of its channel whatever their times: the time only chooses between devices. A time later than the
  compositor's clock when it reads the frame is taken as that clock. A head past which 512 frames of other devices were told
  (`MAX_PASSED`: when it was read the others held at most one head and a channel's queue of 64 each, and an honest frame sent
  later has a later time, but for one wake's frames more, at most 32 since `inputd`'s `poll` gives the device its 64 buffers
  back only at its end; so at most 3 by 97) goes first whatever the times: an honest backlog never reaches it,
  drained at any speed, and a driver whose times are too early cannot hold another device back for longer. A time that is 0, later than the clock or before the driver's time before is
  counted (`Stats::odd_input_times`) and `compd` says so once a boot. Before a head that may end the round is told, the devices
  whose channels were empty are read again, so that a frame sent during the round with an earlier time is not left behind it. A
  pointer move waiting before a key is told to the window under it before the key (with the modifiers it was made with).
- **Evidence for:** fixes both orders (Shift then click, click then Shift) whichever device the port serves first; one field and
  one clock read in `inputd`; no kernel change.
- **Evidence against:** two frames whose drivers ran in the other order than their interrupts (both within the same few
  milliseconds) are told in the drivers' order; a frame not yet sent when the compositor reads is not waited for. The time is
  taken once a wake, before the driver drains its queue: an event the device adds during the drain carries the earlier time
  (a window of the drain's length, microseconds). A driver whose times are too early holds another device's frame back by at
  most 512 of its frames (and the other aged heads) from that frame's read: 16 rounds, or 512 if every flood frame is a key; a
  device with k frames queued waits k times that (decision driver 3: it could already type any key). A frame sent during a round with an
  earlier time is caught only before a head that may end the round, not before a move while a button is held (which then
  carries the modifiers of the frames already told).
- **Cost:** one `u64` per frame; a non-blocking read of every device at the round's start, and of every device without a head
  before each head that holds a key, button or wheel: at most 4 by 32 by 4, 512 reads a round in the worst case, usually a few
  (estimate: microseconds each).

### Option C: no time; a rule for modifiers
- **Description:** each round, read all keyboards' frames up to their first non-modifier key and apply their modifier changes
  before any pointer frame.
- **Evidence against:** wrong for a click followed by Shift (the press gets the later Shift) and for Shift up pending behind a
  click (the press loses its Shift); the order of a click and a typed key across a focus change is still the port's.

### Option D: one input multiplexer in front of the compositor
- **Description:** as Genode's `input_filter` merges several input sessions into one stream (alternatives-postmortem.md#genode),
  a process reads every driver and sends one channel to the compositor.
- **Evidence against:** the multiplexer meets the same problem (two channels, no time) one process earlier; one process more in
  the input path.

## Recommendation

Option B. It is the smallest change that makes the order the drivers saw the order the client hears, for both adverse cases, and it
keeps ADR-0467's bounds. In terms of ADR-0467's recommendation item 2: a key, button or wheel now ends the round's input for every
device, not only the device that sent it (a device stopped alone would let another device's later frames pass its earlier ones).

## Consequences

**Positive:** a Shift-click (or Command-click) waiting in two devices is told as made, whichever device the port serves first; a
click followed by a key reaches the window the click focused. ADR-0467's first limit goes.

**Negative:** the round's input stops at the first key of any device, so two devices with keys waiting are told one key a round
(rounds that compose nothing cost microseconds); the stamp is the driver's, so the order of two drivers that ran late in the
other order than their interrupts is theirs.

**Follow-up:** none required. Option A if a measured case shows the drivers' run order differing from the interrupts'. The sheet's
guard (ADR-0245) compares the compositor's clock when a press is told, not the frame's time: with a backlog longer than the guard,
a press made before a sheet mapped is not guarded (not made worse here; the frame's bounded time could be used).

## Invariants

- **INV-0469-1.** Frames of two input devices waiting in the same round are told in the order of their drivers' times (of equal
  times, the first slot's first), and each device's own frames in the order of its channel, whichever order the port serves the
  devices in. *Check:* `cargo test -p zoraya-compositor` (`a_modifier_click_waiting_in_two_devices_keeps_its_order`,
  `a_click_before_a_modifier_waiting_in_two_devices_keeps_its_order`, `equal_and_backward_times_keep_each_devices_order`: each
  with the fake's port serving the keyboard first and the tablet first, and with either device in slot 0).
- **INV-0469-2.** A frame read and not told is told in the next round, which does not wait on the port for it; a round tells at most
  32 frames of a device and ends its input at the first frame that tells a key, changes a button or turns a wheel. *Check:*
  `cargo test -p zoraya-compositor` (`a_frame_read_and_not_told_is_told_next_round_without_a_wait`,
  `a_round_reads_at_most_max_drain_messages_of_a_device`, `a_wheel_message_ends_the_devices_round`).
- **INV-0469-3.** No driver's times hold another device's frame back past `MAX_PASSED` (512) frames of other devices told after
  it was read (and the other heads aged with it), and an honest backlog never reaches that, however slowly it drains; frames read together keep their times' order at any backlog; a time later than the clock is taken as
  the clock; a time that is 0, later or backwards is counted; a frame sent during a round with a time before the head that ends it
  is told first; a pointer move waiting before a key is told before the key, without the key's modifiers. *Check:*
  `cargo test -p zoraya-compositor` (`a_flood_with_early_times_holds_another_device_back_a_bounded_while`,
  `a_slow_drain_of_an_honest_backlog_keeps_the_order`, `an_honest_backlog_of_two_devices_passes_a_held_head`,
  `each_head_counts_the_frames_passed_from_its_read`, `frames_read_together_keep_their_order_at_any_backlog`, `a_future_time_is_taken_as_when_the_frame_was_read`,
  `a_frame_sent_during_the_round_is_told_before_a_later_press`,
  `a_move_before_a_key_is_told_before_it_with_the_modifiers_before_it`, `each_move_in_a_frame_is_told_before_the_key_after_it`).

## Open questions

- Whether the drivers' run order ever differs from their interrupts' by more than the runner's settle time on a loaded host: not
  measured (the local reproduction delayed only `compd`); Option A answers it if it does.
- `inputd`'s own stamp is checked by no test: the host tests build their frames, and `t_gui` stamps its own. A driver that sends
  0 or a stopped clock shows as `compd`'s one line (`an input driver's frame time is odd`); no GUI run yet fails on that line.
- The measurement's idle rounds (`compd`'s `/config/gui-measure` line) count a round with a frame read and not told as not idle;
  no test checks that, and a frame that is no `Frame` is not counted as a message.

## Revisit triggers

- A GUI step that fails with two devices' frames told in an order other than the runner's while both drivers' times say so.
- A driver that frames events from more than one interrupt in one message.

## Architect's decision

(Left empty for the human.)
