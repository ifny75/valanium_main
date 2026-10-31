# Tor delivery v2

## Connection race

When the selected receive route is onion, two distinct configured onion
entrances race through WebSocket open and HELLO. Each attempt has a 45-second
budget and a malformed challenge cannot win. The losing future is dropped,
closing any socket it already opened. Authentication/registration occurs only
after choosing the winner. No extra authenticated session is created and no
direct route is introduced in onion mode. Auto still starts with its selected
direct route and races onion entrances only when it reaches onion fallback.

## Scheduling and profile thumbnails

The durable anonymous outbox prioritizes pending text over attachments and
keeps one transmission per recipient active. Welcome and epoch-change barriers
cannot be crossed. Attachments age into normal priority after 30 seconds, so
continuous short messages cannot starve them indefinitely. Already transmitting
frames are not interrupted. Exact ciphertext and stable references are retained
for retries; tests check that reordered application messages still decrypt.

New profile avatars are at most 384 pixels on the long side, with an 80,000-byte
base64 limit. Clients can normalize older own avatars after retrieving them,
with guards against overwriting a newer local avatar selection. The encrypted
profile format and avatar permissions remain unchanged. Existing large avatars
need one initial fetch before their owner can publish the compact replacement.

## Local measurements

`diagnostics` returns local stage timings: successful entrance connection,
authentication, durable queue wait and server acknowledgement after the first
send attempt. Retries are included in acknowledgement latency. A server ACK is
not a peer delivery receipt. Reports contain count, last duration, median and
95th percentile over at most 64 samples per stage, plus pending-envelope count.
No contact IDs, destinations or message contents appear in the report. Timings
are stored in the encrypted local database and are not uploaded.

## Android background delivery

The user can choose persistent messaging, periodic checks, or foreground-only
operation. The continuous service uses Android's `remoteMessaging` type for
ongoing inter-device text messaging instead of using `dataSync` indefinitely.
It remains a visible foreground service, subject to OS/user stops. Delivery
also works if notification-drawer permission is denied; Android still
shows the foreground task in its task manager, and the denied permission is not changed.
The service's cold restart can reopen an existing permitted account rather than polling an empty
native handle. A locked account is never unlocked by background restoration.

A separate persisted JobScheduler job checks the queue, requiring connectivity,
with a 15-minute minimum interval. Android may defer it because of Doze, quotas
or device power policy; this mode does not promise immediate notifications.
The job restores an existing account, starts the selected transport, consumes
the inbox and waits for the outbox to drain, with a bounded 150-second session.
It then disconnects unless the UI/service has taken ownership. The job never
creates an identity or starts a foreground service from the background. Logout
and foreground-only mode cancel scheduled work. User force-stop restrictions
are respected. No third-party push provider or battery-policy bypass is added.

References:
- https://developer.android.com/develop/background-work/services/fgs/service-types#remote-messaging
- https://developer.android.com/develop/background-work/services/fgs/timeout
- https://developer.android.com/develop/ui/compose/notifications/notification-permission
- https://developer.android.com/reference/android/app/job/JobInfo.Builder#setPeriodic(long,long)

Coverage includes stalled/failed entrance races and loser cancellation,
priority/epoch barriers and attachment aging, actual out-of-order MLS
decryption, bounded timing reports, Pixel thumbnail export and lifecycle tests,
live onion delivery, and a forced OS-scheduled job that restores a closed
disposable account and receives an offline message without Activity or FGS.
