# Magic card translator

A Rust (Axum) backend and React (Vite) frontend for looking up Magic: The
Gathering cards in other languages using a **local Scryfall JSON database**.
The application makes no calls to Scryfall or translation services at runtime.

## Database

Download and extract the **All Cards** JSON export from
[Scryfall bulk data](https://scryfall.com/docs/api/bulk-data). Unlike Oracle Cards
or Default Cards, All Cards includes language variants. Save the JSON array
locally, for example in `data/all-cards-20261121102255.jsonl.gz` (ignored by Git). The export can
be large: allow time for startup and sufficient memory for the lookup index.
A smaller subset of the same JSON format also works.

Printings are linked by `oracle_id`, with the oldest printing per language
used for display. Cards without an `oracle_id` are skipped. Exact English and
localized names, including individual face names, are supported (case-insensitive,
with surrounding whitespace ignored). The available languages come from the file.
Multi-faced cards display each face separately.

This is a printing lookup, not machine translation. Missing language printings
return an explicit error; missing localized rules text is displayed as labeled
English Oracle text when available. Printed text may differ from current Oracle
rules. Replace the database file and restart the backend to update it.

## Development

Install a current stable Rust toolchain and Node.js 22.12+ (or 24+) with npm.
From the repository root, start the backend:

```sh
CARD_DATABASE="$PWD/data/all-cards-20261121102255.jsonl.gz" cargo run --manifest-path backend/Cargo.toml
```

In another terminal:

```sh
cd frontend
npm ci
npm run dev
```

Open the URL printed by Vite (normally `http://localhost:5173`). Its `/api`
proxy connects to the backend on `127.0.0.1:3001`.
For a quick offline demonstration without downloading bulk data, set
`CARD_DATABASE="$PWD/backend/tests/cards.jsonl"` instead. This small synthetic
fixture includes `Lightning Bolt` / `Blitzschlag` and a double-faced card;
its example rules text is not real card rules.

## Build and run

```sh
npm --prefix frontend ci
npm --prefix frontend run build
cargo build --release --manifest-path backend/Cargo.toml
CARD_DATABASE="$PWD/data/all-cards-20261121102255.jsonl.gz" ./backend/target/release/backend
```

Open `http://127.0.0.1:3001`. Release builds embed `frontend/dist` into the binary, so build the
frontend first; the backend then serves the UI and the API without Node.js. Debug builds read
`frontend/dist` from disk.

Configuration:

| Variable | Default | Purpose |
| --- | --- | --- |
| `CARD_DATABASE` | Required | Path to a local Scryfall JSONL file (`.jsonl`, or gzip-compressed `.jsonl.gz`), or a directory containing one (newest `.jsonl.gz` preferred) |
| `BIND_ADDRESS` | `127.0.0.1:3001` | Backend listen address |
| `IMAGE_CACHE` | `data/images` | Directory for the half-resolution card image cache, served at `/images` |

## Docker

The multi-stage `Dockerfile` builds the frontend and a static backend with the embedded UI into a
`scratch` image (about 14 MB). Mount a directory containing the Scryfall export (`*.jsonl.gz`, the newest file is used) as
`/data`; the image cache is stored in `/data/images`.

```sh
docker build -t mtg-translator .
docker run --rm -p 3001:3001 -v "$PWD/data:/data" mtg-translator
# or: docker compose up --build   (uses the export in ./data)
```

The container listens on `0.0.0.0:3001` (`BIND_ADDRESS`); `CARD_DATABASE` may be a file or a directory.

Crawlers and indexing: requests whose `User-Agent` is empty or matches a known bot, crawler,
scraper, AI/SEO fetcher, link previewer, monitor, scanner or HTTP library (this includes `curl`)
receive an empty HTML page. `/robots.txt` (`Disallow: /`) is answered for everyone; all responses carry
`X-Robots-Tag: noindex, nofollow, …` and `Referrer-Policy: no-referrer`, and the page has `robots` meta tags.

Access limit: within any 2 minutes the server accepts requests from at most 3 distinct client IP
addresses (further IPs get HTTP 429 with `Retry-After`). An IP keeps its slot while active and frees it
after 2 minutes of inactivity. The limit uses the TCP peer address and ignores `X-Forwarded-For`, so
behind a reverse proxy or Docker's userland proxy all clients appear as one IP.

The default binding is local-only. If exposing the server publicly, put it behind
an appropriately secured reverse proxy. No credentials are needed.

## API

- `GET /api/languages`: sorted array of language codes present in the database.
- `GET /api/translate?name=Lightning%20Bolt&lang=de`: array of matching translated
  card objects, including localized fields and `card_faces`. Multiple matches
  are possible when a name identifies more than one Oracle card.
- Invalid input returns HTTP 400; an unknown card or missing language printing
  returns HTTP 404. Lookup errors contain a JSON `error` message.
- Startup fails if the database is missing, malformed, or contains no linked cards.

## Checks

```sh
cargo test --manifest-path backend/Cargo.toml
cargo fmt --manifest-path backend/Cargo.toml --check
cargo clippy --manifest-path backend/Cargo.toml --all-targets -- -D warnings
npm --prefix frontend run lint
npm --prefix frontend run build
```