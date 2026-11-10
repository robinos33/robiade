//! Routines par launchd : les plists de `~/Library/LaunchAgents` dont le label commence par le
//! préfixe, lus par `plutil`, et leurs services dans le domaine `gui/<uid>`, pilotés par
//! `launchctl`. Les deux programmes sont lancés par leur chemin absolu, sans shell.
//!
//! Faits mesurés sur macOS 26 (`launchctl` n'a pas de format stable, la lecture est tolérante) :
//! - `print` d'un service inconnu sort en 113 ; un service désactivé n'est plus chargé, son état
//!   ne se lit que dans `print-disabled` ;
//! - `bootstrap` d'un service déjà chargé, ou désactivé, sort en 5 ;
//! - `kickstart` d'un service qui tourne ne relance rien ;
//! - launchd crée les dossiers des journaux qu'il ouvre.

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use robiade_core::routine_schedule::{CalendarSlot, Schedule};
use robiade_core::routines::{
    Label, LastExit, Routine, RoutineError, RoutineLogs, RoutinePrefix, RoutineSpec, RoutineStatus, Routines,
    ServiceState, StoredRoutine, WRITTEN_KEYS,
};
use serde_json::Value;

const LAUNCHCTL: &str = "/bin/launchctl";
const PLUTIL: &str = "/usr/bin/plutil";
/// `print` d'un service que launchd ne connaît pas.
const NOT_FOUND: i32 = 113;
/// `bootout` d'un service déjà déchargé.
const NO_SUCH_PROCESS: i32 = 3;

/// Sortie d'une commande, décodée.
#[derive(Debug, Clone, Default)]
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// `launchctl`, derrière un trait pour que les tests ne touchent pas au vrai launchd.
pub trait Launchctl: Send + Sync {
    fn run(&self, args: &[&str]) -> Result<Output, String>;
}

pub struct SystemLaunchctl;

