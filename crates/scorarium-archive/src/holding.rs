use std::str::FromStr;

use sqlx::SqliteConnection;

use crate::input::{ValidationError, trimmed_or_none};

/// Whether a holding is a thing on a shelf or a file
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldingKind {
    Physical,
    Digital,
}

impl HoldingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            HoldingKind::Physical => "physical",
            HoldingKind::Digital => "digital",
        }
    }
}

impl FromStr for HoldingKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "physical" => Ok(HoldingKind::Physical),
            "digital" => Ok(HoldingKind::Digital),
            _ => Err(format!("unknown holding kind: {s}")),
        }
    }
}

/// One holding as entered in the web form
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoldingRawInput {
    /// The holding this edits; None for a holding being created
    pub id: Option<i64>,
    pub kind: HoldingKind,
    /// Freeform for a physical copy, a filepath in the assets directory for a digital one
    pub location: String,
}

/// One holding's validated fields
#[derive(Debug, PartialEq, Eq)]
pub struct HoldingInput {
    pub(crate) id: Option<i64>,
    pub(crate) kind: HoldingKind,
    pub(crate) location: Option<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct HoldingErrors {
    /// A publication with no copies at all is a publication nobody holds
    pub none: Option<ValidationError>,
    /// One per holding
    pub each: Vec<Option<ValidationError>>,
}

impl HoldingErrors {
    pub fn is_empty(&self) -> bool {
        self.none.is_none() && self.each.iter().all(Option::is_none)
    }
}

/// One holding of a publication, as read back
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Holding {
    pub id: i64,
    pub kind: HoldingKind,
    pub location: Option<String>,
}

/// Check holdings on their own, for the import entry page, which has no publication yet
pub fn parse_holdings(raw: &[HoldingRawInput]) -> Result<Vec<HoldingInput>, HoldingErrors> {
    let mut holdings = Vec::new();
    let errors = HoldingErrors {
        none: raw.is_empty().then_some(ValidationError::NoHoldings),
        each: raw
            .iter()
            .map(|holding| {
                let location = trimmed_or_none(&holding.location);
                if holding.kind == HoldingKind::Digital && location.is_none() {
                    return Some(ValidationError::FileRequired);
                }
                holdings.push(HoldingInput {
                    id: holding.id,
                    kind: holding.kind,
                    location,
                });
                None
            })
            .collect(),
    };
    if errors.is_empty() {
        Ok(holdings)
    } else {
        Err(errors)
    }
}

/// Reconcile a publication's holdings against the ones its input names.
///
/// Unlike identifiers and contributor links, holdings are not rebuilt outright: one whose id the
/// publication already has is updated in place, so it keeps its identity and whatever comes to hang
/// off it. Holdings the input no longer names are deleted. On a publication with none yet the
/// deletes find nothing, so creating and updating share this.
pub(crate) async fn write_holdings(
    conn: &mut SqliteConnection,
    publication_id: i64,
    holdings: &[HoldingInput],
) -> crate::Result<()> {
    let stored = sqlx::query_scalar!(
        "SELECT id FROM holding WHERE publication_id = ?",
        publication_id
    )
    .fetch_all(&mut *conn)
    .await?;
    let named: Vec<i64> = holdings.iter().filter_map(|holding| holding.id).collect();
    for id in stored.iter().filter(|id| !named.contains(id)) {
        sqlx::query!("DELETE FROM holding WHERE id = ?", id)
            .execute(&mut *conn)
            .await?;
    }
    for holding in holdings {
        let kind = holding.kind.as_str();
        // An id naming no holding of this publication cannot be trusted; take it as a new one
        match holding.id.filter(|id| stored.contains(id)) {
            Some(id) => {
                sqlx::query!(
                    "UPDATE holding SET kind = ?, location = ? WHERE id = ?",
                    kind,
                    holding.location,
                    id
                )
                .execute(&mut *conn)
                .await?;
            }
            None => {
                sqlx::query!(
                    "INSERT INTO holding (publication_id, kind, location) VALUES (?, ?, ?)",
                    publication_id,
                    kind,
                    holding.location,
                )
                .execute(&mut *conn)
                .await?;
            }
        }
    }
    Ok(())
}
