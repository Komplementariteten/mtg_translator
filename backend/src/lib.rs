use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    io::Read,
    sync::Arc,
};

use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    routing::get,
};
pub mod crawlers;
pub mod frontend;
pub mod images;
pub mod limiter;
pub mod logging;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct ImageUris {
    pub normal: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CardFace {
    pub image_uris: Option<ImageUris>,
    pub colors: Option<Vec<String>>,
    pub name: String,
    pub printed_name: Option<String>,
    pub mana_cost: Option<String>,
    pub type_line: Option<String>,
    pub printed_type_line: Option<String>,
    pub oracle_text: Option<String>,
    pub printed_text: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Card {
    #[serde(default)]
    pub keywords: Vec<String>,
    pub image_uris: Option<ImageUris>,
    pub colors: Option<Vec<String>>,
    pub id: String,
    pub oracle_id: Option<String>,
    pub name: String,
    pub lang: String,
    pub printed_name: Option<String>,
    pub mana_cost: Option<String>,
    pub type_line: Option<String>,
    pub printed_type_line: Option<String>,
    pub oracle_text: Option<String>,
    pub printed_text: Option<String>,
    pub set: Option<String>,
    pub rarity: Option<String>,
    pub set_name: Option<String>,
    pub collector_number: Option<String>,
    pub released_at: Option<String>,
    #[serde(default)]
    pub card_faces: Vec<CardFace>,
}

/// One printing (any language) of an Oracle card.
#[derive(Debug, Serialize)]
pub struct Printing {
    pub id: String,
    pub lang: String,
    pub set: Option<String>,
    pub set_name: Option<String>,
    pub collector_number: Option<String>,
    pub released_at: Option<String>,
    pub rarity: Option<String>,
}

impl Printing {
    fn new(card: &Card) -> Self {
        Self {
            id: card.id.clone(),
            lang: card.lang.clone(),
            set: card.set.clone(),
            set_name: card.set_name.clone(),
            collector_number: card.collector_number.clone(),
            released_at: card.released_at.clone(),
            rarity: card.rarity.clone(),
        }
    }
}

#[derive(Default)]
pub struct Database {
    skipped: usize,
    printings: HashMap<String, Vec<Printing>>,
    names: BTreeMap<String, BTreeSet<String>>,
    cards: HashMap<String, HashMap<String, Card>>,
    languages: BTreeSet<String>,
}

/// Summary of the loaded database, used for start-up diagnostics.
#[derive(Debug)]
pub struct Stats {
    pub oracle_cards: usize,
    pub printings: usize,
    pub name_keys: usize,
    pub languages: Vec<String>,
    pub skipped_without_oracle_id: usize,
    pub oracle_cards_without_english: usize,
}

/// Name suggestion with the colour category used to tint it in the UI.
#[derive(Debug, Serialize)]
pub struct Suggestion {
    pub name: String,
    pub mana_cost: Option<String>,
    /// One of W, U, B, R, G, multicolor, colorless or land.
    pub color: &'static str,
}

impl Suggestion {
    fn new(name: &str, card: &Card, face: Option<&CardFace>) -> Self {
        let type_line = face
            .and_then(|f| f.type_line.as_deref())
            .or(card.type_line.as_deref())
            .unwrap_or_default();
        let colors = face
            .and_then(|f| f.colors.as_deref())
            .or(card.colors.as_deref())
            .unwrap_or_default();
        let color = match colors {
            _ if type_line
                .split(" // ")
                .next()
                .is_some_and(|t| t.contains("Land")) =>
            {
                "land"
            }
            [] => "colorless",
            [single] => match single.as_str() {
                "W" => "W",
                "U" => "U",
                "B" => "B",
                "R" => "R",
                "G" => "G",
                _ => "colorless",
            },
            _ => "multicolor",
        };
        let mana_cost = face
            .and_then(|f| f.mana_cost.clone())
            .or_else(|| card.mana_cost.clone())
            .filter(|cost| !cost.is_empty());
        Self {
            name: name.to_string(),
            mana_cost,
            color,
        }
    }
}

const MIN_SUGGEST_CHARS: usize = 3;
const MAX_SUGGESTIONS: usize = 10;

/// Lower-cases and folds umlauts (ä = ae, ö = oe, ü = ue, ß = ss) and other accents,
/// so that "Blitzschlag", "bluetenzauber" and "Blütenzauber" are all matchable.
fn normalize(name: &str) -> String {
    let mut folded = String::with_capacity(name.len());
    for c in name.trim().chars().flat_map(char::to_lowercase) {
        match c {
            'ä' => folded.push_str("ae"),
            'ö' => folded.push_str("oe"),
            'ü' => folded.push_str("ue"),
            'ß' => folded.push_str("ss"),
            'æ' => folded.push_str("ae"),
            'œ' => folded.push_str("oe"),
            'à' | 'á' | 'â' | 'ã' | 'å' | 'ā' => folded.push('a'),
            'ç' => folded.push('c'),
            'è' | 'é' | 'ê' | 'ë' | 'ē' => folded.push('e'),
            'ì' | 'í' | 'î' | 'ï' | 'ī' => folded.push('i'),
            'ñ' => folded.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ø' | 'ō' => folded.push('o'),
            'ù' | 'ú' | 'û' | 'ū' => folded.push('u'),
            'ý' | 'ÿ' => folded.push('y'),
            _ => folded.push(c),
        }
    }
    folded
}

/// 0 when `normalized` starts with `query`, 1 when `query` starts at a later word boundary.
fn match_rank(normalized: &str, query: &str) -> Option<u8> {
    if normalized.starts_with(query) {
        return Some(0);
    }
    normalized
        .match_indices(query)
        .any(|(index, _)| {
            normalized[..index]
                .chars()
                .next_back()
                .is_some_and(|before| !before.is_alphanumeric())
        })
        .then_some(1)
}

impl Database {
    /// Reads Scryfall bulk data in JSON Lines format: one card object per line.
    pub fn from_reader(reader: impl Read) -> Result<Self, serde_json::Error> {
        let mut database = Database::default();
        for card in serde_json::Deserializer::from_reader(reader).into_iter::<Card>() {
            database.insert(card?);
        }
        if database.cards.is_empty() {
            return Err(serde::de::Error::custom(
                "database contains no cards with an oracle_id",
            ));
        }
        Ok(database)
    }

    fn insert(&mut self, card: Card) {
        let Some(oracle_id) = &card.oracle_id else {
            self.skipped += 1;
            return;
        };
        let names = std::iter::once(card.name.as_str())
            .chain(card.printed_name.as_deref())
            .chain(card.card_faces.iter().flat_map(|face| {
                std::iter::once(face.name.as_str()).chain(face.printed_name.as_deref())
            }));
        for name in names {
            self.names
                .entry(normalize(name))
                .or_default()
                .insert(oracle_id.clone());
        }
        self.languages.insert(card.lang.clone());
        self.printings
            .entry(oracle_id.clone())
            .or_default()
            .push(Printing::new(&card));
        let translations = self.cards.entry(oracle_id.clone()).or_default();
        // Keep the oldest printing (undated ones last), with a stable tie-breaker for bulk-file ordering.
        let age = |card: &Card| {
            (
                card.released_at.is_none(),
                card.released_at.clone(),
                card.id.clone(),
            )
        };
        let replace = translations
            .get(&card.lang)
            .is_none_or(|existing| age(&card) < age(existing));
        if replace {
            translations.insert(card.lang.clone(), card);
        }
    }

    /// Normal-size image URLs of the oldest English printing of every Oracle card.
    pub fn image_jobs(&self) -> Vec<images::ImageJob> {
        let mut jobs = Vec::new();
        for (oracle_id, translations) in &self.cards {
            let Some(card) = translations.get("en") else {
                continue;
            };
            let urls: Vec<_> = if card.card_faces.iter().any(|f| f.image_uris.is_some()) {
                card.card_faces
                    .iter()
                    .map(|f| f.image_uris.as_ref())
                    .collect()
            } else {
                vec![card.image_uris.as_ref()]
            };
            for (index, uris) in urls.into_iter().enumerate() {
                if let Some(url) = uris.and_then(|u| u.normal.clone()) {
                    jobs.push(images::ImageJob {
                        oracle_id: oracle_id.clone(),
                        index,
                        url,
                    });
                }
            }
        }
        jobs.sort_by(|a, b| (&a.oracle_id, a.index).cmp(&(&b.oracle_id, b.index)));
        jobs
    }

    /// Names in `lang` in which a word starts with `query` (prefix matches first);
    /// every suggestion is accepted by `translate`.
    fn suggest(&self, query: &str, lang: &str) -> Vec<Suggestion> {
        let query = normalize(query);
        if query.chars().count() < MIN_SUGGEST_CHARS {
            return Vec::new();
        }
        let mut suggestions = BTreeMap::new();
        for (_, oracle_ids) in self
            .names
            .range(query.clone()..)
            .take_while(|(key, _)| key.starts_with(&query))
        {
            self.collect_suggestions(oracle_ids, lang, &query, &mut suggestions);
            if suggestions.len() >= MAX_SUGGESTIONS * 4 {
                break;
            }
        }
        // Word-start matches inside names need a scan over all keys.
        if suggestions.len() < MAX_SUGGESTIONS {
            for (key, oracle_ids) in &self.names {
                if match_rank(key, &query) == Some(1) {
                    self.collect_suggestions(oracle_ids, lang, &query, &mut suggestions);
                }
            }
        }
        suggestions.into_values().take(MAX_SUGGESTIONS).collect()
    }

    fn collect_suggestions(
        &self,
        oracle_ids: &BTreeSet<String>,
        lang: &str,
        query: &str,
        suggestions: &mut BTreeMap<(u8, String), Suggestion>,
    ) {
        for id in oracle_ids {
            let Some(card) = self.cards.get(id).and_then(|c| c.get(lang)) else {
                continue;
            };
            let whole = (
                card.printed_name.as_deref().unwrap_or(&card.name),
                card.card_faces.first(),
            );
            let faces = card.card_faces.iter().map(|face| {
                (
                    face.printed_name.as_deref().unwrap_or(&face.name),
                    Some(face),
                )
            });
            for (name, face) in std::iter::once(whole).chain(faces) {
                if let Some(rank) = match_rank(&normalize(name), query) {
                    suggestions
                        .entry((rank, name.to_string()))
                        .or_insert_with(|| Suggestion::new(name, card, face));
                }
            }
        }
    }

    pub fn stats(&self) -> Stats {
        Stats {
            oracle_cards: self.cards.len(),
            printings: self.printings.values().map(Vec::len).sum(),
            name_keys: self.names.len(),
            languages: self.languages.iter().cloned().collect(),
            skipped_without_oracle_id: self.skipped,
            oracle_cards_without_english: self
                .cards
                .values()
                .filter(|translations| !translations.contains_key("en"))
                .count(),
        }
    }

    /// All printings of an Oracle card, oldest first.
    fn printings(&self, oracle_id: &str) -> Vec<&Printing> {
        let mut printings: Vec<_> = self
            .printings
            .get(oracle_id)
            .into_iter()
            .flatten()
            .collect();
        let key = |p: &Printing| {
            (
                p.released_at.is_none(),
                p.released_at.clone(),
                p.set.clone(),
                p.collector_number.clone(),
                p.lang.clone(),
            )
        };
        printings.sort_by_key(|p| key(p));
        printings
    }

    fn translate(&self, name: &str, lang: &str) -> Result<Vec<&Card>, ApiError> {
        if name.trim().is_empty() || name.len() > 300 {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "Enter a card name between 1 and 300 bytes.",
            ));
        }
        if !self.languages.contains(lang) {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "That language is not available in the local database.",
            ));
        }
        let oracle_ids = self.names.get(&normalize(name)).ok_or_else(|| {
            api_error(
                StatusCode::NOT_FOUND,
                "Card not found in the local database.",
            )
        })?;
        let cards: Vec<_> = oracle_ids
            .iter()
            .filter_map(|id| self.cards.get(id)?.get(lang))
            .collect();
        if cards.is_empty() {
            return Err(api_error(
                StatusCode::NOT_FOUND,
                "This card has no printing in the selected language.",
            ));
        }
        Ok(cards)
    }
}

