//! Bounded edit history for an open scan session. Image revisions stay on disk.
//! Undo/redo never rescans, rewrites originals, or deletes retained image files.
use crate::{Result, ScanError, session::ScanSession};

const LIMIT: usize = 32;

struct Change {
    label: String,
    before: ScanSession,
    after: ScanSession,
}

#[derive(Default)]
pub struct ScanHistory {
    undo: Vec<Change>,
    redo: Vec<Change>,
}

impl ScanHistory {
    /// Record committed changes, including completed pages from a partial batch.
    /// Opening a different session starts a separate history.
    pub fn record(&mut self, before: &ScanSession, after: &ScanSession, label: &str) {
        if before.directory != after.directory {
            self.clear();
            return;
        }
        if before.pages == after.pages {
            return;
        }
        self.redo.clear();
        self.undo.push(Change {
            label: label.into(),
            before: before.clone(),
            after: after.clone(),
        });
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
    }
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|c| c.label.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|c| c.label.as_str())
    }
    pub fn undo(&mut self, session: &mut ScanSession) -> Result<()> {
        let change = self
            .undo
            .last()
            .ok_or_else(|| ScanError::InvalidRequest("Nothing to undo".into()))?;
        if session.directory != change.after.directory || session.pages != change.after.pages {
            return Err(ScanError::InvalidRequest(
                "Session changed outside this edit history".into(),
            ));
        }
        session.restore_pages(&change.before.pages)?;
        self.redo.push(self.undo.pop().unwrap());
        Ok(())
    }
    pub fn redo(&mut self, session: &mut ScanSession) -> Result<()> {
        let change = self
            .redo
            .last()
            .ok_or_else(|| ScanError::InvalidRequest("Nothing to redo".into()))?;
        if session.directory != change.before.directory || session.pages != change.before.pages {
            return Err(ScanError::InvalidRequest(
                "Session changed outside this edit history".into(),
            ));
        }
        session.restore_pages(&change.after.pages)?;
        self.undo.push(self.redo.pop().unwrap());
        Ok(())
    }
}
