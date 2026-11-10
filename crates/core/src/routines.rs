//! Routines : des LaunchAgents de l'utilisateur, ceux dont le label commence par le préfixe réglé.
//! launchd les planifie et les lance ; Robiade les lit, les lance à la demande, les active ou les
//! désactive, et écrit les siennes. Il ne touche à aucun autre service, ni à `LaunchDaemons`.
//!
//! Un plist d'un autre outil est une donnée non fiable : il se lit, il ne se réécrit que s'il
//! n'utilise que les clés que Robiade sait écrire.

use crate::routine_schedule::{Schedule, WeeklySchedule};
use crate::Millis;

/// Préfixe d'un profil neuf : celui de l'identifiant de l'application.
pub const DEFAULT_PREFIX: &str = "dev.robin.robiade.routine.";

const PREFIX_MAX: usize = 100;
const NAME_MAX: usize = 48;
const LABEL_MAX: usize = 160;
const MAX_ARGS: usize = 64;
const ARG_MAX_CHARS: usize = 4_096;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RoutineError {
    #[error("préfixe invalide : lettres, chiffres, points et tirets, au moins deux segments, terminé par un point (com.exemple.)")]
    InvalidPrefix,
    #[error("le préfixe ne peut pas viser les services d'Apple")]
    ApplePrefix,
    #[error("nom invalide : minuscules, chiffres et tirets, {NAME_MAX} caractères au plus")]
    InvalidName,
    #[error("label invalide")]
    InvalidLabel,
    #[error("programme absent")]
    NoProgram,
    #[error("le programme doit être un chemin absolu : {0}")]
    RelativeProgram(String),
    #[error("{MAX_ARGS} arguments au plus")]
    TooManyArguments,
    #[error("argument trop long : {ARG_MAX_CHARS} caractères au plus")]
    ArgumentTooLong,
    #[error("caractère de contrôle refusé : {0}")]
    Control(&'static str),
    #[error("{0} doit être un chemin absolu")]
    RelativePath(&'static str),
    #[error("routine introuvable")]
    NotFound,
    #[error("une routine porte déjà ce nom")]
    Exists,
    #[error("launchd a refusé : {0}")]
    Launchd(String),
    #[error("{0}")]
    Io(String),
}

/// Début commun des labels des routines : `com.exemple.`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutinePrefix(String);

impl RoutinePrefix {
    pub fn parse(raw: &str) -> Result<Self, RoutineError> {
        let raw = raw.trim();
        let chars_ok = raw.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
        let segments: Vec<&str> = raw.strip_suffix('.').map_or(vec![""], |s| s.split('.').collect());
        let shape_ok = segments.len() >= 2 && segments.iter().all(|s| !s.is_empty());
        if !chars_ok || !shape_ok || raw.len() > PREFIX_MAX {
            return Err(RoutineError::InvalidPrefix);
        }
        if raw.to_ascii_lowercase().starts_with("com.apple.") {
            return Err(RoutineError::ApplePrefix);
        }
        Ok(Self(raw.to_owned()))
    }

    pub fn default_prefix() -> Self {
        Self(DEFAULT_PREFIX.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Nom d'une routine créée par Robiade : la fin de son label, et le nom de ses journaux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutineName(String);

impl RoutineName {
    pub fn parse(raw: &str) -> Result<Self, RoutineError> {
        let raw = raw.trim();
        let valid = !raw.is_empty()
            && raw.len() <= NAME_MAX
            && raw.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !raw.starts_with('-')
            && !raw.ends_with('-');
        valid.then(|| Self(raw.to_owned())).ok_or(RoutineError::InvalidName)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Label launchd d'une routine. Il finit dans un argument de `launchctl` et dans un nom de
/// fichier : ni `/`, ni espace, ni caractère hors de `[A-Za-z0-9._-]`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Label(String);

impl Label {
    pub fn parse(raw: &str) -> Result<Self, RoutineError> {
        let valid = !raw.is_empty()
            && raw.len() <= LABEL_MAX
            && raw.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
            && !raw.starts_with('.');
        valid.then(|| Self(raw.to_owned())).ok_or(RoutineError::InvalidLabel)
    }

    pub fn of(prefix: &RoutinePrefix, name: &RoutineName) -> Self {
        Self(format!("{}{}", prefix.0, name.0))
    }

    /// Label d'une routine du préfixe ; `None` pour un autre service.
    pub fn within(raw: &str, prefix: &RoutinePrefix) -> Option<Self> {
        let label = Self::parse(raw).ok()?;
        (raw.len() > prefix.0.len() && raw.starts_with(&prefix.0)).then_some(label)
    }

    /// Ce qui suit le préfixe : le nom affiché.
    pub fn name<'a>(&'a self, prefix: &RoutinePrefix) -> &'a str {
        self.0.strip_prefix(&prefix.0).unwrap_or(&self.0)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Nom du fichier dans `~/Library/LaunchAgents`, que launchd attend égal au label.
    pub fn file_name(&self) -> String {
        format!("{}.plist", self.0)
    }
}

fn control_free(s: &str, what: &'static str) -> Result<(), RoutineError> {
    // Un saut de ligne ou une tabulation a sa place dans un `sh -c` ; le reste ne passe pas en XML.
    if s.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        return Err(RoutineError::Control(what));
    }
    Ok(())
}

fn absolute(path: &str, what: &'static str) -> Result<(), RoutineError> {
    control_free(path, what)?;
    if path.starts_with('/') && !path.contains('\n') && !path.contains('\t') {
        Ok(())
    } else {
        Err(RoutineError::RelativePath(what))
    }
}

/// Ce que Robiade sait écrire dans un plist, et rien d'autre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutineSpec {
    /// `ProgramArguments` : le programme, en chemin absolu, puis ses arguments. Aucun shell entre eux.
    program: Vec<String>,
    working_dir: Option<String>,
    schedule: WeeklySchedule,
    stdout: Option<String>,
    stderr: Option<String>,
    /// `PATH` passé au programme : sans lui, launchd n'a que `/usr/bin:/bin:/usr/sbin:/sbin`.
    path_env: Option<String>,
}

impl RoutineSpec {
    pub fn new(
        program: Vec<String>,
        working_dir: Option<String>,
        schedule: WeeklySchedule,
        stdout: Option<String>,
        stderr: Option<String>,
        path_env: Option<String>,
    ) -> Result<Self, RoutineError> {
        let first = program.first().ok_or(RoutineError::NoProgram)?;
        if first.trim().is_empty() {
            return Err(RoutineError::NoProgram);
        }
        if !first.starts_with('/') {
            return Err(RoutineError::RelativeProgram(first.clone()));
        }
        absolute(first, "programme")?;
        if program.len() > MAX_ARGS {
            return Err(RoutineError::TooManyArguments);
        }
        for arg in &program {
            if arg.chars().count() > ARG_MAX_CHARS {
                return Err(RoutineError::ArgumentTooLong);
            }
            control_free(arg, "argument")?;
        }
        let working_dir = working_dir.filter(|d| !d.trim().is_empty());
        if let Some(dir) = &working_dir {
            absolute(dir, "le dossier de travail")?;
        }
        for (log, what) in [(&stdout, "le journal de sortie"), (&stderr, "le journal d'erreurs")] {
            if let Some(log) = log {
                absolute(log, what)?;
            }
        }
        if let Some(path) = &path_env {
            control_free(path, "PATH")?;
        }
        Ok(Self { program, working_dir, schedule, stdout, stderr, path_env })
    }

    pub fn program(&self) -> &[String] {
        &self.program
    }

    pub fn working_dir(&self) -> Option<&str> {
        self.working_dir.as_deref()
    }

    pub fn schedule(&self) -> &WeeklySchedule {
        &self.schedule
    }

    pub fn stdout(&self) -> Option<&str> {
        self.stdout.as_deref()
    }

    pub fn stderr(&self) -> Option<&str> {
        self.stderr.as_deref()
    }

    pub fn path_env(&self) -> Option<&str> {
        self.path_env.as_deref()
    }
}

/// Journaux d'une routine neuve : `~/Library/Logs/Robiade/routines/<nom>.out.log` et `.err.log`.
pub fn default_logs(home: &str, name: &RoutineName) -> (String, String) {
    let dir = format!("{}/Library/Logs/Robiade/routines", home.trim_end_matches('/'));
    (format!("{dir}/{}.out.log", name.0), format!("{dir}/{}.err.log", name.0))
}

/// Clés que Robiade écrit ; une autre rend la routine non modifiable ici.
pub const WRITTEN_KEYS: [&str; 7] = [
    "Label",
    "ProgramArguments",
    "WorkingDirectory",
    "StandardOutPath",
    "StandardErrorPath",
    "EnvironmentVariables",
    "StartCalendarInterval",
];

/// Un plist de routine, tel qu'il se lit. Les clés inconnues sont gardées par leur nom.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoredRoutine {
    /// `ProgramArguments` ; `None` si le plist n'a que `Program`, ou rien.
    pub program_arguments: Option<Vec<String>>,
    /// `Program`, quand il est là.
    pub program: Option<String>,
    pub working_dir: Option<String>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub environment: Vec<(String, String)>,
    pub schedule: Schedule,
    /// Clés que Robiade n'écrit pas (`StartInterval`, `RunAtLoad`, `KeepAlive`…), ou qu'il n'a pas
    /// su lire (`StartCalendarInterval` mal formé).
    pub other_keys: Vec<String>,
}

impl StoredRoutine {
    /// La commande lancée, telle que launchd l'exécute.
    pub fn command(&self) -> Vec<String> {
        match (&self.program_arguments, &self.program) {
            (Some(args), Some(program)) if !args.is_empty() => {
                std::iter::once(program.clone()).chain(args.iter().skip(1).cloned()).collect()
            }
            (Some(args), _) => args.clone(),
            (None, Some(program)) => vec![program.clone()],
            (None, None) => vec![],
        }
    }

    /// Le formulaire d'édition ; sinon, pourquoi la routine reste en lecture seule ici.
    pub fn editable(&self) -> Result<RoutineSpec, String> {
        let mut unknown = self.other_keys.clone();
        if self.program.is_some() {
            unknown.push("Program".into());
        }
        if !unknown.is_empty() {
            unknown.sort();
            return Err(format!("clés que Robiade n'écrit pas : {}", unknown.join(", ")));
        }
        let Some(program) = self.program_arguments.clone() else {
            return Err("pas de ProgramArguments".into());
        };
        if self.environment.iter().any(|(k, _)| k != "PATH") {
            return Err("variables d'environnement autres que PATH".into());
        }
        let Some(schedule) = self.schedule.weekly() else {
            return Err(format!("planning que Robiade n'écrit pas : {}", self.schedule.describe()));
        };
        let path = self.environment.iter().find(|(k, _)| k == "PATH").map(|(_, v)| v.clone());
        RoutineSpec::new(program, self.working_dir.clone(), schedule, self.stdout.clone(), self.stderr.clone(), path)
            .map_err(|e| e.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    Running { pid: u32 },
    /// Chargée, en attente de son prochain créneau.
    Idle,
    /// Désactivée dans launchd : elle ne se charge plus, même à la prochaine session.
    Disabled,
    /// Ni chargée ni désactivée : un plist posé à la main, ou refusé par launchd.
    Unloaded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LastExit {
    Never,
    Code(i32),
    /// Arrêtée par un signal (« Terminated: 15 »).
    Signal(String),
}

impl LastExit {
    pub fn failed(&self) -> bool {
        matches!(self, Self::Code(c) if *c != 0) || matches!(self, Self::Signal(_))
    }
}

/// Ce que launchd sait de la routine ; `runs` et `last_exit` sont perdus quand elle se décharge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutineStatus {
    pub state: ServiceState,
    pub runs: Option<u32>,
    pub last_exit: Option<LastExit>,
}

impl RoutineStatus {
    pub fn failed(&self) -> bool {
        self.last_exit.as_ref().is_some_and(LastExit::failed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Routine {
    pub label: Label,
    /// Chemin du plist.
    pub file: String,
    /// Le plist lu ; `Err` s'il est illisible, ou si son label n'est pas son nom de fichier.
    pub definition: Result<StoredRoutine, String>,
    pub status: RoutineStatus,
    /// Dernière écriture d'un de ses journaux : l'heure approximative du dernier passage.
    pub logs_written_at: Option<Millis>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutineAction {
    Run,
    Disable,
    Enable,
    Edit,
    Delete,
}

impl RoutineAction {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "run" => Some(Self::Run),
            "disable" => Some(Self::Disable),
            "enable" => Some(Self::Enable),
            "edit" => Some(Self::Edit),
            "delete" => Some(Self::Delete),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Disable => "disable",
            Self::Enable => "enable",
            Self::Edit => "edit",
            Self::Delete => "delete",
        }
    }

    /// Raison de refuser l'action sur la routine telle qu'elle est ; `None` si elle a un sens.
    /// Une routine qui tourne ne se décharge pas : `bootout` tuerait son programme en plein travail.
    pub fn refused_on(self, routine: &Routine) -> Option<String> {
        use ServiceState::*;
        let state = routine.status.state;
        let running = matches!(state, Running { .. });
        let refusal = match self {
            Self::Run => match state {
                Running { .. } => Some("la routine tourne déjà"),
                Disabled => Some("la routine est désactivée"),
                Unloaded => Some("la routine n'est pas chargée : active-la"),
                Idle => None,
            },
            Self::Disable => match state {
                Disabled => Some("la routine est déjà désactivée"),
                Running { .. } => Some("la routine tourne : attends la fin"),
                Idle | Unloaded => None,
            },
            Self::Enable => match state {
                Idle | Running { .. } => Some("la routine est déjà active"),
                Disabled | Unloaded => None,
            },
            Self::Edit | Self::Delete if running => Some("la routine tourne : attends la fin"),
            Self::Edit | Self::Delete => None,
        };
        if let Some(r) = refusal {
            return Some(r.to_owned());
        }
        match (self, &routine.definition) {
            (Self::Enable | Self::Edit, Err(e)) => Some(format!("plist illisible : {e}")),
            (Self::Edit, Ok(stored)) => stored.editable().err(),
            _ => None,
        }
    }
}

/// Fin des journaux d'une routine, déjà décodée ; `None` : pas de journal, ou illisible.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RoutineLogs {
    pub stdout: Option<String>,
    pub stderr: Option<String>,
}

/// launchd, et le dossier `~/Library/LaunchAgents`, bornés au préfixe des routines.
pub trait Routines: Send + Sync {
    /// Les routines du préfixe, plist et état relus.
    fn list(&self, prefix: &RoutinePrefix) -> Result<Vec<Routine>, RoutineError>;
    /// Une routine, relue maintenant : une action ne part jamais sur un état périmé.
    fn find(&self, label: &Label) -> Result<Routine, RoutineError>;
    /// Le plist exact que `create` ou `replace` écrirait.
    fn render(&self, label: &Label, spec: &RoutineSpec) -> String;
    /// Ce qui empêcherait la routine de partir (programme absent, dossier inexistant…).
    fn check(&self, spec: &RoutineSpec) -> Option<String>;
    /// Écrit un plist neuf et le charge ; rien ne reste si launchd le refuse.
    fn create(&self, label: &Label, spec: &RoutineSpec) -> Result<(), RoutineError>;
    /// Réécrit un plist existant et le recharge, sauf s'il est désactivé.
    fn replace(&self, label: &Label, spec: &RoutineSpec) -> Result<(), RoutineError>;
    /// Décharge la routine et supprime son plist ; ses journaux restent.
    fn delete(&self, label: &Label) -> Result<(), RoutineError>;
    fn kickstart(&self, label: &Label) -> Result<(), RoutineError>;
    fn enable(&self, label: &Label) -> Result<(), RoutineError>;
    fn disable(&self, label: &Label) -> Result<(), RoutineError>;
    /// Fin des journaux, `max_bytes` au plus chacun.
    fn logs(&self, routine: &StoredRoutine, max_bytes: u64) -> RoutineLogs;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routine_schedule::{CalendarSlot, Weekday};

    fn weekdays_nine() -> WeeklySchedule {
        let days = (1..=5).map(|n| Weekday::from_iso(n).unwrap()).collect();
        WeeklySchedule::new(days, [(9, 0)].into()).unwrap()
    }

    fn stored() -> StoredRoutine {
        StoredRoutine {
            program_arguments: Some(vec!["/usr/bin/python3".into(), "/Users/me/briefing.py".into()]),
            stdout: Some("/Users/me/logs/b.out.log".into()),
            stderr: Some("/Users/me/logs/b.err.log".into()),
            schedule: weekdays_nine().schedule(),
            ..Default::default()
        }
    }

    fn routine(state: ServiceState, definition: Result<StoredRoutine, String>) -> Routine {
        Routine {
            label: Label::parse("com.exemple.briefing").unwrap(),
            file: "/Users/me/Library/LaunchAgents/com.exemple.briefing.plist".into(),
            definition,
            status: RoutineStatus { state, runs: Some(3), last_exit: Some(LastExit::Code(0)) },
            logs_written_at: None,
        }
    }

    #[test]
    fn a_prefix_is_a_reverse_domain_ending_with_a_dot() {
        assert_eq!(RoutinePrefix::parse(" com.exemple. ").unwrap().as_str(), "com.exemple.");
        assert!(RoutinePrefix::parse(DEFAULT_PREFIX).is_ok());
        for bad in ["", ".", "com.", "com.exemple", "com..exemple.", "com/exemple.", "com.exemple .", "com.*."] {
            assert_eq!(RoutinePrefix::parse(bad), Err(RoutineError::InvalidPrefix), "{bad}");
        }
        assert_eq!(RoutinePrefix::parse("com.apple."), Err(RoutineError::ApplePrefix));
        assert_eq!(RoutinePrefix::parse("COM.Apple.Siri."), Err(RoutineError::ApplePrefix));
    }

    #[test]
    fn a_name_is_lowercase_digits_and_dashes() {
        assert!(RoutineName::parse("daily-briefing").is_ok());
        assert!(RoutineName::parse("essai2").is_ok());
        for bad in ["", "-x", "x-", "Daily", "a.b", "a/b", "a b", &"a".repeat(49)] {
            assert_eq!(RoutineName::parse(bad), Err(RoutineError::InvalidName), "{bad}");
        }
    }

    #[test]
    fn a_label_never_leaves_the_launch_agents_folder() {
        let prefix = RoutinePrefix::parse("com.exemple.").unwrap();
        let label = Label::of(&prefix, &RoutineName::parse("briefing").unwrap());
        assert_eq!(label.as_str(), "com.exemple.briefing");
        assert_eq!(label.file_name(), "com.exemple.briefing.plist");
        assert_eq!(label.name(&prefix), "briefing");
        for bad in ["", "../x", "a/b", ".hidden", "a b", "a\nb"] {
            assert_eq!(Label::parse(bad), Err(RoutineError::InvalidLabel), "{bad:?}");
        }
    }

    #[test]
    fn only_labels_after_the_prefix_are_routines() {
        let prefix = RoutinePrefix::parse("com.exemple.").unwrap();
        assert!(Label::within("com.exemple.briefing", &prefix).is_some());
        assert!(Label::within("com.exemple.", &prefix).is_none());
        assert!(Label::within("com.exemplex.briefing", &prefix).is_none());
        assert!(Label::within("com.dropbox.agent", &prefix).is_none());
    }

    #[test]
    fn a_spec_runs_an_absolute_program_without_shell() {
        let spec = |program: Vec<&str>| {
            RoutineSpec::new(program.into_iter().map(String::from).collect(), None, weekdays_nine(), None, None, None)
        };
        assert!(spec(vec!["/bin/echo", "bonjour"]).is_ok());
        assert!(spec(vec!["/bin/sh", "-c", "echo a\necho b"]).is_ok());
        assert_eq!(spec(vec![]), Err(RoutineError::NoProgram));
        assert_eq!(spec(vec!["echo"]), Err(RoutineError::RelativeProgram("echo".into())));
        assert_eq!(spec(vec!["/bin/echo", "a\u{1b}[31m"]), Err(RoutineError::Control("argument")));
        let many = std::iter::once("/bin/echo").chain(std::iter::repeat_n("x", MAX_ARGS)).collect();
        assert_eq!(spec(many), Err(RoutineError::TooManyArguments));
        let relative_dir = RoutineSpec::new(vec!["/bin/echo".into()], Some("tmp".into()), weekdays_nine(), None, None, None);
        assert_eq!(relative_dir, Err(RoutineError::RelativePath("le dossier de travail")));
        let blank_dir = RoutineSpec::new(vec!["/bin/echo".into()], Some(" ".into()), weekdays_nine(), None, None, None);
        assert_eq!(blank_dir.unwrap().working_dir(), None);
    }

    #[test]
    fn new_routines_log_under_the_library() {
        let (out, err) = default_logs("/Users/me/", &RoutineName::parse("essai").unwrap());
        assert_eq!(out, "/Users/me/Library/Logs/Robiade/routines/essai.out.log");
        assert_eq!(err, "/Users/me/Library/Logs/Robiade/routines/essai.err.log");
    }

    #[test]
    fn a_plist_with_only_written_keys_is_editable() {
        let spec = stored().editable().unwrap();
        assert_eq!(spec.program()[1], "/Users/me/briefing.py");
        assert_eq!(spec.schedule().describe(), "lun–ven 9h00");
        let with_path = StoredRoutine { environment: vec![("PATH".into(), "/opt/bin:/usr/bin".into())], ..stored() };
        assert_eq!(with_path.editable().unwrap().path_env(), Some("/opt/bin:/usr/bin"));
    }

    #[test]
    fn anything_else_stays_read_only_with_its_reason() {
        let other = StoredRoutine { other_keys: vec!["RunAtLoad".into(), "KeepAlive".into()], ..stored() };
        assert_eq!(other.editable().unwrap_err(), "clés que Robiade n'écrit pas : KeepAlive, RunAtLoad");
        let env = StoredRoutine { environment: vec![("TOKEN".into(), "x".into())], ..stored() };
        assert!(env.editable().is_err());
        let hourly = Schedule::new(vec![CalendarSlot { minute: Some(15), ..Default::default() }]).unwrap();
        let hourly = StoredRoutine { schedule: hourly, ..stored() };
        assert_eq!(hourly.editable().unwrap_err(), "planning que Robiade n'écrit pas : toutes les heures à :15");
        let program_only = StoredRoutine { program_arguments: None, program: Some("/bin/date".into()), ..stored() };
        assert!(program_only.editable().is_err());
        assert_eq!(program_only.command(), ["/bin/date"]);
    }

    #[test]
    fn program_overrides_the_first_argument_like_launchd() {
        let both = StoredRoutine {
            program: Some("/usr/local/bin/tool".into()),
            program_arguments: Some(vec!["tool".into(), "--quiet".into()]),
            ..Default::default()
        };
        assert_eq!(both.command(), ["/usr/local/bin/tool", "--quiet"]);
    }

    #[test]
    fn a_signal_or_a_non_zero_code_is_a_failure() {
        assert!(!LastExit::Never.failed());
        assert!(!LastExit::Code(0).failed());
        assert!(LastExit::Code(3).failed());
        assert!(LastExit::Signal("Terminated: 15".into()).failed());
    }

    #[test]
    fn actions_follow_the_state_of_launchd() {
        let idle = routine(ServiceState::Idle, Ok(stored()));
        let running = routine(ServiceState::Running { pid: 42 }, Ok(stored()));
        let disabled = routine(ServiceState::Disabled, Ok(stored()));
        assert_eq!(RoutineAction::Run.refused_on(&idle), None);
        assert_eq!(RoutineAction::Run.refused_on(&running).unwrap(), "la routine tourne déjà");
        assert_eq!(RoutineAction::Run.refused_on(&disabled).unwrap(), "la routine est désactivée");
        assert_eq!(RoutineAction::Disable.refused_on(&idle), None);
        assert!(RoutineAction::Disable.refused_on(&running).is_some());
        assert_eq!(RoutineAction::Enable.refused_on(&disabled), None);
        assert!(RoutineAction::Enable.refused_on(&idle).is_some());
        assert_eq!(RoutineAction::Edit.refused_on(&idle), None);
        assert_eq!(RoutineAction::Delete.refused_on(&disabled), None);
        for action in [RoutineAction::Edit, RoutineAction::Delete] {
            assert_eq!(action.refused_on(&running).unwrap(), "la routine tourne : attends la fin");
        }
    }

    #[test]
    fn an_unreadable_plist_can_be_deleted_but_not_edited_nor_enabled() {
        let broken = routine(ServiceState::Unloaded, Err("plist illisible".into()));
        assert!(RoutineAction::Edit.refused_on(&broken).is_some());
        assert!(RoutineAction::Enable.refused_on(&broken).is_some());
        assert_eq!(RoutineAction::Delete.refused_on(&broken), None);
        let foreign = routine(ServiceState::Idle, Ok(StoredRoutine { other_keys: vec!["KeepAlive".into()], ..stored() }));
        assert!(RoutineAction::Edit.refused_on(&foreign).is_some());
        assert_eq!(RoutineAction::Run.refused_on(&foreign), None);
    }

    #[test]
    fn actions_round_trip_through_their_name() {
        for a in [RoutineAction::Run, RoutineAction::Disable, RoutineAction::Enable, RoutineAction::Edit, RoutineAction::Delete] {
            assert_eq!(RoutineAction::parse(a.as_str()), Some(a));
        }
        assert_eq!(RoutineAction::parse("bootout"), None);
    }
}
