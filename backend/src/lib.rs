use std::{
    collections::{BTreeSet, HashMap},
    fmt,
    io::Read,
    sync::Arc,
};

use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    routing::get,
};
use serde::{
    Deserialize, Serialize,
    de::{SeqAccess, Visitor},
};

#[derive(Debug, Deserialize, Serialize)]
pub struct CardFace {
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
    pub set_name: Option<String>,
    pub collector_number: Option<String>,
    pub released_at: Option<String>,
    #[serde(default)]
    pub card_faces: Vec<CardFace>,
}

#[derive(Default)]
pub struct Database {
    names: HashMap<String, BTreeSet<String>>,
    cards: HashMap<String, HashMap<String, Card>>,
    languages: BTreeSet<String>,
}

fn normalize(name: &str) -> String {
    name.trim().to_lowercase()
}

impl Database {
    pub fn from_reader(reader: impl Read) -> Result<Self, serde_json::Error> {
        struct DatabaseVisitor;

        impl<'de> Visitor<'de> for DatabaseVisitor {
            type Value = Database;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a Scryfall bulk-data array")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Database, A::Error> {
                let mut database = Database::default();
                while let Some(card) = seq.next_element::<Card>()? {
                    database.insert(card);
                }
                if database.cards.is_empty() {
                    return Err(serde::de::Error::custom(
                        "database contains no cards with an oracle_id",
                    ));
                }
                Ok(database)
            }
        }

        let mut deserializer = serde_json::Deserializer::from_reader(reader);
        let database =
            serde::de::Deserializer::deserialize_seq(&mut deserializer, DatabaseVisitor)?;
        deserializer.end()?;
        Ok(database)
    }

    fn insert(&mut self, card: Card) {
        let Some(oracle_id) = &card.oracle_id else {
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
        let translations = self.cards.entry(oracle_id.clone()).or_default();
        // Keep the newest printing, with a stable tie-breaker for bulk-file ordering.
        let replace = translations.get(&card.lang).is_none_or(|existing| {
            (&card.released_at, &card.id) > (&existing.released_at, &existing.id)
        });
        if replace {
            translations.insert(card.lang.clone(), card);
        }
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

async fn translate(
    State(database): State<Arc<Database>>,
    Query(query): Query<TranslationQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let cards = database.translate(&query.name, &query.lang)?;
    serde_json::to_value(cards).map(Json).map_err(|_| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not serialize cards.",
        )
    })
}

pub fn api(database: Database) -> Router {
    Router::new()
        .route("/api/languages", get(languages))
        .route("/api/translate", get(translate))
        .with_state(Arc::new(database))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Database {
        Database::from_reader(include_bytes!("../tests/cards.json").as_slice()).unwrap()
    }

    #[test]
    fn translates_english_and_localized_names_case_insensitively() {
        let db = database();
        for name in ["  LIGHTNING BOLT ", "Blitzschlag"] {
            let cards = db.translate(name, "de").ok().unwrap();
            assert_eq!(cards.len(), 1);
            assert_eq!(cards[0].printed_name.as_deref(), Some("Blitzschlag"));
            assert_eq!(cards[0].id, "bolt-de-new");
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
    fn validates_database_and_skips_cards_without_oracle_ids() {
        for invalid in ["[]", "{}", "[", "[] trailing"] {
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
    }
}
