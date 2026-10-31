# Avatar rendering and private profile interoperability

Android avatar backgrounds now scale the bitmap to cover their actual bounds
and center it before clipping. An untransformed CLAMP shader previously sampled
the edge pixels to the right and bottom instead of scaling a small image.
Portrait and landscape images keep their proportions, and changing the view's
size recalculates the transform.

Private profile keys are stored under the authenticated sender device's normal
hex identifier. Readers also accept the accidentally double-hexed identifiers
written by older clients. The owner reads their own profile key; it is not
looked up in the peer map. Both avatar and decoration decryption use this lookup.
Receiving a key persists the receive ratchet and requests the profile again,
so an earlier response without the key does not leave a permanently empty avatar.

Key recipients follow the avatar permission rule and its exceptions, excluding
blocked peers. Replacing a conversation group invalidates the corresponding
key-delivery marker so recovery can share the key through the new channel.

Crop zoom now bounds the image origin before deriving the image-space crop.
This keeps the frame stationary when pinching near an edge. JPEG size estimation
is delayed during zoom so each gesture sample does not encode another image.

Host tests cover avatar sizing and aspect ratios, owner and legacy-key
decryption, permission selection, and a received key's persisted ratchet.
The disposable `live_delivery --repair-missing --avatars` test verifies channel
repair, bidirectional delivery, and an owner's private avatar being decrypted
by that owner and an approved peer over Tor. Physical Android gesture rendering
still needs device coverage.
