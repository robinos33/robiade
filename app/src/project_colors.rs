//! Couleur de chaque projet (Réglages, section Projets) : lue une fois au chargement de l'interface,
//! qui la reprend partout où le projet apparaît, et réenregistrée à chaque choix.

use std::collections::BTreeMap;

use robiade_core::project_colors::{ProjectColor, PALETTE};
use serde::Serialize;
use tauri::Manager;

use crate::AppState;

type CmdResult<T> = Result<T, String>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectColorsDto {
    /// Teintes proposées, dans l'ordre du sélecteur.
    pub palette: &'static [&'static str],
    /// Couleur par racine de dépôt principal ; un dépôt absent n'en a pas.
    pub colors: BTreeMap<String, &'static str>,
}

fn describe(state: &AppState) -> CmdResult<ProjectColorsDto> {
    let colors = state.project_colors.all().map_err(|e| e.to_string())?;
    Ok(ProjectColorsDto { palette: &PALETTE, colors: colors.into_iter().map(|(r, c)| (r, c.as_str())).collect() })
}

#[tauri::command]
pub async fn project_colors(app: tauri::AppHandle) -> CmdResult<ProjectColorsDto> {
    tauri::async_runtime::spawn_blocking(move || describe(&app.state::<AppState>()))
        .await
        .map_err(|e| e.to_string())?
}

/// Donne une teinte de la palette au dépôt, ou la lui retire (`None`).
#[tauri::command]
pub async fn save_project_color(
    app: tauri::AppHandle,
    repository: String,
    color: Option<String>,
) -> CmdResult<ProjectColorsDto> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let repository = crate::commands::main_repository(&state, &repository)?;
        let color = color.as_deref().map(ProjectColor::parse).transpose().map_err(|e| e.to_string())?;
        state.project_colors.set(&repository, color).map_err(|e| e.to_string())?;
        describe(&state)
    })
    .await
    .map_err(|e| e.to_string())?
}
