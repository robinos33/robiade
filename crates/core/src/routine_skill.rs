//! Routine de skill : une routine dont la commande lance un skill Claude Code dans un dossier.
//!
//! Ce n'est qu'une forme de `ProgramArguments` : Robiade l'écrit, et la reconnaît à la relecture.
//! Le plist n'a aucune clé de plus qu'une autre routine, et la commande ne contourne jamais les
//! permissions de Claude Code : le skill n'a que les outils listés dans `--allowedTools`.
//!
//! La forme écrite, et la seule reconnue :
//! `<claude> -p "/<skill> <arguments>" --max-turns <n> [--model <m>] [--effort <e>] [--allowedTools <règle>…]`.
//! `--allowedTools` vient en dernier : il prend chaque argument qui le suit comme une règle.

use crate::agents::is_cli_token;

/// Tours par défaut : de quoi dérouler un skill, sans laisser un skill bloqué occuper la routine.
pub const DEFAULT_MAX_TURNS: u32 = 30;
pub const MAX_TURNS_LIMIT: u32 = 500;
const SKILL_MAX: usize = 128;
const ARGUMENTS_MAX: usize = 2_000;
const RULE_MAX: usize = 300;
/// Avec le programme et les options, une routine de skill reste sous les 64 arguments d'une routine.
pub const RULES_MAX: usize = 40;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SkillRunError {
    #[error("skill invalide : lettres, chiffres, `-`, `_`, `.` et `:` ({SKILL_MAX} caractères au plus)")]
    InvalidSkill,
    #[error("arguments du skill : une seule ligne, {ARGUMENTS_MAX} caractères au plus")]
    InvalidArguments,
    #[error("modèle invalide : {0}")]
    InvalidModel(String),
    #[error("effort invalide : {0}")]
    InvalidEffort(String),
    #[error("nombre de tours : entre 1 et {MAX_TURNS_LIMIT}")]
    InvalidMaxTurns,
    #[error("règle d'outil invalide : {0}")]
    InvalidRule(String),
    #[error("{RULES_MAX} règles d'outils au plus")]
    TooManyRules,
}

/// Un skill que `claude` propose depuis un dossier : de son utilisateur, du projet, d'un plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillOption {
    pub name: String,
    pub description: String,
    /// Forme attendue des arguments (`[low|high] [<pr#>]`) ; vide si le skill n'en dit rien.
    pub argument_hint: String,
}

/// Un nom de skill tel que `claude` l'accepte après `/` : `audit`, `plugin:skill`.
pub fn is_skill_name(raw: &str) -> bool {
    !raw.is_empty()
        && raw.len() <= SKILL_MAX
        && raw.starts_with(|c: char| c.is_ascii_alphanumeric())
        && raw.chars().all(|c| c.is_ascii_alphanumeric() || "-_.:".contains(c))
}

/// Le skill lancé, et ce qui borne son passage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillRun {
    /// Nom du skill, sans `/` : `audit`, `plugin:skill`.
    skill: String,
    /// Texte passé après le nom, sur une ligne ; vide s'il n'y en a pas.
    arguments: String,
    model: Option<String>,
    effort: Option<String>,
    max_turns: u32,
    /// Règles de permission de Claude Code (`Read`, `Bash(gh pr list:*)`) ; vide : aucune.
    allowed_tools: Vec<String>,
}

