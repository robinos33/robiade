//! Commandes des routines : lecture, actions confirmées, création et modification avec aperçu du
//! plist. Toute action relit la routine juste avant et s'inscrit au journal d'audit.

use std::collections::BTreeSet;
use std::sync::Arc;

use robiade_adapters::local_clock;
use robiade_core::merge_actions::AuditEntry;
use robiade_core::routine_schedule::{Weekday, WeeklySchedule};
use robiade_core::routines::{
    default_logs, Label, LastExit, Routine, RoutineAction, RoutineName, RoutinePrefix, RoutineSpec, Routines,
    ServiceState,
};
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

type CmdResult<T> = Result<T, String>;
use crate::{now, AppState};

/// Fin de journal lue par flux : de quoi voir le dernier passage sans charger un journal de plusieurs Mo.
const LOG_TAIL_BYTES: u64 = 32 * 1024;

pub struct RoutineContext {
    pub launchd: Arc<dyn Routines>,
    pub home: String,
    /// `PATH` du shell interactif, copié dans une routine neuve.
    pub path: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastExitDto {
    /// `never`, `code` ou `signal`.
    pub kind: &'static str,
    pub code: Option<i32>,
    pub signal: Option<String>,
}

/// Ce qu'il faut pour pré-remplir le formulaire d'une routine modifiable.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RoutineFormDto {
    pub name: String,
    pub program: String,
    pub arguments: Vec<String>,
    pub working_dir: Option<String>,
    /// Jours ISO : 1 = lundi … 7 = dimanche.
    pub days: Vec<u8>,
    /// `9:00`, `17:30`.
    pub times: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineDto {
    pub label: String,
    pub name: String,
    pub file: String,
    /// `running`, `idle`, `disabled` ou `unloaded`.
    pub state: &'static str,
    pub pid: Option<u32>,
    pub runs: Option<u32>,
    pub last_exit: Option<LastExitDto>,
    pub failed: bool,
    /// Dernière écriture d'un journal : l'heure approximative du dernier passage.
    pub logs_written_at: Option<i64>,
    pub schedule: Option<String>,
    /// Prochain créneau, si la routine est chargée.
    pub next_run: Option<i64>,
    pub command: Vec<String>,
    pub working_dir: Option<String>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    /// Plist illisible, ou label qui n'est pas son nom de fichier.
    pub problem: Option<String>,
    /// Pourquoi Robiade ne la modifie pas ; `None` si elle est modifiable.
    pub read_only: Option<String>,
    pub form: Option<RoutineFormDto>,
}

fn state_name(state: ServiceState) -> &'static str {
    match state {
        ServiceState::Running { .. } => "running",
        ServiceState::Idle => "idle",
        ServiceState::Disabled => "disabled",
        ServiceState::Unloaded => "unloaded",
    }
}

fn last_exit(e: &LastExit) -> LastExitDto {
    match e {
        LastExit::Never => LastExitDto { kind: "never", code: None, signal: None },
        LastExit::Code(c) => LastExitDto { kind: "code", code: Some(*c), signal: None },
        LastExit::Signal(s) => LastExitDto { kind: "signal", code: None, signal: Some(s.clone()) },
    }
}

fn form_of(name: &str, spec: &RoutineSpec) -> RoutineFormDto {
    let schedule = spec.schedule();
    RoutineFormDto {
        name: name.to_owned(),
        program: spec.program()[0].clone(),
        arguments: spec.program()[1..].to_vec(),
        working_dir: spec.working_dir().map(str::to_owned),
        days: schedule.days().iter().map(|d| d.iso()).collect(),
        times: schedule.times().iter().map(|(h, m)| format!("{h}:{m:02}")).collect(),
    }
}

