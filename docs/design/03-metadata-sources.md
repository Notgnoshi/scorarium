# scorarium metadata sources

This document describes the online metadata sources scorarium intends to build against, based on
research performed 2026-08-30. Every claim about API behavior below was verified with live test
queries at that time unless noted otherwise. Each source covers three questions: what's available,
usage restrictions, and what's missing. Sources that were evaluated and rejected are listed briefly
at the end.

Sources fall into three rough tiers:

1. **Publication-level**: resolve an ISBN/ISMN or title search into publication metadata (title,
   publisher, year, contributors).
2. **Work-level**: canonical metadata about musical works (composer, opus/catalog numbers, key,
   instrumentation, movements).
3. **Contents-level**: enumerate the works contained in a specific published edition. This is the
   scarcest data and the most valuable to scorarium.

## Publication metadata

Resolve an identifier or title search into publication metadata.

### Open Library (primary for ISBN)

https://openlibrary.org/dev/docs/api

Free JSON API, no key. Lookup by ISBN (`GET /isbn/{isbn}.json`, follow redirects) and title/author
search (`search.json`; field-targeted parameters like `title=` outperform the general `q=`). Fields:
title, subtitle, authors, publishers, publish date, identifiers, page count, covers.

Limits: 1 req/s (3 req/s with a User-Agent containing app name and contact email). Licensing is
informal but open (https://openlibrary.org/developers/licensing).

Missing: contributor roles, contents, quality on sheet music records (thin, bookseller-sourced).

### K10plus SRU (primary for ISMN)

https://wiki.k10plus.de/display/K10PLUS/SRU

Free SRU endpoint, no auth, CC0 data
(https://wiki.k10plus.de/spaces/K10PLUS/pages/358711298/Open+Data). Indexes: `pica.ism` (ISMN),
`pica.isb` (ISBN), `pica.tit`, `pica.all`. MARCXML/PICA/MODS responses. German legal deposit gives
excellent Henle/Schott/Baerenreiter coverage with editor-level detail (MARC 700 entries with relator
codes and GND IDs, publisher numbers in MARC 028).

Limits: none documented. XML only; needs MARCXML parsing.

Missing: contents notes; UK/US printings (Boosey & Hawkes ISMNs mostly missing).

### DNB SRU (ISMN/ISBN fallback)

https://www.dnb.de/EN/Professionell/Metadatendienste/Datenbezug/SRU/sru_node.html

Free, no registration, CC0. `dnb.num` resolves both ISBNs and ISMNs; title/person/publisher indexes;
separate music-archive endpoint (`dnb.dma`). Max 100 records/request. Some records link scanned
table-of-contents PDFs.

Limits: XML only; `recordSchema=oai_dc` silently returns zero records (use MARC21-xml); identifiers
must be normalized to bare digits first.

Missing: structured contents; coverage differs from K10plus per printing.

### Library of Congress SRU (ISBN fallback)

https://www.loc.gov/standards/sru/resources/lcServers.html

Free SRU endpoint (`http://lx2.loc.gov:210/LCDB`), no auth, effectively public domain records. ISBN
lookup via `bath.isbn`; title/author search. MODS responses distinguish contributor roles (author vs
editor), the best role data of the English-language sources.

Limits: plain HTTP on port 210; database name must be uppercase `LCDB`.

Missing: ISMN lookup entirely; JSON.

## Works

Canonical metadata about musical works: composer, opus/catalog numbers, key, instrumentation,
movements. Used to turn imported contents strings into first-class work records, and to enrich
manually entered works.

### MusicBrainz (primary)

https://musicbrainz.org/doc/MusicBrainz_API

Free JSON/XML API, no auth for reads, core data CC0
(https://musicbrainz.org/doc/About/Data_License). Work entities carry title, type, key, aliases;
catalog numbers (BWV, K., Op.) are "part of series" relationships with ordering; movements are
ordered "parts" relationships; URL relationships link to IMSLP, Wikidata, VIAF, GND.

Limits: hard ~1 req/s per IP; meaningful User-Agent mandatory; supplementary data (annotations,
tags) is CC BY-NC-SA, avoid storing it.

Missing: instrumentation (no field); printed publications (audio-only release model); search results
dominated by movement-level works.

### Wikidata (enrichment and crosswalk hub)

https://www.mediawiki.org/wiki/Wikidata_Query_Service/User_Manual

SPARQL service plus per-entity JSON, CC0. Musical works carry composer (P86), opus (P10855),
tonality (P826), instrumentation (P870, which MusicBrainz lacks), catalog code (P528), and
crosswalks to MusicBrainz (P435), IMSLP (P839), GND, VIAF, LoC.

Limits: 60s query deadline, 60s processing per minute per client, 5 parallel queries per IP,
User-Agent required.

Missing: records for specific published editions; consistency (field population varies wildly
between famous and obscure works).

### IMSLP (deep enrichment)

https://imslp.org/api.php

MediaWiki API; work pages contain a structured wikitext info block: opus/catalog number, key,
per-movement names with keys and bar counts, dedication, composition year, instrumentation, and a
Wikidata crosswalk. The richest single-record work metadata found.

Limits: CC BY-SA 4.0 (attribute if republishing descriptive text); wikitext requires template/regex
parsing.

Missing: modern copyrighted editions (absent by design); structured responses.

## Publication contents

Enumerate the works contained in a specific published edition. The scarcest data and the most
valuable to scorarium; no purpose-built API exists anywhere.

### Harvard LibraryCloud (primary)

https://library.harvard.edu/services-tools/harvard-library-apis-datasets

Free JSON/XML API, no auth, CC0. MODS records; many score records carry a `tableOfContents` field
(MARC 505 contents note) that enumerates the contained works, delimiter-structured (" -- " and " ;
") and parseable with modest effort.

Limits: none of note.

Missing: reliability. Contents notes are free-text conventions, present on many but not all score
records; the right record must be picked among editions; anthology titles often match audio
recordings instead. Best-effort by nature.

### Publisher product pages (opt-in helper)

Example: https://www.henle.de/en/Piano-Sonatas-Volume-II/HN-34

Henle pages list every contained work in stable per-work HTML divs with difficulty grades, keyed by
publisher number. Baerenreiter and retailer pages carry similar HTML contents lists.

Limits: no API

Missing: stability guarantees, machine-readable structure, coverage beyond each publisher's catalog.

### Also worth knowing

* DNB records sometimes link scanned table-of-contents PDFs (image, not text) and offer a `dnb.inh`
  contents search index.
* IMSLP anthology collection pages enumerate member works, but only for public-domain publications.
* Open Library resolved one multi-work volume's contents through its title alone (the Alfred
  Rachmaninoff "Variations on a Theme of Chopin, Op. 22 ; On a Theme of Corelli, Op. 42").

## Considered and rejected

* **Google Books**: requires an API key; terms of service grant no right to store and republish
  metadata on a public site; no contributor roles, contents, or ISMN support.
* **ISBNdb**: paid only, proprietary licensing, no ISMN support.
* **WorldCat / OCLC**: the free Search API was sunset 2024-12-31; current APIs require institutional
  subscriptions unobtainable by individuals. The largest catalog in the world, but no access path.
* **RISM**: open API and it does model anthology contents, but coverage is historical sources
  (pre-1850 manuscripts and early prints), not modern editions.
* **Open Opus**: free and CC0 but too shallow (~200 composers, no structured opus/key fields)