#[derive(Serialize)]
pub struct ErrorBody {
    pub error: String,
}

type ApiError = (StatusCode, Json<ErrorBody>);

fn api_error(status: StatusCode, message: &str) -> ApiError {
    (
        status,
        Json(ErrorBody {
            error: message.into(),
        }),
    )
}

#[derive(Deserialize)]
pub struct TranslationQuery {
    pub name: String,
    pub lang: String,
}

async fn languages(State(database): State<Arc<Database>>) -> Json<Vec<String>> {
    Json(database.languages.iter().cloned().collect())
}

#[derive(Deserialize)]
pub struct SuggestQuery {
    pub q: String,
    pub lang: String,
}

async fn suggest(
    State(database): State<Arc<Database>>,
    Query(query): Query<SuggestQuery>,
) -> Json<Vec<Suggestion>> {
    Json(database.suggest(&query.q, &query.lang))
}

async fn translate(
    State(database): State<Arc<Database>>,
    Query(query): Query<TranslationQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let cards = database.translate(&query.name, &query.lang)?;
    let serialize = |_| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not serialize cards.",
        )
    };
    let mut values = Vec::with_capacity(cards.len());
    for card in cards {
        let mut value = serde_json::to_value(card).map_err(serialize)?;
        if let (Some(object), Some(oracle_id)) = (value.as_object_mut(), &card.oracle_id) {
            let printings =
                serde_json::to_value(database.printings(oracle_id)).map_err(serialize)?;
            object.insert("printings".into(), printings);
        }
        values.push(value);
    }
    Ok(Json(values.into()))
}

