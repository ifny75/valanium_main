# Anonymous delivery latency fixes

Each message still uses its own onion WebSocket and bearer ticket; there is no
fallback to authenticated SEND and no reduction of SOCKS/Tor circuit isolation.
A bounded pool prepares at most two anonymous sockets before a message exists.
They contain no identity, ticket, recipient or ciphertext, are each used once,
and expire 45 seconds after preparation starts. Completion wakes the message
queue immediately, independently of heartbeat. A send waits for an existing
preparation rather than opening a competing cold connection; preparation failures
release the slot and schedule a retry after two seconds. Preparation has the existing 25-second timeout. This moves connection
setup off the send path when preparation has completed; it cannot guarantee Tor
latency, especially immediately after startup or in a burst.

Failed envelopes retry after 2, 4, then 8 seconds using a dedicated deadline,
independent of heartbeat. Other recipients continue sending; the same recipient
retains FIFO ordering. A failed route is avoided for 30 seconds when another
route is available. These measurements and cooldowns are memory-only.

Tickets are discarded locally when expired or with less than 30 seconds of
remaining validity (25-second send budget plus margin). Expiry is a scheduling
hint; the server still verifies the signature. This requires a reasonably correct
client clock. Missing stock triggers one outstanding request at a time, retried
on a dedicated ten-second deadline; reconnect clears the outstanding request.
The stock is bounded to 32 tickets. The wire format and server do not change.

Set `VALANIUM_DELIVERY_TIMINGS=1` to enable local stderr durations for transport
connection, anonymous WebSocket/HELLO and server ACK. Logs contain stage names
and milliseconds, no addresses, identities, message references or plaintext.
This is transport timing, not a measurement of peer delivery or Tor bootstrap.

## Pass exchange before restricting incoming messages

Pass gifts are generated when an encrypted contact channel becomes available,
as well as at reconnect. Before first enabling restricted incoming messages,
existing contacts with channels must have mutual passes and no pending essential
anonymous envelopes. An incomplete exchange produces
`privacy_pass_exchange_pending` and leaves the settings unchanged. Keep both
clients connected to exchange gifts, then retry the setting. The new guard also
works offline; it does not silently queue a privacy setting while leaving the
server open.

An account already restricted before this update is never silently reopened.
If both peers were already restricted without mutual passes, use existing
out-of-band pass invitations to restore admission; this patch does not bypass
that access policy or recover a missing bearer secret.

## Server and client rollout

Both server repositories include anonymous tickets, recipient-bound anonymous
passes, retry deduplication, and ACK-driven queue draining beyond 200 envelopes.
Backpressure resumes through the WebSocket drain handler. The standalone server
must be updated before relying on anonymous delivery there. Android and Windows
must use the immutable core revision containing these fixes, then be rebuilt and
distributed; updating GitHub alone does not update installed applications.

Unit tests cover anonymous socket frames without AUTH, retry deadlines, the pass
activation barrier, a 451-envelope backlog with reordered/duplicate ACKs and
transport backpressure. Real Tor two-device latency, Android packaging and a
Windows build still require platform/network testing before release.