impl Launchctl for SystemLaunchctl {
    fn run(&self, args: &[&str]) -> Result<Output, String> {
        let out = std::process::Command::new(LAUNCHCTL).args(args).output().map_err(|e| e.to_string())?;
        Ok(Output {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }
}

pub struct Launchd<L: Launchctl = SystemLaunchctl> {
    agents_dir: PathBuf,
    domain: String,
    launchctl: L,
}

impl Launchd<SystemLaunchctl> {
    /// Les LaunchAgents de l'utilisateur courant.
    pub fn for_user(home: &Path) -> Self {
        // SAFETY : `getuid` ne lit que l'identité du processus et n'échoue jamais.
        let uid = unsafe { libc::getuid() };
        Self::new(home.join("Library").join("LaunchAgents"), format!("gui/{uid}"), SystemLaunchctl)
    }
}

impl<L: Launchctl> Launchd<L> {
    pub fn new(agents_dir: PathBuf, domain: String, launchctl: L) -> Self {
        Self { agents_dir, domain, launchctl }
    }

    fn file(&self, label: &Label) -> PathBuf {
        self.agents_dir.join(label.file_name())
    }

    fn service(&self, label: &Label) -> String {
        format!("{}/{}", self.domain, label.as_str())
    }

    fn launchctl(&self, args: &[&str]) -> Result<Output, RoutineError> {
        self.launchctl.run(args).map_err(RoutineError::Launchd)
    }

    /// Commande qui doit réussir ; sinon, la première ligne de son erreur.
    fn must(&self, args: &[&str]) -> Result<(), RoutineError> {
        let out = self.launchctl(args)?;
        if out.code == 0 {
            return Ok(());
        }
        let message = out.stderr.lines().chain(out.stdout.lines()).find(|l| !l.trim().is_empty());
        Err(RoutineError::Launchd(message.map_or_else(|| format!("code {}", out.code), |l| l.trim().to_owned())))
    }

    fn disabled_services(&self) -> Result<BTreeMap<String, bool>, RoutineError> {
        let out = self.launchctl(&["print-disabled", &self.domain])?;
        Ok(parse_disabled(&out.stdout))
    }

    fn status(&self, label: &Label, disabled: &BTreeMap<String, bool>) -> Result<RoutineStatus, RoutineError> {
        let out = self.launchctl(&["print", &self.service(label)])?;
        if out.code == NOT_FOUND {
            let state =
                if disabled.get(label.as_str()) == Some(&true) { ServiceState::Disabled } else { ServiceState::Unloaded };
            return Ok(RoutineStatus { state, runs: None, last_exit: None });
        }
        if out.code != 0 {
            return Err(RoutineError::Launchd(out.stderr.trim().to_owned()));
        }
        Ok(parse_print(&out.stdout))
    }

    fn loaded(&self, label: &Label) -> Result<bool, RoutineError> {
        Ok(self.launchctl(&["print", &self.service(label)])?.code == 0)
    }

    fn bootout(&self, label: &Label) -> Result<(), RoutineError> {
        let out = self.launchctl(&["bootout", &self.service(label)])?;
        if out.code == 0 || out.code == NO_SUCH_PROCESS {
            return Ok(());
        }
        Err(RoutineError::Launchd(out.stderr.lines().next().unwrap_or("bootout refusé").trim().to_owned()))
    }

    fn bootstrap(&self, label: &Label) -> Result<(), RoutineError> {
        self.must(&["bootstrap", &self.domain, &self.file(label).to_string_lossy()])
    }

    fn routine(&self, label: Label, path: PathBuf, disabled: &BTreeMap<String, bool>) -> Result<Routine, RoutineError> {
        let definition = read_plist(&path, &label);
        let logs_written_at = definition.as_ref().ok().and_then(logs_written_at);
        Ok(Routine {
            status: self.status(&label, disabled)?,
            file: path.to_string_lossy().into_owned(),
            label,
            definition,
            logs_written_at,
        })
    }

    /// Le plist écrit à côté de sa place, vérifié par `plutil` : rien n'est renommé s'il est faux.
    fn staged(&self, label: &Label, spec: &RoutineSpec) -> Result<PathBuf, RoutineError> {
        std::fs::create_dir_all(&self.agents_dir).map_err(io)?;
        let tmp = self.agents_dir.join(format!(".{}.tmp", label.file_name()));
        let _ = std::fs::remove_file(&tmp);
        let mut file =
            std::fs::OpenOptions::new().write(true).create_new(true).mode(0o644).open(&tmp).map_err(io)?;
        let written = file.write_all(render(label, spec).as_bytes()).and_then(|()| file.sync_all());
        if let Err(e) = written.map_err(io).and_then(|()| lint(&tmp)) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        Ok(tmp)
    }

    fn regular_file(&self, label: &Label) -> Result<PathBuf, RoutineError> {
        let path = self.file(label);
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_file() => Ok(path),
            Ok(_) => Err(RoutineError::Io(format!("{} n'est pas un fichier ordinaire", path.display()))),
            Err(_) => Err(RoutineError::NotFound),
        }
    }
}

fn io(e: std::io::Error) -> RoutineError {
    RoutineError::Io(e.to_string())
}

impl<L: Launchctl> Routines for Launchd<L> {
    fn list(&self, prefix: &RoutinePrefix) -> Result<Vec<Routine>, RoutineError> {
        let entries = match std::fs::read_dir(&self.agents_dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(io(e)),
        };
        let mut found = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(label) = name.strip_suffix(".plist").and_then(|stem| Label::within(stem, prefix)) else {
                continue;
            };
            // Un lien pourrait viser un plist hors du dossier : il n'est pas une routine.
            if entry.file_type().is_ok_and(|t| t.is_file()) {
                found.push((label, entry.path()));
            }
        }
        found.sort();
        let disabled = self.disabled_services()?;
        found.into_iter().map(|(label, path)| self.routine(label, path, &disabled)).collect()
    }

    fn find(&self, label: &Label) -> Result<Routine, RoutineError> {
        let path = self.regular_file(label)?;
        self.routine(label.clone(), path, &self.disabled_services()?)
    }

