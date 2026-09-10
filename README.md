# scorarium

![lint workflow](https://github.com/Notgnoshi/scorarium/actions/workflows/lint.yml/badge.svg?event=push)
![code coverage](https://img.shields.io/endpoint?url=https://gist.githubusercontent.com/Notgnoshi/595d549002d10272f601d5eb4e005627/raw/scorarium-coverage.json)

A physical and digital sheet music library

## Why build a new thing?

While there are several self-hostable library database projects out there better than what I can
build, none that I have found have modeled sheet music with a Library -> Publication -> Work
hierarchy. This is important to me, because I want to be able to answer the question:

> Do I already have the sheet music for Scriabin's Prelude in B minor, Op. 13 No. 6?

The plan is to build out metadata enrichment capabilities against various open APIs for books and
sheet music, so that (in most cases?) you only have to import the publication title.

## Why use Rust?

Because I like Rust :)

## Demo

There's a demo instance running at <https://library.agill.net>. The demo instance uses an ephemeral
in-memory database, which gets wiped with every deployment.

## Developer info

See [developer-guide.md](./docs/developer-guide.md).
