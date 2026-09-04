# Developer Guide

This project is not really intended to be used as a library, so the MSRV is the latest stable
toolchain, and we aspire to keep all dependencies up-to-date.

It's a "usual" Rust project, so you can use `cargo` as normal:

```sh
cargo build
cargo run -- ...
# Serve an in-memory demo database for demo and testing purposes
cargo run -- --demo
# nextest is preferred over 'cargo test' for better speed; both will work
cargo nextest run
cargo fmt -- --config group_imports=StdExternalCrate,imports_granularity=Module
cargo clippy --all-targets --all-features
# pip install djlint
djlint --reformat --profile django --indent 4 --max-line-length 120 --format-css --format-js crates/scorarium/templates

# For updating or auditing dependencies
cargo install cargo-edit
cargo upgrade --dry-run --incompatible
```

## Database

Database queries use sqlx's compile-time checked macros. Normal builds need no database: the macros
read the query metadata checked in under `.sqlx/`. When you add or change a query or a migration,
regenerate that metadata against a migrated dev database and check in the result:

```sh
cargo install sqlx-cli --no-default-features --features sqlite
export DATABASE_URL=sqlite://target/dev.db
cargo sqlx database setup --source crates/scorarium/migrations
cargo sqlx prepare --workspace -- --all-targets --all-features
```

While `DATABASE_URL` is exported, the macros check queries against that live database instead of
`.sqlx/`, so schema mistakes surface immediately during development.

## Identifiers

ISBN validation and hyphenation need the ISBN agency's range table, which the `isbn` crate embeds at
build time. The crate publishes a new date-suffixed patch version as the table changes, so
`cargo update` needs to be done periodically to keep the table up-to-date.

## Docker build

The CI pipeline builds a static musl binary and packages it into a Docker image. This is how you can
build that image yourself:

```sh
# Fedora
sudo dnf install musl-gcc
# Ubuntu
sudo apt install musl-tools
```

```sh
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
docker build --tag scorarium:dev .
docker run --user "$(id -u):$(id -g)" --volume ./data:/data --publish 3000:3000 scorarium:dev
```

## Maintenance notes

This project values tests, but does not prioritize complete test coverage. Tests are code that needs
to be reviewed and maintained just as well as any other, so this project prioritizes test quality
over quantity. When adding a new test, consider if that test is actually valuable and justifies its
maintenance cost. The same goes for comments and documentation: don't just say what the code does,
but why it does it; the audience of comments and documentation is not the developer at the time of
writing, but the maintainer in several years.

To make a release, add an entry to the `CHANGELOG.md` file and then merge a PR that bumps the
version in the workspace `Cargo.toml`. The CI pipeline will do the rest. Not every change needs
mentioned in the changelog. Keep the focus on changes that are relevant to the end user.