    fn render(&self, label: &Label, spec: &RoutineSpec) -> String {
        render(label, spec)
    }

    fn check(&self, spec: &RoutineSpec) -> Option<String> {
        let program = &spec.program()[0];
        match std::fs::metadata(program) {
            Ok(m) if m.is_file() && m.permissions().mode() & 0o111 != 0 => {}
            Ok(m) if m.is_file() => return Some(format!("{program} n'est pas exécutable")),
            Ok(_) => return Some(format!("{program} n'est pas un fichier")),
            Err(_) => return Some(format!("{program} n'existe pas")),
        }
        if let Some(dir) = spec.working_dir() {
            if !Path::new(dir).is_dir() {
                return Some(format!("le dossier {dir} n'existe pas"));
            }
        }
        None
    }

    fn create(&self, label: &Label, spec: &RoutineSpec) -> Result<(), RoutineError> {
        let target = self.file(label);
        if std::fs::symlink_metadata(&target).is_ok() {
            return Err(RoutineError::Exists);
        }
        let tmp = self.staged(label, spec)?;
        // Un lien dur n'écrase rien : une routine apparue entre-temps reste intacte.
        let linked = std::fs::hard_link(&tmp, &target);
        let _ = std::fs::remove_file(&tmp);
        linked.map_err(|e| if e.kind() == std::io::ErrorKind::AlreadyExists { RoutineError::Exists } else { io(e) })?;
        // Une ancienne routine du même nom a pu rester désactivée : la neuve part active.
        let loaded = self.must(&["enable", &self.service(label)]).and_then(|()| self.bootstrap(label));
        if loaded.is_err() {
            let _ = std::fs::remove_file(&target);
        }
        loaded
    }

    fn replace(&self, label: &Label, spec: &RoutineSpec) -> Result<(), RoutineError> {
        let target = self.regular_file(label)?;
        let previous = std::fs::read(&target).map_err(io)?;
        let disabled = self.disabled_services()?.get(label.as_str()) == Some(&true);
        let tmp = self.staged(label, spec)?;
        let was_loaded = self.loaded(label)?;
        if was_loaded {
            if let Err(e) = self.bootout(label) {
                let _ = std::fs::remove_file(&tmp);
                return Err(e);
            }
        }
        std::fs::rename(&tmp, &target).map_err(io)?;
        if disabled {
            return Ok(());
        }
        match self.bootstrap(label) {
            Ok(()) => Ok(()),
            Err(e) => {
                // launchd refuse le nouveau plist : l'ancien revient, tel qu'il tournait.
                let _ = std::fs::write(&target, previous);
                if was_loaded {
                    let _ = self.bootstrap(label);
                }
                Err(e)
            }
        }
    }

    fn delete(&self, label: &Label) -> Result<(), RoutineError> {
        let target = self.regular_file(label)?;
        self.bootout(label)?;
        std::fs::remove_file(target).map_err(io)
    }

    fn kickstart(&self, label: &Label) -> Result<(), RoutineError> {
        self.must(&["kickstart", &self.service(label)])
    }

    fn enable(&self, label: &Label) -> Result<(), RoutineError> {
        self.regular_file(label)?;
        self.must(&["enable", &self.service(label)])?;
        if self.loaded(label)? { Ok(()) } else { self.bootstrap(label) }
    }

    /// Désactivée puis déchargée : elle ne part plus, et ne revient pas à la prochaine session.
    fn disable(&self, label: &Label) -> Result<(), RoutineError> {
        self.must(&["disable", &self.service(label)])?;
        self.bootout(label)
    }

    fn logs(&self, routine: &StoredRoutine, max_bytes: u64) -> RoutineLogs {
        let tail = |path: &Option<String>| path.as_deref().and_then(|p| tail(Path::new(p), max_bytes));
        let stderr = if routine.stderr == routine.stdout { None } else { tail(&routine.stderr) };
        RoutineLogs { stdout: tail(&routine.stdout), stderr }
    }
}

