# Calls: encrypted capability signaling

Phase one adds `call_probe` and `call_status` to the shared core and a preview
entry point to both clients. It does not implement audio calls. The UI explicitly
states this before sending a capability check; the microphone is never opened.

The probe and its `unavailable` response are MLS control messages. They use the
existing encrypted conversation and anonymous outbox, not a new signaling server.
They do not appear in conversation history. Incoming checks obey the recipient's
`calls` privacy rule; blocking and device-key checks continue to apply.

The versioned signal accepts only a bounded random identifier, action and expiry.
Unknown fields and versions are rejected. In particular, the schema contains no
IP address, ICE candidate, SDP or media URL. Responses must match an outstanding
request and the authenticated MLS sender device. Requests expire after 90 seconds;
pending and replay-tracking maps are bounded and held in memory. Clients stop
showing a pending check after a timeout; older clients may simply ignore it.

Unit tests cover malformed signals, expiry, replay and sender binding. The isolated
Pixel live test exchanges messages through embedded Tor, performs the capability
check, loads a peer profile and compares verification payloads. UI tests cover the
preview, without granting microphone access or contacting real user accounts.

The next phase needs explicit incoming/outgoing call states, user consent, audio
capture and playback, echo cancellation, interruption handling and a media relay
design consistent with the selected privacy mode. Direct peer connectivity must
not silently disclose network addresses when Tor is selected. Do not advertise
working audio calls until media and that privacy contract are implemented and
tested on both platforms.
