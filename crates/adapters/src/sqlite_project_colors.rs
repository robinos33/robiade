//! Couleur de chaque projet, dans `robiade.db`, indexée comme les branches de tronc par la racine
//! de l'arbre principal du dépôt.

use std::path::Path;
use std::sync::Mutex;

use robiade_core::project_colors::ProjectColor;

#[derive(Debug, thiserror::Error)]
#[error("stockage des couleurs de projet : {0}")]
pub struct ColorStorageError(String);

fn storage(e: impl std::fmt::Display) -> ColorStorageError {
    ColorStorageError(e.to_string())
}

pub struct SqliteProjectColors {
    conn: Mutex<rusqlite::Connection>,
}

impl SqliteProjectColors {
    pub fn open(path: &Path) -> Result<Self, ColorStorageError> {
        let conn = rusqlite::Connection::open(path).map_err(storage)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS project_color (
                 repository TEXT PRIMARY KEY,
                 color TEXT NOT NULL
             );",
        )
        .map_err(storage)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Dépôts colorés. Une teinte sortie de la palette vaut « aucune » plutôt qu'une erreur.
    pub fn all(&self) -> Result<Vec<(String, ProjectColor)>, ColorStorageError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare("SELECT repository, color FROM project_color ORDER BY repository").map_err(storage)?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(storage)?;
        let rows = rows.collect::<Result<Vec<_>, _>>().map_err(storage)?;
        Ok(rows.into_iter().filter_map(|(r, c)| Some((r, ProjectColor::parse(&c).ok()?))).collect())
    }

    /// Donne ou retire (`None`) la couleur d'un dépôt.
    pub fn set(&self, repository: &str, color: Option<ProjectColor>) -> Result<(), ColorStorageError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        match color {
            Some(c) => conn.execute(
                "INSERT INTO project_color (repository, color) VALUES (?1, ?2)
                 ON CONFLICT (repository) DO UPDATE SET color = excluded.color",
                [repository, c.as_str()],
            ),
            None => conn.execute("DELETE FROM project_color WHERE repository = ?1", [repository]),
        }
        .map(drop)
        .map_err(storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_repository_keeps_its_own_color_until_it_is_removed() {
        let path = std::env::temp_dir().join(format!("robiade-colors-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let colors = SqliteProjectColors::open(&path).unwrap();
        assert!(colors.all().unwrap().is_empty());
        let (bleu, vert) = (ProjectColor::parse("bleu").unwrap(), ProjectColor::parse("vert").unwrap());
        colors.set("/r/a", Some(bleu)).unwrap();
        colors.set("/r/b", Some(bleu)).unwrap();
        colors.set("/r/a", Some(vert)).unwrap();
        assert_eq!(colors.all().unwrap(), [("/r/a".to_owned(), vert), ("/r/b".to_owned(), bleu)]);
        colors.set("/r/a", None).unwrap();
        assert_eq!(colors.all().unwrap(), [("/r/b".to_owned(), bleu)]);
        drop(colors);
        // Une teinte retirée de la palette ne casse pas la lecture.
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute("INSERT INTO project_color VALUES ('/r/c', 'orange')", []).unwrap();
        assert_eq!(SqliteProjectColors::open(&path).unwrap().all().unwrap(), [("/r/b".to_owned(), bleu)]);
        std::fs::remove_file(path).unwrap();
    }
}