/// Fin d'un fichier ordinaire, à partir d'un début de ligne.
fn tail(path: &Path, max_bytes: u64) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok().filter(|m| m.is_file())?.len();
    let start = len.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.take(max_bytes).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if start == 0 {
        return Some(text);
    }
    Some(text.split_once('\n').map_or(text.clone(), |(_, rest)| rest.to_owned()))
}

fn logs_written_at(routine: &StoredRoutine) -> Option<i64> {
    [&routine.stdout, &routine.stderr]
        .into_iter()
        .flatten()
        .filter_map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
        .filter_map(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .max()
}

fn lint(path: &Path) -> Result<(), RoutineError> {
    let out = std::process::Command::new(PLUTIL).arg("-lint").arg("-s").arg(path).output().map_err(io)?;
    if out.status.success() {
        Ok(())
    } else {
        Err(RoutineError::Io(format!("plist refusé par plutil : {}", String::from_utf8_lossy(&out.stdout).trim())))
    }
}

/// Le plist, lu par `plutil` en JSON : un plist binaire ou XML se lit pareil. Un plist qu'il ne sait
/// pas convertir (dates, données binaires) reste illisible ici.
fn read_plist(path: &Path, label: &Label) -> Result<StoredRoutine, String> {
    let out = std::process::Command::new(PLUTIL)
        .args(["-convert", "json", "-o", "-", "--"])
        .arg(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("plist illisible : {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let value: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("plist illisible : {e}"))?;
    stored_routine(&value, label)
}

fn stored_routine(value: &Value, label: &Label) -> Result<StoredRoutine, String> {
    let dict = value.as_object().ok_or("le plist n'est pas un dictionnaire")?;
    match dict.get("Label").and_then(Value::as_str) {
        Some(l) if l == label.as_str() => {}
        Some(l) => return Err(format!("son label ({l}) n'est pas son nom de fichier")),
        None => return Err("pas de Label".into()),
    }
    let mut routine = StoredRoutine::default();
    let string = |v: &Value| v.as_str().map(str::to_owned);
    for (key, v) in dict {
        let read = match key.as_str() {
            "Label" => true,
            "ProgramArguments" => {
                routine.program_arguments = v.as_array().and_then(|a| a.iter().map(string).collect());
                routine.program_arguments.is_some()
            }
            "Program" => {
                routine.program = string(v);
                routine.program.is_some()
            }
            "WorkingDirectory" => {
                routine.working_dir = string(v);
                routine.working_dir.is_some()
            }
            "StandardOutPath" => {
                routine.stdout = string(v);
                routine.stdout.is_some()
            }
            "StandardErrorPath" => {
                routine.stderr = string(v);
                routine.stderr.is_some()
            }
            "EnvironmentVariables" => match v.as_object() {
                Some(env) => {
                    let pairs: Option<Vec<_>> = env.iter().map(|(k, v)| Some((k.clone(), string(v)?))).collect();
                    pairs.map(|p| routine.environment = p).is_some()
                }
                None => false,
            },
            "StartCalendarInterval" => match schedule(v) {
                Some(s) => {
                    routine.schedule = s;
                    true
                }
                None => false,
            },
            _ => false,
        };
        // Une clé connue mais mal formée est traitée comme inconnue : Robiade ne la réécrit pas.
        if !read || !WRITTEN_KEYS.contains(&key.as_str()) && key != "Program" {
            routine.other_keys.push(key.clone());
        }
    }
    Ok(routine)
}

/// Un créneau seul ou une liste ; toute clé inconnue ou valeur hors bornes rend le planning illisible.
fn schedule(v: &Value) -> Option<Schedule> {
    let slots: Vec<&Value> = match v {
        Value::Array(list) => list.iter().collect(),
        Value::Object(_) => vec![v],
        _ => return None,
    };
    let slot = |v: &Value| -> Option<CalendarSlot> {
        let dict = v.as_object()?;
        if dict.keys().any(|k| !matches!(k.as_str(), "Month" | "Day" | "Weekday" | "Hour" | "Minute")) {
            return None;
        }
        let get = |k: &str| -> Option<Option<i64>> {
            match dict.get(k) {
                None => Some(None),
                Some(v) => v.as_i64().map(Some),
            }
        };
        CalendarSlot::new(get("Month")?, get("Day")?, get("Weekday")?, get("Hour")?, get("Minute")?).ok()
    };
    Schedule::new(slots.into_iter().map(slot).collect::<Option<_>>()?).ok()
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Le plist XML d'une routine, clés dans un ordre fixe pour qu'un aperçu se compare d'un coup d'œil.
pub fn render(label: &Label, spec: &RoutineSpec) -> String {
    let mut x = String::from(concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
        "<plist version=\"1.0\">\n<dict>\n",
    ));
    let string = |x: &mut String, key: &str, value: &str| {
        x.push_str(&format!("\t<key>{key}</key>\n\t<string>{}</string>\n", escape(value)));
    };
    string(&mut x, "Label", label.as_str());
    x.push_str("\t<key>ProgramArguments</key>\n\t<array>\n");
    for arg in spec.program() {
        x.push_str(&format!("\t\t<string>{}</string>\n", escape(arg)));
    }
    x.push_str("\t</array>\n");
    if let Some(dir) = spec.working_dir() {
        string(&mut x, "WorkingDirectory", dir);
    }
    if let Some(path) = spec.path_env() {
        x.push_str(&format!(
            "\t<key>EnvironmentVariables</key>\n\t<dict>\n\t\t<key>PATH</key>\n\t\t<string>{}</string>\n\t</dict>\n",
            escape(path)
        ));
    }
    x.push_str("\t<key>StartCalendarInterval</key>\n\t<array>\n");
    for slot in spec.schedule().schedule().slots() {
        x.push_str("\t\t<dict>\n");
        let fields = [("Weekday", slot.weekday.map(|w| w.launchd())), ("Hour", slot.hour), ("Minute", slot.minute)];
        for (key, value) in fields {
            if let Some(v) = value {
                x.push_str(&format!("\t\t\t<key>{key}</key>\n\t\t\t<integer>{v}</integer>\n"));
            }
        }
        x.push_str("\t\t</dict>\n");
    }
    x.push_str("\t</array>\n");
    if let Some(out) = spec.stdout() {
        string(&mut x, "StandardOutPath", out);
    }
    if let Some(err) = spec.stderr() {
        string(&mut x, "StandardErrorPath", err);
    }
    x.push_str("</dict>\n</plist>\n");
    x
}

/// Lignes du premier niveau de `launchctl print` (une tabulation) : les blocs imbriqués ont aussi
/// des `state = …`, qui ne sont pas celui du service.
fn parse_print(out: &str) -> RoutineStatus {
    let mut fields = BTreeMap::new();
    for line in out.lines() {
        let Some(rest) = line.strip_prefix('\t') else { continue };
        if rest.starts_with('\t') {
            continue;
        }
        if let Some((k, v)) = rest.split_once(" = ") {
            fields.entry(k.trim()).or_insert_with(|| v.trim());
        }
    }
    let pid = fields.get("pid").and_then(|p| p.parse().ok());
    let state = match (fields.get("state").copied(), pid) {
        (Some("running"), Some(pid)) => ServiceState::Running { pid },
        _ => ServiceState::Idle,
    };
    let last_exit = match (fields.get("last terminating signal"), fields.get("last exit code")) {
        (Some(signal), _) => Some(LastExit::Signal((*signal).to_owned())),
        (None, Some(code)) => match code.parse() {
            Ok(code) => Some(LastExit::Code(code)),
            Err(_) => Some(LastExit::Never),
        },
        (None, None) => None,
    };
    RoutineStatus { state, runs: fields.get("runs").and_then(|r| r.parse().ok()), last_exit }
}

/// `"label" => disabled` (ou `true`, sur les macOS plus anciens).
fn parse_disabled(out: &str) -> BTreeMap<String, bool> {
    out.lines()
        .filter_map(|line| {
            let (label, state) = line.trim().split_once(" => ")?;
            let label = label.strip_prefix('"')?.strip_suffix('"')?;
            Some((label.to_owned(), matches!(state.trim(), "disabled" | "true")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use robiade_core::routine_schedule::{Weekday, WeeklySchedule};
    use robiade_core::routines::RoutineName;
    use std::sync::Mutex;

    /// Sortie réelle de `launchctl print` (macOS 26), abrégée.
    const PRINT_IDLE: &str = "gui/501/com.exemple.briefing = {\n\tactive count = 0\n\tpath = /Users/me/Library/LaunchAgents/com.exemple.briefing.plist\n\ttype = LaunchAgent\n\tstate = not running\n\n\tprogram = /usr/bin/python3\n\targuments = {\n\t\t/usr/bin/python3\n\t}\n\n\truns = 10\n\tlast exit code = 0\n\n\tresource coalition = {\n\t\tID = 15480\n\t\tstate = active\n\t}\n}\n";
    const PRINT_RUNNING: &str = "gui/501/x = {\n\tstate = running\n\truns = 1\n\tpid = 25485\n\tlast exit code = (never exited)\n\tjetsam coalition = {\n\t\tstate = active\n\t}\n}\n";
    const PRINT_KILLED: &str = "gui/501/x = {\n\tstate = not running\n\truns = 2\n\tlast terminating signal = Terminated: 15\n}\n";
    const DISABLED: &str = "\n\tdisabled services = {\n\t\t\"com.exemple.briefing\" => enabled\n\t\t\"com.exemple.off\" => disabled\n\t\t\"com.exemple.old\" => true\n\t}\n";

    #[test]
    fn print_is_read_at_its_first_level_only() {
        let idle = parse_print(PRINT_IDLE);
        assert_eq!(idle, RoutineStatus { state: ServiceState::Idle, runs: Some(10), last_exit: Some(LastExit::Code(0)) });
        let running = parse_print(PRINT_RUNNING);
        assert_eq!(running.state, ServiceState::Running { pid: 25485 });
        assert_eq!(running.last_exit, Some(LastExit::Never));
        assert_eq!(parse_print(PRINT_KILLED).last_exit, Some(LastExit::Signal("Terminated: 15".into())));
    }

    #[test]
    fn disabled_services_read_both_spellings() {
        let disabled = parse_disabled(DISABLED);
        assert_eq!(disabled.get("com.exemple.briefing"), Some(&false));
        assert_eq!(disabled.get("com.exemple.off"), Some(&true));
        assert_eq!(disabled.get("com.exemple.old"), Some(&true));
    }

    fn label(raw: &str) -> Label {
        Label::parse(raw).unwrap()
    }

    #[test]
    fn a_plist_is_read_key_by_key() {
        let json = serde_json::json!({
            "Label": "com.exemple.briefing",
            "ProgramArguments": ["/usr/bin/python3", "/Users/me/briefing.py"],
            "StandardOutPath": "/Users/me/b.out.log",
            "StartCalendarInterval": [{"Weekday": 1, "Hour": 9, "Minute": 0}, {"Weekday": 7, "Hour": 9, "Minute": 0}],
            "RunAtLoad": true,
        });
        let r = stored_routine(&json, &label("com.exemple.briefing")).unwrap();
        assert_eq!(r.command(), ["/usr/bin/python3", "/Users/me/briefing.py"]);
        assert_eq!(r.schedule.describe(), "lun, dim 9h00");
        assert_eq!(r.other_keys, ["RunAtLoad"]);
    }

    #[test]
    fn a_label_that_is_not_the_file_name_is_refused() {
        let json = serde_json::json!({ "Label": "com.autre.chose" });
        assert!(stored_routine(&json, &label("com.exemple.briefing")).unwrap_err().contains("com.autre.chose"));
    }

    #[test]
    fn a_malformed_known_key_is_kept_as_unknown() {
        let json = serde_json::json!({
            "Label": "com.exemple.x",
            "ProgramArguments": "/bin/echo",
            "StartCalendarInterval": {"Hour": 25},
            "EnvironmentVariables": {"PATH": 3},
        });
        let r = stored_routine(&json, &label("com.exemple.x")).unwrap();
        assert_eq!(r.other_keys, ["ProgramArguments", "StartCalendarInterval", "EnvironmentVariables"]);
        assert!(r.editable().is_err());
    }

    fn spec(program: &[&str]) -> RoutineSpec {
        let days = [Weekday::Monday, Weekday::Friday].into();
        RoutineSpec::new(
            program.iter().map(|s| s.to_string()).collect(),
            Some("/tmp".into()),
            WeeklySchedule::new(days, [(9, 0), (17, 30)].into()).unwrap(),
            Some("/tmp/x.out.log".into()),
            Some("/tmp/x.err.log".into()),
            Some("/opt/homebrew/bin:/usr/bin:/bin".into()),
        )
        .unwrap()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("robiade-launchd-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn what_is_written_reads_back_the_same() {
        let dir = temp_dir("round-trip");
        let l = label("com.exemple.essai");
        let s = spec(&["/bin/sh", "-c", "echo \"a & b\" <x>\necho c"]);
        let path = dir.join(l.file_name());
        std::fs::write(&path, render(&l, &s)).unwrap();
        lint(&path).unwrap();
        let back = read_plist(&path, &l).unwrap();
        assert_eq!(back.editable().unwrap(), s);
        assert!(back.other_keys.is_empty());
    }

    /// launchd simulé : chaque appel est noté ; `print` répond selon les services chargés.
    #[derive(Default)]
    struct FakeLaunchctl {
        calls: Mutex<Vec<String>>,
        loaded: Mutex<Vec<String>>,
        refuse_bootstrap: bool,
    }

    impl Launchctl for FakeLaunchctl {
        fn run(&self, args: &[&str]) -> Result<Output, String> {
            self.calls.lock().unwrap().push(args.join(" "));
            let mut loaded = self.loaded.lock().unwrap();
            let ok = Output::default();
            Ok(match args {
                ["print-disabled", _] => Output { stdout: "\tdisabled services = {\n\t}\n".into(), ..ok },
                ["print", service] if loaded.iter().any(|l| l == service) => Output { stdout: PRINT_IDLE.into(), ..ok },
                ["print", _] => Output { code: NOT_FOUND, ..ok },
                ["bootstrap", ..] if self.refuse_bootstrap => {
                    Output { code: 5, stderr: "Bootstrap failed: 5: Input/output error\n".into(), ..ok }
                }
                ["bootstrap", domain, path] => {
                    let stem = Path::new(path).file_stem().unwrap().to_string_lossy().into_owned();
                    loaded.push(format!("{domain}/{stem}"));
                    ok
                }
                ["bootout", service] => {
                    loaded.retain(|l| l != service);
                    ok
                }
                _ => ok,
            })
        }
    }

    fn launchd(name: &str, fake: FakeLaunchctl) -> (Launchd<FakeLaunchctl>, PathBuf) {
        let dir = temp_dir(name);
        (Launchd::new(dir.clone(), "gui/501".into(), fake), dir)
    }

    #[test]
    fn a_new_routine_is_written_enabled_and_loaded() {
        let (launchd, dir) = launchd("create", FakeLaunchctl::default());
        let prefix = RoutinePrefix::parse("com.exemple.").unwrap();
        let l = Label::of(&prefix, &RoutineName::parse("essai").unwrap());
        launchd.create(&l, &spec(&["/bin/echo", "bonjour"])).unwrap();
        assert!(dir.join("com.exemple.essai.plist").is_file());
        let calls = launchd.launchctl.calls.lock().unwrap().clone();
        assert_eq!(calls[0], "enable gui/501/com.exemple.essai");
        assert!(calls[1].starts_with("bootstrap gui/501 "));
        let listed = launchd.list(&prefix).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].status.state, ServiceState::Idle);
        assert_eq!(launchd.create(&l, &spec(&["/bin/echo"])), Err(RoutineError::Exists));
        // Aucun fichier temporaire ne traîne.
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    }

    #[test]
    fn a_routine_launchd_refuses_leaves_nothing_behind() {
        let fake = FakeLaunchctl { refuse_bootstrap: true, ..Default::default() };
        let (launchd, dir) = launchd("refused", fake);
        let err = launchd.create(&label("com.exemple.essai"), &spec(&["/bin/echo"])).unwrap_err();
        assert_eq!(err, RoutineError::Launchd("Bootstrap failed: 5: Input/output error".into()));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    }

    #[test]
    fn only_regular_files_of_the_prefix_are_listed() {
        let (launchd, dir) = launchd("list", FakeLaunchctl::default());
        let mine = label("com.exemple.a");
        std::fs::write(dir.join(mine.file_name()), render(&mine, &spec(&["/bin/echo"]))).unwrap();
        std::fs::write(dir.join("com.dropbox.agent.plist"), "x").unwrap();
        std::fs::write(dir.join("com.exemple.notes.txt"), "x").unwrap();
        std::os::unix::fs::symlink(dir.join(mine.file_name()), dir.join("com.exemple.lien.plist")).unwrap();
        let listed = launchd.list(&RoutinePrefix::parse("com.exemple.").unwrap()).unwrap();
        assert_eq!(listed.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), ["com.exemple.a"]);
        assert_eq!(listed[0].status.state, ServiceState::Unloaded);
        assert!(listed[0].definition.is_ok());
    }

    #[test]
    fn a_replaced_routine_is_reloaded_and_restored_if_refused() {
        let (launchd, dir) = launchd("replace", FakeLaunchctl::default());
        let l = label("com.exemple.r");
        launchd.create(&l, &spec(&["/bin/echo", "v1"])).unwrap();
        launchd.replace(&l, &spec(&["/bin/echo", "v2"])).unwrap();
        let text = std::fs::read_to_string(dir.join(l.file_name())).unwrap();
        assert!(text.contains("v2"));
        let calls = launchd.launchctl.calls.lock().unwrap().clone();
        assert!(calls.iter().any(|c| c == "bootout gui/501/com.exemple.r"));
        assert!(calls.last().unwrap().starts_with("bootstrap "));

        let refusing = Launchd::new(dir.clone(), "gui/501".into(), FakeLaunchctl { refuse_bootstrap: true, ..Default::default() });
        assert!(refusing.replace(&l, &spec(&["/bin/echo", "v3"])).is_err());
        assert!(std::fs::read_to_string(dir.join(l.file_name())).unwrap().contains("v2"));
    }

    #[test]
    fn a_deleted_routine_is_unloaded_then_removed() {
        let (launchd, dir) = launchd("delete", FakeLaunchctl::default());
        let l = label("com.exemple.d");
        launchd.create(&l, &spec(&["/bin/echo"])).unwrap();
        launchd.delete(&l).unwrap();
        assert!(!dir.join(l.file_name()).exists());
        assert!(launchd.launchctl.calls.lock().unwrap().iter().any(|c| c == "bootout gui/501/com.exemple.d"));
        assert_eq!(launchd.delete(&l), Err(RoutineError::NotFound));
    }

    #[test]
    fn a_missing_program_or_folder_is_caught_before_writing() {
        let (launchd, _) = launchd("check", FakeLaunchctl::default());
        assert_eq!(launchd.check(&spec(&["/bin/echo"])), None);
        assert!(launchd.check(&spec(&["/nulle/part"])).unwrap().contains("n'existe pas"));
        assert!(launchd.check(&spec(&["/etc/hosts"])).unwrap().contains("pas exécutable"));
    }

    #[test]
    fn logs_end_at_a_line_start() {
        let dir = temp_dir("logs");
        let out = dir.join("x.out.log");
        std::fs::write(&out, "une\ndeux\ntrois\n").unwrap();
        assert_eq!(tail(&out, 1024).unwrap(), "une\ndeux\ntrois\n");
        assert_eq!(tail(&out, 8).unwrap(), "trois\n");
        assert_eq!(tail(&dir.join("absent.log"), 8), None);
        assert_eq!(tail(&dir, 8), None);
    }
}