impl SkillRun {
    pub fn new(
        skill: &str,
        arguments: &str,
        model: Option<&str>,
        effort: Option<&str>,
        max_turns: u32,
        allowed_tools: &[String],
    ) -> Result<Self, SkillRunError> {
        let skill = skill.trim().trim_start_matches('/');
        if !is_skill_name(skill) {
            return Err(SkillRunError::InvalidSkill);
        }
        let arguments = arguments.trim();
        if arguments.chars().count() > ARGUMENTS_MAX || arguments.chars().any(char::is_control) {
            return Err(SkillRunError::InvalidArguments);
        }
        let token = |raw: Option<&str>, error: fn(String) -> SkillRunError| {
            match raw.map(str::trim).filter(|v| !v.is_empty()) {
                None => Ok(None),
                Some(v) if is_cli_token(v) => Ok(Some(v.to_owned())),
                Some(v) => Err(error(v.to_owned())),
            }
        };
        let model = token(model, SkillRunError::InvalidModel)?;
        let effort = token(effort, SkillRunError::InvalidEffort)?;
        if !(1..=MAX_TURNS_LIMIT).contains(&max_turns) {
            return Err(SkillRunError::InvalidMaxTurns);
        }
        let rules: Vec<String> =
            allowed_tools.iter().map(|r| r.trim()).filter(|r| !r.is_empty()).map(str::to_owned).collect();
        if rules.len() > RULES_MAX {
            return Err(SkillRunError::TooManyRules);
        }
        for rule in &rules {
            // Une règle qui commence par `-` serait lue comme une option de `claude`.
            if rule.starts_with('-') || rule.chars().count() > RULE_MAX || rule.chars().any(char::is_control) {
                return Err(SkillRunError::InvalidRule(rule.clone()));
            }
        }
        Ok(Self {
            skill: skill.to_owned(),
            arguments: arguments.to_owned(),
            model,
            effort,
            max_turns,
            allowed_tools: rules,
        })
    }

    pub fn skill(&self) -> &str {
        &self.skill
    }

    pub fn arguments(&self) -> &str {
        &self.arguments
    }

    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    pub fn effort(&self) -> Option<&str> {
        self.effort.as_deref()
    }

    pub fn max_turns(&self) -> u32 {
        self.max_turns
    }

    pub fn allowed_tools(&self) -> &[String] {
        &self.allowed_tools
    }

    /// Le prompt passé à `claude -p` : `/skill arguments`.
    pub fn prompt(&self) -> String {
        if self.arguments.is_empty() {
            format!("/{}", self.skill)
        } else {
            format!("/{} {}", self.skill, self.arguments)
        }
    }

    /// `ProgramArguments` complet, `claude` en chemin absolu en tête.
    pub fn program(&self, claude: &str) -> Vec<String> {
        let mut args = vec![claude.to_owned(), "-p".into(), self.prompt(), "--max-turns".into(), self.max_turns.to_string()];
        if let Some(model) = &self.model {
            args.extend(["--model".into(), model.clone()]);
        }
        if let Some(effort) = &self.effort {
            args.extend(["--effort".into(), effort.clone()]);
        }
        if !self.allowed_tools.is_empty() {
            args.push("--allowedTools".into());
            args.extend(self.allowed_tools.iter().cloned());
        }
        args
    }

