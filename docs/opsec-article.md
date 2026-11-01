# OPSEC article

Published at `/articles/opsec`, with links from the homepage. The Russian article is a reading edition of the user-supplied 32-page `OPSEC_Internet_Security_Guide_v2.pdf` (edition 2026-11-01), including chapters 01–32 and the operating systems/email supplement. It preserves the document's claims and bibliography rather than representing a new independent audit of the recommendations.

The original PDF is served unchanged from `public/guides/opsec-internet-security-v2.pdf`.
SHA-256: `4ad419e5a35eea72c1b782a20f47a1ab58b8e782e15f2426c1a32978bfc51c69`.

`app/articles/opsec/content.json` stores semantic headings, paragraphs, and table rows. Page headers, footers, redundant PDF contents pages and cover pages are replaced by the site header and linked web contents. Source page links remain available for every chapter. All 32 source reference URLs are preserved as external links. The supplied Russian article is explicitly marked `lang="ru"` and excluded from the site's text replacement translator; the surrounding site controls have English translations.

Verification: production build, TypeScript, source-to-web token comparison (only the 32 standalone chapter number badges removed), 32 unique section anchors, 18 tables, narrow viewport overflow check, byte-for-byte PDF hash check.
