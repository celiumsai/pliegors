// SPDX-License-Identifier: Apache-2.0

use pliego_fold::{
    CanonicalJsonCodec, Projection, ReactiveLog, Reducer, ReducerError, ReducerIdentity,
};
use pliego_log::{EventCatalogBuilder, EventSchema, SealedEventCatalog};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::io;

pub type DomainResult<T> = Result<T, Box<dyn Error>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    CreateNote { title: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteCreated {
    pub id: u64,
    pub title: String,
}

impl EventSchema for NoteCreated {
    const KIND: &'static str = "app_note_created";
    const VERSION: u32 = 1;
    const SCHEMA_ID: &'static str = "starter.note-created/v1";
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum AppEvent {
    NoteCreated(NoteCreated),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotesProjection {
    pub notes: Vec<Note>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub id: u64,
    pub title: String,
}

pub fn dispatch(log: ReactiveLog, action: Action) -> DomainResult<()> {
    match action {
        Action::CreateNote { title } => {
            let title = title.trim();
            if title.is_empty() {
                return Err(
                    io::Error::new(io::ErrorKind::InvalidInput, "title is required").into(),
                );
            }
            let id = log.len().checked_add(1).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "note sequence overflow")
            })?;
            log.append_typed(&NoteCreated {
                id,
                title: title.to_owned(),
            })?;
            Ok(())
        }
    }
}

pub fn projection(log: ReactiveLog) -> DomainResult<Projection<NotesProjection, AppEvent>> {
    Ok(Projection::new(
        log,
        NotesProjection::default(),
        catalog()?,
        reducer()?,
        CanonicalJsonCodec::default(),
    )?)
}

pub fn first_replayable_state() -> DomainResult<NotesProjection> {
    let log = ReactiveLog::new();
    dispatch(
        log,
        Action::CreateNote {
            title: "First replayable event".to_owned(),
        },
    )?;
    Ok(projection(log)?.try_get()?)
}

fn catalog() -> DomainResult<SealedEventCatalog<AppEvent>> {
    let mut catalog = EventCatalogBuilder::new();
    catalog.register_current::<NoteCreated, _>("starter.note-created/current/1", |event| {
        AppEvent::NoteCreated(event)
    })?;
    Ok(catalog.seal()?)
}

fn reducer() -> DomainResult<Reducer<NotesProjection, AppEvent>> {
    let identity = ReducerIdentity::from_serializable_config(
        "starter.notes",
        1,
        &serde_json::json!({ "ordering": "event-sequence" }),
    )?;
    Ok(Reducer::new(
        identity,
        |state: &mut NotesProjection, event: &AppEvent| {
            match event {
                AppEvent::NoteCreated(note) => state.notes.push(Note {
                    id: note.id,
                    title: note.title.clone(),
                }),
            }
            Ok::<(), ReducerError>(())
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn populated_log() -> DomainResult<ReactiveLog> {
        let log = ReactiveLog::new();
        for title in [
            "Define the event",
            "Fold the projection",
            "Replay the history",
        ] {
            dispatch(
                log,
                Action::CreateNote {
                    title: title.to_owned(),
                },
            )?;
        }
        Ok(log)
    }

    #[test]
    fn live_projection_equals_replay_from_genesis() -> DomainResult<()> {
        let live_log = populated_log()?;
        let live = projection(live_log)?;
        let history = live_log.with(Clone::clone);
        let replay = projection(ReactiveLog::from_log(history))?;
        assert_eq!(live.try_get()?, replay.try_get()?);
        assert_eq!(replay.events_folded(), 3);
        Ok(())
    }

    #[test]
    fn verified_snapshot_restores_then_folds_only_the_tail() -> DomainResult<()> {
        let log = populated_log()?;
        let before_tail = projection(log)?;
        let snapshot = before_tail.snapshot()?;
        dispatch(
            log,
            Action::CreateNote {
                title: "Fold the tail".to_owned(),
            },
        )?;
        let restored = Projection::restore(
            log,
            snapshot,
            catalog()?,
            reducer()?,
            CanonicalJsonCodec::default(),
        )?;
        assert_eq!(restored.try_get()?.notes.len(), 4);
        assert_eq!(restored.events_folded(), 1);
        Ok(())
    }

    #[test]
    fn invalid_action_does_not_append_an_event() {
        let log = ReactiveLog::new();
        let result = dispatch(
            log,
            Action::CreateNote {
                title: "   ".to_owned(),
            },
        );
        assert!(result.is_err());
        assert!(log.is_empty());
    }
}