pub fn api(database: Database) -> Router {
    Router::new()
        .route("/api/languages", get(languages))
        .route("/api/suggest", get(suggest))
        .route("/api/translate", get(translate))
        .with_state(Arc::new(database))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Database {
        Database::from_reader(include_bytes!("../tests/cards.jsonl").as_slice()).unwrap()
    }

    #[test]
    fn translates_english_and_localized_names_case_insensitively() {
        let db = database();
        for name in ["  LIGHTNING BOLT ", "Blitzschlag"] {
            let cards = db.translate(name, "de").ok().unwrap();
            assert_eq!(cards.len(), 1);
            assert_eq!(cards[0].printed_name.as_deref(), Some("Blitzschlag"));
            assert_eq!(cards[0].id, "bolt-de-old");
            assert_eq!(cards[0].printed_text.as_deref(), Some("Beispieltext"));
        }
        assert_eq!(
            db.translate("Blitzschlag", "en").ok().unwrap()[0].name,
            "Lightning Bolt"
        );
    }

    #[test]
    fn translates_individual_faces_and_preserves_localized_fields() {
        let db = database();
        let cards = db.translate("Delver of Secrets", "de").ok().unwrap();
        assert_eq!(cards[0].card_faces.len(), 2);
        assert_eq!(
            cards[0].card_faces[0].printed_name.as_deref(),
            Some("Geheimnisstöberer")
        );
        assert_eq!(
            db.translate("Insekten-Scheusal", "en").ok().unwrap()[0].lang,
            "en"
        );
    }

    #[test]
    fn rejects_invalid_queries_and_distinguishes_missing_translations() {
        let db = database();
        for (name, lang, status, message) in [
            ("", "en", StatusCode::BAD_REQUEST, "Enter a card name"),
            ("Lightning Bolt", "xx", StatusCode::BAD_REQUEST, "language"),
            ("Unknown", "en", StatusCode::NOT_FOUND, "Card not found"),
            ("Only English", "de", StatusCode::NOT_FOUND, "no printing"),
        ] {
            let (actual, body) = db.translate(name, lang).err().unwrap();
            assert_eq!(actual, status);
            assert!(body.error.contains(message));
        }
        assert_eq!(
            db.translate(&"a".repeat(301), "en").err().unwrap().0,
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn suggests_names_in_the_requested_language() {
        let names = |q: &str, lang: &str| {
            database()
                .suggest(q, lang)
                .into_iter()
                .map(|s| s.name)
                .collect::<Vec<_>>()
        };
        assert!(names("Bl", "de").is_empty());
        assert_eq!(names("blitz", "de"), ["Blitzschlag"]);
        assert!(names("blitz", "en").is_empty());
        assert_eq!(names("Lightn", "en"), ["Lightning Bolt"]);
        assert_eq!(names("Insek", "de"), ["Insekten-Scheusal"]);
        assert!(names("zzz", "de").is_empty());
    }

    #[test]
    fn folds_umlauts_and_matches_at_word_boundaries() {
        assert_eq!(
            normalize(" Blütenzauber STRAßE Ærie "),
            "bluetenzauber strasse aerie"
        );
        let names = |q: &str, lang: &str| {
            database()
                .suggest(q, lang)
                .into_iter()
                .map(|s| s.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names("geheimnisstoeb", "de"), ["Geheimnisstöberer"]);
        assert_eq!(names("geheimnisstöb", "de"), ["Geheimnisstöberer"]);
        assert_eq!(names("scheus", "de"), ["Insekten-Scheusal"]);
        assert_eq!(names("bolt", "en"), ["Lightning Bolt"]);
        assert!(names("ightning", "en").is_empty());
        assert_eq!(names("lightning b", "en"), ["Lightning Bolt"]);
    }

    #[test]
    fn suggestions_carry_color_category_and_mana_cost() {
        let db = database();
        let bolt = &db.suggest("lightn", "en")[0];
        assert_eq!((bolt.color, bolt.mana_cost.as_deref()), ("R", Some("{R}")));
        assert_eq!(db.suggest("delver", "en")[0].color, "U");
        assert_eq!(db.suggest("Only", "en")[0].color, "colorless");
        assert_eq!(db.suggest("Gold", "en")[0].color, "multicolor");
        assert_eq!(db.suggest("Test Fo", "en")[0].color, "land");
    }

    #[test]
    fn validates_database_and_skips_cards_without_oracle_ids() {
        for invalid in ["", "[]", "{", "{\"id\": \"x\"}"] {
            assert!(Database::from_reader(invalid.as_bytes()).is_err());
        }
        let db = database();
        assert_eq!(
            db.languages.iter().cloned().collect::<Vec<_>>(),
            ["de", "en"]
        );
        assert!(!db.names.contains_key("unlinked token"));
    }

    #[tokio::test]
    async fn handlers_return_languages_and_translations() {
        let db = Arc::new(database());
        assert_eq!(languages(State(db.clone())).await.0, ["de", "en"]);
        let response = translate(
            State(db),
            Query(TranslationQuery {
                name: "Lightning Bolt".into(),
                lang: "de".into(),
            }),
        )
        .await
        .ok()
        .unwrap();
        assert_eq!(response.0[0]["printed_name"], "Blitzschlag");
        let printings = response.0[0]["printings"].as_array().unwrap();
        let ids: Vec<_> = printings
            .iter()
            .map(|p| p["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["bolt-de-old", "bolt-en", "bolt-de-new"]);
    }
}
