# Magic card translator

A Rust (Axum) backend and React (Vite) frontend for looking up Magic: The
Gathering cards in other languages using a **local Scryfall JSON database**.
The application makes no calls to Scryfall or translation services at runtime.

## Database

Download and extract the **All Cards** JSON export from
[Scryfall bulk data](https://scryfall.com/docs/api/bulk-data). Unlike Oracle Cards
or Default Cards, All Cards includes language variants. Save the JSON array
locally, for example in `data/all-cards.json` (ignored by Git). The export can
be large: allow time for startup and sufficient memory for the lookup index.
A smaller subset of the same JSON format also works.

Printings are linked by `oracle_id`, with the newest printing per language
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
CARD_DATABASE="$PWD/data/all-cards.json" cargo run --manifest-path backend/Cargo.toml
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
`CARD_DATABASE="$PWD/backend/tests/cards.json"` instead. This small synthetic
fixture includes `Lightning Bolt` / `Blitzschlag` and a double-faced card;
its example rules text is not real card rules.

## Build and run

```sh
npm --prefix frontend ci
npm --prefix frontend run build
cargo build --release --manifest-path backend/Cargo.toml
CARD_DATABASE="$PWD/data/all-cards.json" ./backend/target/release/backend
```

Open `http://127.0.0.1:3001`. The backend serves `frontend/dist` as well as the API.

Configuration:

| Variable | Default | Purpose |
| --- | --- | --- |
| `CARD_DATABASE` | Required | Path to a local Scryfall JSON array |
| `BIND_ADDRESS` | `127.0.0.1:3001` | Backend listen address |
| `FRONTEND_DIST` | `frontend/dist` relative to the backend source directory | Static frontend directory |

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