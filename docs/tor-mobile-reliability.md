# Mobile connection recovery and profile updates

The follow-up implementation is described in [Tor delivery v2](tor-delivery-v2.md):
entrance racing, queue priority, profile thumbnails, timings and scheduled background delivery.

The Activity previously unregistered the only network callback in `onStop`.
The foreground service could keep an obsolete WebSocket while the default
network changed. Re-entering with `connect` reported the old session instead
of checking whether the transport still worked.

Network monitoring now uses the application context and the lifetime of the
core/service, including the fallback foreground-only poller. A changed network
debounces reconnect requests, replaces the main socket and invalidates cached
anonymous sockets and tasks. Stable encrypted envelopes and their client
references remain in the durable queue for idempotent retry. Explicitly
disconnected clients are not connected by these recovery commands.

Foreground resume sends a random WebSocket Ping. A matching Pong keeps the
connection; an eight-second probe timeout triggers reconnect. This replaces
neither the running Tor client nor the account. During connection backoff,
resume wakes the next attempt immediately. Onion mode retains onion routing.

Avatar publication is tracked until an own-profile response confirms the exact
published payload. Eligible peers then receive an encrypted change notification
and re-fetch the profile. The same path covers replacement and removal. A saved
pending publication survives a lost acknowledgement or reconnect; notifications
never contain the avatar or a public account name.

## Further work, in priority order

1. Treat Android process death and foreground-service budget exhaustion as
   separate states. Restore the permitted account when possible; otherwise show
   an explicit unlock/foreground requirement. Android 15 limits `dataSync`
   foreground services to six background hours per day; a sticky service alone
   cannot provide permanent background delivery:
   https://developer.android.com/develop/background-work/services/fgs/timeout
2. Add local, content-free timings for Tor bootstrap, onion handshake, tickets,
   queue wait and acknowledgement. Optimize measured tail latency instead of
   repeatedly rebuilding a healthy Tor client.
3. Race a bounded number of onion entrances through HELLO, select one before
   account authentication, and use short per-entrance failure budgets. Keep
   one-use anonymous sending sockets and avoid making the authenticated
   receiving socket an identifiable sending channel.
4. Separate small control messages from large attachments in queue scheduling;
   add resumable encrypted attachment transfers and preview-sized avatars.
5. Offer explicit delivery/privacy choices if users want a speed tradeoff.
   Never silently send directly when onion mode is selected. Tor latency also
   depends on relay congestion, beyond application-side recovery:
   https://support.torproject.org/tor-browser/general/tor-browser-speed/
