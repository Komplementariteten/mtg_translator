# Copilot instructions

Rust (Axum) backend + React (Vite) frontend that looks up Magic: The Gathering card printings in other languages from a **local Scryfall "All Cards" JSON file**. No runtime calls to Scryfall or translation services; this is printing lookup, not machine translation.

## Commands

Backend (from repo root):
- Run: `CARD_DATABASE="$PWD/backend/tests/cards.jsonl" cargo run --manifest-path backend/Cargo.toml` (small synthetic fixture; real data goes in git-ignored `data/all-cards.json`)
- Test all: `cargo test --manifest-path backend/Cargo.toml`
- Single test: `cargo test --manifest-path backend/Cargo.toml translates_individual_faces`

Frontend (`frontend/`, Node 22.12+/24+):
- `npm ci`, `npm run dev` (Vite proxies `/api` to `127.0.0.1:3001`), `npm run build`, `npm run lint` (oxlint)
- There is no frontend test suite.

Production: build the frontend, then the release backend; the backend serves `frontend/dist` itself (`FRONTEND_DIST` overrides). Env vars: `CARD_DATABASE` (required), `BIND_ADDRESS` (default `127.0.0.1:3001`).

## Architecture

- `backend/src/lib.rs` holds everything except startup: the `Database` (streamed from the JSON array via a serde `visit_seq`, so the huge file is never fully materialised), the lookup index, the handlers and the unit tests. `main.rs` only reads env vars, loads the database and serves `api()` plus the static frontend.
- Indexing: printings are joined by `oracle_id` (cards without one are skipped). English and localized names, including individual card-face names, are indexed under a normalized key (case-insensitive, trimmed). Per language, only the newest printing is kept (ID tie-breaker).
- API: `GET /api/languages` and `GET /api/translate?name=&lang=`. Errors are JSON `{ "error": ... }`: 400 for invalid input, 404 for an unknown card or a missing language printing. A name can match several Oracle cards, so translate returns an array.
- Missing localized rules text falls back to English Oracle text, which the frontend labels as such. Multi-faced cards are displayed face by face (`card_faces`).
- Frontend is a single-component app (`frontend/src/App.jsx`) with plain CSS.
- Unit tests in `lib.rs` load `backend/tests/cards.jsonl` via `include_bytes!`; it is a synthetic fixture (e.g. `Lightning Bolt` / `Blitzschlag`, a double-faced card) whose rules text is not real.
