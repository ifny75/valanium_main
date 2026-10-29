# Russian and English interfaces

The website, Windows messenger and Android messenger support Russian and English.
Website language is selected in the header, Windows language in Settings, and
Android language on the entry screen or in Settings. The choice is saved locally.
Android also offers the device language. Unsupported device languages use English.

## Translation boundaries

- Android XML resources have a complete `values-en/strings.xml` counterpart.
  Application-owned Java labels use `I18n.t` and `assets/ui-en.json`.
- Windows static chrome is translated before the core populates the interface.
  Dynamic application labels use `tr` or `trTemplate`; template arguments are
  inserted after translating the template. There is no message-list observer.
- The website translates application-owned text and accessibility labels through
  `LanguageProvider`. Animated headings translate complete lines before splitting
  them into letters. Form values, code blocks and `data-no-translate` content are
  excluded. Server responses do not depend on a visitor's language.

Never pass messages, names, identifiers, QR payloads, recovery words, or imported
account data to a translator. Dictionaries contain interface source strings only.
Unknown Russian core errors receive a generic English UI explanation while keeping
the technical error code; they are never sent to an online translation service.

## Validation on 29 October 2026

- Windows: 27 tests, including composer submission, privacy defaults, swipe bounds,
  translation templates and preservation of interpolated user content.
- Android English resources cover every default resource and retain format slots.
- Core: 257 unit tests. Server: type checking and 194 tests.
- Pixel 10: isolated `app.valanium.qa` tests cover message interactions, calculator,
  scrolling, profiles, delivery labels, English labels and verification QR codes.
  QR checks include encode/decode, rejecting an unrelated URL and decoding the
  actual rendered dialog screenshot.
- Live phone test: two disposable JNI accounts use embedded Tor, exchange encrypted
  messages in both directions, receive server acknowledgements, load a peer profile
  and produce matching pair verification QR payloads and safety numbers. One run
  passed in about 49 seconds including Tor startup; this is not a latency guarantee.

Live tests create temporary server accounts. Run them deliberately, using
`-PisolatedTest` and `app.valanium.qa.test/android.test.InstrumentationTestRunner`.
UI tests refuse to operate on the installed production package.
Registration precedes KeyPackage publication: the live fixture waits for the
initial profile plus a subsequent profile response before attempting first contact.

Release artifacts and the public signed manifest are checked by SHA-256 and the
pinned release verification key. Android updates retain the signing lineage needed
to install over earlier versions without clearing account data.
