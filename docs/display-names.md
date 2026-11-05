# Display names and local aliases

A display name is a mutable label, not an account address, username, verified
identity or registration handle. It has no uniqueness requirement and no server
index or search endpoint. Chat codes and device keys are unchanged. Names are
trimmed, limited to 80 Unicode scalar values, and reject controls and bidi overrides.

The own-profile label is editable by clicking its name. A peer profile name opens
the local-contact-name editor. Local aliases live only in the sealed directory;
clearing an alias returns to the peer's display name, then the chat-code fallback.
The precedence is local alias > peer display name > chat code/device fallback.
Fallback avatars use the first letters of the first two words (Андрей
Владимирович -> АВ), with code-point-safe handling on both platforms.

Profile-name visibility is respected. Public names are nullable profile metadata,
available to authenticated users who know a device/chat code, without name lookup.
For restricted visibility the public field is cleared, and permitted established
peers receive names in authenticated MLS control messages. Changed visibility sends
an empty control to withdraw the cached label. Already learned information cannot
be made unknown to another device. Local aliases are never replaced by these signals.

The server migration adds a nullable display_name column; it does not change the
chat code, avatar or account keys. Older clients ignore the added profile field.
Deploy the matching relay before using updated Android/Windows clients from this
monorepository. Standalone client UI copies are not updated by this change.

Validation includes local alias preservation of approval state, clearing aliases,
profile rename/clear with stable chat codes, and MLS delivery plus withdrawal of
names under visibility rules. Native device and Windows packaging tests remain
separate from source compilation and unit tests.
