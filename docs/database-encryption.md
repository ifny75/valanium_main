# Database encryption: implementation status

`full-db-encryption` is an opt-in core feature using bundled SQLCipher and
vendored OpenSSL. SQLCipher encrypts SQLite pages with AES-256 and authenticates
them; WAL pages are encrypted too. Temp storage is in memory. The key is installed
before the schema is read, and memory-security wiping is enabled in SQLCipher.
Record encryption remains an additional layer. Password bytes are encoded as a
SQLCipher passphrase, whose salted PBKDF2 derivation is performed by SQLCipher.

The feature is NOT enabled in normal application builds. Existing plaintext
SQLite files return `database_migration_required` without modification. There is
no automatic migration yet. Do not enable this feature for existing profiles
until the migration and platform unlock lifecycle are implemented and tested.

The application BFU state must mean a closed core/database, no retained unwrap
key, no background task able to reopen it, and no plaintext UI caches. AFU means
an unlocked session, not a decrypted file on disk. Closing a view alone is not
BFU. A custom password must wrap the random database secret with a memory-hard
KDF; the system mode must use OS-protected authentication-bound wrapping.
Those lifecycle and UI changes are not implemented by the SQLCipher feature.

Migration must export to a separate encrypted destination, verify integrity and
credentials, sync the output, and handle main/WAL files and crash recovery before
switching the active path. Original files must not be destroyed on a failed
password or export. A successful migration must not leave plaintext backups as an
unadvertised bypass of the new protection. Android ABI builds, Windows builds,
power-loss recovery and device lock tests are required before enabling it.