    /// Le skill d'une commande qui a exactement la forme que Robiade écrit ; `None` sinon, et la
    /// routine reste une routine ordinaire. Un flag de plus (`--dangerously-skip-permissions`,
    /// `--permission-mode`…) suffit à ne pas la reconnaître.
    pub fn parse(program: &[String]) -> Option<Self> {
        let claude = program.first()?;
        if !claude.starts_with('/') || claude.rsplit('/').next() != Some("claude") {
            return None;
        }
        let (flag, prompt) = (program.get(1)?, program.get(2)?);
        if flag != "-p" {
            return None;
        }
        let invocation = prompt.strip_prefix('/')?;
        let (skill, arguments) = invocation.split_once(' ').unwrap_or((invocation, ""));
        let mut rest = program[3..].iter().map(String::as_str);
        if rest.next()? != "--max-turns" {
            return None;
        }
        let max_turns: u32 = rest.next()?.parse().ok()?;
        let (mut model, mut effort, mut rules) = (None, None, Vec::new());
        let mut next = rest.next();
        if next == Some("--model") {
            model = Some(rest.next()?);
            next = rest.next();
        }
        if next == Some("--effort") {
            effort = Some(rest.next()?);
            next = rest.next();
        }
        if next == Some("--allowedTools") {
            rules = rest.map(str::to_owned).collect();
            if rules.is_empty() {
                return None;
            }
        } else if next.is_some() {
            return None;
        }
        let run = Self::new(skill, arguments, model, effort, max_turns, &rules).ok()?;
        // Relu tel qu'il serait réécrit, ou pas reconnu : une espace en trop, une règle vide…
        (run.program(claude) == program).then_some(run)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE: &str = "/Users/me/.local/bin/claude";

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    fn audit() -> SkillRun {
        SkillRun::new("audit", "projet-a", Some("sonnet"), Some("high"), 30, &strings(&["Read", "Bash(gh pr list:*)"])).unwrap()
    }

    #[test]
    fn a_skill_run_becomes_a_claude_print_command() {
        assert_eq!(
            audit().program(CLAUDE),
            strings(&[
                CLAUDE, "-p", "/audit projet-a", "--max-turns", "30", "--model", "sonnet", "--effort", "high",
                "--allowedTools", "Read", "Bash(gh pr list:*)",
            ])
        );
        let bare = SkillRun::new("/plugin:skill", "", None, None, 5, &[]).unwrap();
        assert_eq!(bare.program("/opt/claude"), strings(&["/opt/claude", "-p", "/plugin:skill", "--max-turns", "5"]));
    }

    #[test]
    fn what_is_written_is_recognised_back() {
        for run in [audit(), SkillRun::new("x", "", None, None, 1, &[]).unwrap(), SkillRun::new("x", "a b  c", None, Some("low"), 9, &[]).unwrap()] {
            assert_eq!(SkillRun::parse(&run.program(CLAUDE)), Some(run));
        }
    }

    #[test]
    fn any_other_command_stays_an_ordinary_routine() {
        let written = audit().program(CLAUDE);
        let mut bypass = written.clone();
        bypass.insert(3, "--dangerously-skip-permissions".into());
        let mut mode = written.clone();
        mode.extend(["--permission-mode".into(), "bypassPermissions".into()]);
        let mut reordered = audit().program(CLAUDE);
        reordered.swap(5, 7);
        reordered.swap(6, 8);
        let others = [
            strings(&["/usr/bin/python3", "/Users/me/briefing.py"]),
            strings(&["claude", "-p", "/audit", "--max-turns", "3"]),
            strings(&["/opt/not-claude", "-p", "/audit", "--max-turns", "3"]),
            strings(&[CLAUDE, "-p", "fais l'audit", "--max-turns", "3"]),
            strings(&[CLAUDE, "-p", "/audit"]),
            strings(&[CLAUDE, "-p", "/audit ", "--max-turns", "3"]),
            strings(&[CLAUDE, "-p", "/audit", "--max-turns", "0"]),
            strings(&[CLAUDE, "-p", "/audit", "--max-turns", "3", "--allowedTools"]),
            strings(&[CLAUDE, "-p", "/audit", "--max-turns", "3", "--allowedTools", "Read", "--model", "opus"]),
            bypass,
            mode,
            reordered,
        ];
        for program in others {
            assert_eq!(SkillRun::parse(&program), None, "{program:?}");
        }
    }

    #[test]
    fn inputs_are_bounded() {
        let run = |skill: &str, args: &str, model: Option<&str>, turns: u32, rules: &[&str]| {
            SkillRun::new(skill, args, model, None, turns, &strings(rules))
        };
        for bad in ["", "/", "-x", "a b", "a/b", "é", &"a".repeat(129)] {
            assert_eq!(run(bad, "", None, 3, &[]), Err(SkillRunError::InvalidSkill), "{bad:?}");
        }
        assert_eq!(run("x", "a\nb", None, 3, &[]), Err(SkillRunError::InvalidArguments));
        assert_eq!(run("x", "", Some("--settings"), 3, &[]), Err(SkillRunError::InvalidModel("--settings".into())));
        assert_eq!(run("x", "", None, 0, &[]), Err(SkillRunError::InvalidMaxTurns));
        assert_eq!(run("x", "", None, MAX_TURNS_LIMIT + 1, &[]), Err(SkillRunError::InvalidMaxTurns));
        assert_eq!(run("x", "", None, 3, &["--dangerously-skip-permissions"]), Err(SkillRunError::InvalidRule("--dangerously-skip-permissions".into())));
        let many: Vec<String> = (0..=RULES_MAX).map(|i| format!("Bash(t{i})")).collect();
        assert_eq!(SkillRun::new("x", "", None, None, 3, &many), Err(SkillRunError::TooManyRules));
        // Blancs et lignes vides du formulaire ne comptent pas.
        let tidy = run(" /x ", "  a  ", Some(" "), 3, &[" Read ", "", "  "]).unwrap();
        assert_eq!((tidy.skill(), tidy.arguments(), tidy.model(), tidy.allowed_tools()), ("x", "a", None, &strings(&["Read"])[..]));
    }
}
