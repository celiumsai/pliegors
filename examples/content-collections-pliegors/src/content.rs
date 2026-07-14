use pliego_content::{Collection, CollectionSpec, LoadError, MarkdownPolicy};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ritual {
    pub title: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub temperature: String,
    pub duration: String,
    pub benefit: String,
    pub excerpt: String,
    pub sensory: Vec<String>,
    pub best_for: Vec<String>,
    pub safety: Option<String>,
    pub image: String,
    pub image_alt: String,
    pub order: u16,
    pub featured: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct House {
    pub title: String,
    pub setting: String,
    pub descriptor: String,
    pub signature: String,
    pub excerpt: String,
    pub address: String,
    pub hours: Vec<String>,
    pub amenities: Vec<String>,
    pub accessibility: String,
    pub image: String,
    pub image_alt: String,
    pub order: u16,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalEntry {
    pub title: String,
    pub description: String,
    pub publish_date: String,
    pub author: String,
    pub category: String,
    pub image: String,
    pub image_alt: String,
    pub read_time: String,
    pub featured: bool,
}

pub struct Catalog {
    pub rituals: Collection<Ritual>,
    pub houses: Collection<House>,
    pub journal: Collection<JournalEntry>,
}

impl Catalog {
    pub fn load(root: &Path) -> Result<Self, LoadError> {
        let load = |collection: &str| {
            CollectionSpec::new(root.join(collection))
                .options(|options| options.markdown_policy(MarkdownPolicy::Safe))
        };
        Ok(Self {
            rituals: load("rituals").load()?,
            houses: load("houses").load()?,
            journal: load("journal").load()?,
        })
    }

    pub fn total_entries(&self) -> usize {
        self.rituals.len() + self.houses.len() + self.journal.len()
    }
}