fn dto(r: Routine, prefix: &RoutinePrefix, at: i64) -> RoutineDto {
    let name = r.label.name(prefix).to_owned();
    let loaded = matches!(r.status.state, ServiceState::Idle | ServiceState::Running { .. });
    let (problem, definition) = match &r.definition {
        Ok(d) => (None, Some(d)),
        Err(e) => (Some(e.clone()), None),
    };
    let editable = definition.map(|d| d.editable());
    let next_run = definition
        .filter(|_| loaded)
        .and_then(|d| d.schedule.next_after(local_clock::local_time(at)))
        .and_then(local_clock::millis_of);
    RoutineDto {
        label: r.label.as_str().to_owned(),
        file: r.file.clone(),
        state: state_name(r.status.state),
        pid: match r.status.state {
            ServiceState::Running { pid } => Some(pid),
            _ => None,
        },
        runs: r.status.runs,
        last_exit: r.status.last_exit.as_ref().map(last_exit),
        failed: r.status.failed(),
        logs_written_at: r.logs_written_at,
        schedule: definition.map(|d| d.schedule.describe()),
        next_run,
        command: definition.map(|d| d.command()).unwrap_or_default(),
        working_dir: definition.and_then(|d| d.working_dir.clone()),
        stdout: definition.and_then(|d| d.stdout.clone()),
        stderr: definition.and_then(|d| d.stderr.clone()),
        problem,
        read_only: match &editable {
            Some(Err(reason)) => Some(reason.clone()),
            Some(Ok(_)) => None,
            None => Some("plist illisible".into()),
        },
        form: editable.and_then(Result::ok).map(|spec| form_of(&name, &spec)),
        name,
    }
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn prefix(state: &AppState) -> CmdResult<RoutinePrefix> {
    state.settings.routine_prefix().map_err(err)
}

/// Le label, et seulement s'il est du préfixe réglé : l'interface ne peut pas viser un autre service.
fn label_of(state: &AppState, raw: &str) -> CmdResult<Label> {
    Label::within(raw, &prefix(state)?).ok_or_else(|| "routine hors du préfixe réglé".into())
}

fn record(state: &AppState, label: &Label, action: &str, outcome: &Result<(), String>) {
    let result = outcome.clone().err().unwrap_or_else(|| "ok".into());
    let entry = AuditEntry { at: now(), target: format!("routine {}", label.as_str()), action: action.into(), outcome: result };
    if let Err(e) = state.audit.record(&entry) {
        eprintln!("{e}");
    }
}

async fn blocking<T: Send + 'static>(
    app: tauri::AppHandle,
    run: impl FnOnce(&AppState) -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(move || run(&app.state::<AppState>())).await.map_err(err)?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutinesDto {
    pub prefix: String,
    pub routines: Vec<RoutineDto>,
}

/// Les routines du préfixe réglé, relues sur launchd.
#[tauri::command]
pub async fn routines(app: tauri::AppHandle) -> CmdResult<RoutinesDto> {
    blocking(app, |state| {
        let prefix = prefix(state)?;
        let found = state.routines.launchd.list(&prefix).map_err(err)?;
        let at = now();
        Ok(RoutinesDto { prefix: prefix.as_str().to_owned(), routines: found.into_iter().map(|r| dto(r, &prefix, at)).collect() })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineLogsDto {
    pub stdout: Option<String>,
    pub stderr: Option<String>,
}

#[tauri::command]
pub async fn routine_logs(app: tauri::AppHandle, label: String) -> CmdResult<RoutineLogsDto> {
    blocking(app, move |state| {
        let label = label_of(state, &label)?;
        let routine = state.routines.launchd.find(&label).map_err(err)?;
        let definition = routine.definition.map_err(|e| format!("plist illisible : {e}"))?;
        let logs = state.routines.launchd.logs(&definition, LOG_TAIL_BYTES);
        Ok(RoutineLogsDto { stdout: logs.stdout, stderr: logs.stderr })
    })
    .await
}

fn action_of(raw: &str) -> CmdResult<RoutineAction> {
    RoutineAction::parse(raw).filter(|a| *a != RoutineAction::Edit).ok_or_else(|| "action inconnue".into())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineActionPreviewDto {
    pub routine: RoutineDto,
    pub action: &'static str,
    pub blocker: Option<String>,
}

/// Ce que l'action ferait, sur la routine relue. N'écrit rien.
#[tauri::command]
pub async fn prepare_routine_action(
    app: tauri::AppHandle,
    label: String,
    action: String,
) -> CmdResult<RoutineActionPreviewDto> {
    blocking(app, move |state| {
        let (label, action) = (label_of(state, &label)?, action_of(&action)?);
        let routine = state.routines.launchd.find(&label).map_err(err)?;
        let blocker = action.refused_on(&routine);
        Ok(RoutineActionPreviewDto { routine: dto(routine, &prefix(state)?, now()), action: action.as_str(), blocker })
    })
    .await
}

/// Action confirmée : l'état est revérifié, puis l'action part et s'inscrit au journal d'audit.
#[tauri::command]
pub async fn run_routine_action(app: tauri::AppHandle, label: String, action: String) -> CmdResult<()> {
    blocking(app, move |state| {
        let (label, action) = (label_of(state, &label)?, action_of(&action)?);
        let routine = state.routines.launchd.find(&label).map_err(err)?;
        if let Some(refused) = action.refused_on(&routine) {
            return Err(refused);
        }
        let launchd = &state.routines.launchd;
        let outcome = match action {
            RoutineAction::Run => launchd.kickstart(&label),
            RoutineAction::Disable => launchd.disable(&label),
            RoutineAction::Enable => launchd.enable(&label),
            RoutineAction::Delete => launchd.delete(&label),
            RoutineAction::Edit => unreachable!("refusée par action_of"),
        }
        .map_err(err);
        record(state, &label, action.as_str(), &outcome);
        outcome
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineInputDto {
    #[serde(flatten)]
    pub form: RoutineFormDto,
    /// Label de la routine modifiée ; `None` pour en créer une.
    pub editing: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineSavePreviewDto {
    pub label: String,
    pub file: String,
    pub plist: String,
    pub schedule: String,
    pub next_run: Option<i64>,
    /// Raison de refuser l'enregistrement ; `None` s'il peut partir.
    pub blocker: Option<String>,
}

fn weekly(form: &RoutineFormDto) -> CmdResult<WeeklySchedule> {
    let days: BTreeSet<Weekday> =
        form.days.iter().map(|&d| Weekday::from_iso(d).ok_or("jour inconnu")).collect::<Result<_, _>>()?;
    let times = form
        .times
        .iter()
        .filter(|t| !t.trim().is_empty())
        .map(|t| WeeklySchedule::parse_time(t))
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(err)?;
    WeeklySchedule::new(days, times).map_err(err)
}

/// La routine à écrire, et ce qui l'empêcherait de partir. Une routine neuve journalise sous
/// `~/Library/Logs/Robiade/routines` et reçoit le `PATH` du shell ; une routine modifiée garde
/// ses journaux et son `PATH`.
fn planned(state: &AppState, input: &RoutineInputDto) -> CmdResult<(Label, RoutineSpec, Option<String>)> {
    let form = &input.form;
    let program = std::iter::once(form.program.trim().to_owned()).chain(form.arguments.iter().cloned()).collect();
    let schedule = weekly(form)?;
    let launchd = &state.routines.launchd;
    let (label, spec, refusal) = match &input.editing {
        None => {
            let name = RoutineName::parse(&form.name).map_err(err)?;
            let label = Label::of(&prefix(state)?, &name);
            let (out, error) = default_logs(&state.routines.home, &name);
            let spec = RoutineSpec::new(program, form.working_dir.clone(), schedule, Some(out), Some(error), state.routines.path.clone())
                .map_err(err)?;
            let exists = launchd.find(&label).is_ok().then(|| "une routine porte déjà ce nom".to_owned());
            (label, spec, exists)
        }
        Some(raw) => {
            let label = label_of(state, raw)?;
            let routine = launchd.find(&label).map_err(err)?;
            let refusal = RoutineAction::Edit.refused_on(&routine);
            let kept = routine.definition.as_ref().ok().and_then(|d| d.editable().ok());
            let (out, error, path) = kept.map_or((None, None, None), |k| {
                (k.stdout().map(str::to_owned), k.stderr().map(str::to_owned), k.path_env().map(str::to_owned))
            });
            let spec = RoutineSpec::new(program, form.working_dir.clone(), schedule, out, error, path).map_err(err)?;
            (label, spec, refusal)
        }
    };
    let blocker = refusal.or_else(|| launchd.check(&spec));
    Ok((label, spec, blocker))
}

/// Le plist exact qui serait écrit, et la raison d'un refus. N'écrit rien.
#[tauri::command]
pub async fn prepare_routine_save(app: tauri::AppHandle, input: RoutineInputDto) -> CmdResult<RoutineSavePreviewDto> {
    blocking(app, move |state| {
        let (label, spec, blocker) = planned(state, &input)?;
        let schedule = spec.schedule().schedule();
        Ok(RoutineSavePreviewDto {
            file: format!("{}/Library/LaunchAgents/{}", state.routines.home, label.file_name()),
            plist: state.routines.launchd.render(&label, &spec),
            schedule: schedule.describe(),
            next_run: schedule.next_after(local_clock::local_time(now())).and_then(local_clock::millis_of),
            label: label.as_str().to_owned(),
            blocker,
        })
    })
    .await
}

/// Enregistrement confirmé : tout est revérifié, puis le plist est écrit et chargé.
#[tauri::command]
pub async fn save_routine(app: tauri::AppHandle, input: RoutineInputDto) -> CmdResult<String> {
    blocking(app, move |state| {
        let (label, spec, blocker) = planned(state, &input)?;
        if let Some(refused) = blocker {
            return Err(refused);
        }
        let launchd = &state.routines.launchd;
        let (action, outcome) = match input.editing {
            None => ("create", launchd.create(&label, &spec)),
            Some(_) => ("edit", launchd.replace(&label, &spec)),
        };
        let outcome = outcome.map_err(err);
        record(state, &label, action, &outcome);
        outcome.map(|()| label.as_str().to_owned())
    })
    .await
}

#[tauri::command]
pub fn save_routine_prefix(state: State<AppState>, prefix: String) -> CmdResult<String> {
    let prefix = RoutinePrefix::parse(&prefix).map_err(err)?;
    state.settings.save_routine_prefix(&prefix).map_err(err)?;
    Ok(prefix.as_str().to_owned())
}

/// Montre dans le Finder le plist de la routine, ou le script qu'elle lance : le premier argument
/// qui est un fichier, sinon le programme. Rien n'est ouvert ni exécuté.
#[tauri::command]
pub async fn reveal_routine(app: tauri::AppHandle, label: String, script: bool) -> CmdResult<()> {
    blocking(app, move |state| {
        let label = label_of(state, &label)?;
        let routine = state.routines.launchd.find(&label).map_err(err)?;
        let target = if script {
            let command = routine.definition.map_err(|e| format!("plist illisible : {e}"))?.command();
            let is_file = |p: &&String| p.starts_with('/') && std::path::Path::new(p.as_str()).is_file();
            let found = command.iter().skip(1).find(is_file).or_else(|| command.first().filter(is_file));
            found.cloned().ok_or("aucun fichier à montrer")?
        } else {
            routine.file
        };
        let status = std::process::Command::new("/usr/bin/open").args(["-R", "--", &target]).status().map_err(err)?;
        status.success().then_some(()).ok_or_else(|| "le Finder n'a pas pu montrer ce fichier".into())
    })
    .await
}
