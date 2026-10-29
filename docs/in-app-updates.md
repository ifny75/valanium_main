# In-app updates

Windows and Android check at startup and at least every six hours while the
application is open. Failed foreground checks retry after a minute. In Tor mode,
both the signed release manifest and the artifact use the existing onion web
entrances through local SOCKS5 with remote name resolution. There is no direct
HTTPS fallback. Auto mode uses the public HTTPS website.

The shared core verifies the pinned Ed25519 signature before using any release
fields. Only exact Valanium download filenames, numeric versions, bounded sizes
and SHA-256 hashes are accepted. Downloads stream to a temporary file in the
application cache and become usable only after size and hash verification.
Redirects and environment-supplied proxies are disabled. The download is checked
again before installation. Account keys, database paths and conversation data
are never sent in update requests.

Downloading is automatic; installation requires an explicit user action.
Windows runs the verified new executable as a helper, waits for the previous
process to exit, replaces its executable and restarts at the same path. The old
binary is retained as `.previous.exe`, with rollback if replacement or restart
fails. Android shares just the selected cached APK through a private, read-only
content provider; the system controls permission and installation confirmation.
Neither installer replaces or clears account storage.

Connection recovery also has bounded opening, HELLO, AUTH and socket-write
deadlines, plus a heartbeat inactivity watchdog. Android notices network changes
that occurred while its activity was stopped. Windows responds to network
online/offline events, and owns its Tor helper with a kill-on-close Windows job
so a crash cannot leave Tor processes behind.
