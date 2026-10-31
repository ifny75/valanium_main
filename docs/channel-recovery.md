# Recovering a locally missing MLS channel

A conversation index can survive while its MLS group is missing from a stored
snapshot. Previously this produced `store not initialised` on send and failed
decryption on receive, even with a healthy Tor connection.

At authenticated startup and on the heartbeat, the client checks indexed direct
channels. Only a locally missing group triggers recovery. Blocked peers and
unconfirmed changed device keys are excluded. The normal KeyPackage claim and
device-binding validation establish a fresh encrypted group for the same device.
Welcome and an encrypted control marker use the existing delivery policy,
including anonymous onion delivery. No server-side privacy restrictions change.

Changing the group atomically stores the MLS snapshot and attaches the new group
to the previous conversation thread. Local history and healthy groups remain in
place. Control messages also persist their receive ratchet before ACK, including
unknown protocol extensions. Recovery does not reconstruct old ciphertext whose
decryption keys were already lost; such messages may need to be resent.

An Engine holds an exclusive SQLite OS lock on a per-database sidecar for its
entire worker lifetime. Another Engine, including in another process, fails
before opening the account. Shutdown or a process crash releases the lock. This
prevents concurrent clients from overwriting one another's MLS snapshots.

Regression coverage checks writer exclusion and release, repaired bidirectional
encryption after reopening, preservation of old history and healthy channels,
and durable control-frame ratchets. `live_delivery --repair-missing` additionally
fault-injects a missing group in disposable accounts against the live onion
entrances and verifies both directions after restart.
