//! MR et issues sur l'API REST de GitLab, instance configurée uniquement, avec le token du trousseau.
//! Les écritures passent toutes par `execute`, `execute_on_issue` et `create_issue`, avec une action
//! déjà validée par le domaine.

use std::sync::Mutex;
use std::time::Duration;

use rayon::prelude::*;
use robiade_core::contributions::Contribution;
use robiade_core::forge::{
    is_project_path, is_username, readable, Approvals, Discussion, Forge, ForgeError, Issue, IssueRef, IssueState, MergeRequest,
    MrRef, MrState, Note, PipelineStatus, GITLAB_HOST,
};
use robiade_core::issue_actions::{IssuePlanned, NewIssue};
use robiade_core::merge_actions::Planned;
use robiade_core::Millis;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::json;

use crate::keychain;

/// Service du trousseau qui porte le token (le même que mes scripts).
const TOKEN_SERVICE: &str = "gitlab-api-token";
/// Une page suffit : au-delà de 100 MR ouvertes, la liste n'est plus un outil de suivi.
const PER_PAGE: &str = "100";
/// Pages d'événements lues au plus pour une plage : au-delà, la plage est trop large pour un appel.
const MAX_EVENT_PAGES: usize = 50;
/// Pages de discussions lues au plus : chaque note système en compte une, une MR active en a vite 100.
const MAX_DISCUSSION_PAGES: usize = 10;

pub struct GitLab {
    http: ureq::Agent,
    api: String,
    /// Lu au premier appel, oublié sur un 401 pour relire un token renouvelé.
    token: Mutex<Option<SecretString>>,
    username: Mutex<Option<String>>,
}

impl Default for GitLab {
    fn default() -> Self {
        let http = ureq::Agent::config_builder()
            .https_only(true)
            // Aucune redirection : le token, porté par un en-tête maison, la suivrait vers un autre hôte.
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(15)))
            .user_agent("robiade")
            .build()
            .into();
        Self { http, api: format!("https://{GITLAB_HOST}/api/v4"), token: Mutex::default(), username: Mutex::default() }
    }
}

impl GitLab {
    fn token(&self) -> Result<SecretString, ForgeError> {
        let mut cached = self.token.lock().unwrap_or_else(|e| e.into_inner());
        if cached.is_none() {
            *cached = keychain::generic_password(TOKEN_SERVICE);
        }
        cached.clone().ok_or(ForgeError::NoToken)
    }

    /// `GET` sur l'API ; `path_and_query` est construit ici, jamais repris tel quel d'une entrée.
    fn get<T: serde::de::DeserializeOwned>(&self, path_and_query: &str) -> Result<T, ForgeError> {
        let token = self.token()?;
        let response = self
            .http
            .get(format!("{}{path_and_query}", self.api))
            .header("PRIVATE-TOKEN", token.expose_secret())
            .call();
        let mut response = match response {
            Ok(r) => r,
            Err(ureq::Error::StatusCode(401)) => {
                *self.token.lock().unwrap_or_else(|e| e.into_inner()) = None;
                return Err(ForgeError::Unauthorized);
            }
            Err(ureq::Error::StatusCode(403 | 404)) => return Err(ForgeError::NotFound),
            Err(ureq::Error::StatusCode(code)) => return Err(ForgeError::Api(format!("statut HTTP {code}"))),
            Err(e) => return Err(ForgeError::Network(e.to_string())),
        };
        if response.status() != 200 {
            return Err(ForgeError::Api(format!("statut HTTP {}", response.status().as_u16())));
        }
        response.body_mut().read_json().map_err(|e| ForgeError::Api(e.to_string()))
    }

    /// `POST` ou `PUT` d'un corps JSON. Le message d'erreur de GitLab est remonté, tronqué.
    fn send(&self, put: bool, path: &str, body: serde_json::Value) -> Result<(), ForgeError> {
        self.request(put, path, body).map(drop)
    }

    /// Comme `send`, en gardant le corps de la réponse.
    fn request(&self, put: bool, path: &str, body: serde_json::Value) -> Result<String, ForgeError> {
        let token = self.token()?;
        let url = format!("{}{path}", self.api);
        let request = if put { self.http.put(url) } else { self.http.post(url) };
        let mut response = request
            .config()
            .http_status_as_error(false)
            .build()
            .header("PRIVATE-TOKEN", token.expose_secret())
            .send_json(&body)
            .map_err(|e| ForgeError::Network(e.to_string()))?;
        match response.status().as_u16() {
            200..=299 => response
                .body_mut()
                .with_config()
                .limit(1024 * 1024)
                .read_to_string()
                .map_err(|e| ForgeError::Api(e.to_string())),
            401 => {
                *self.token.lock().unwrap_or_else(|e| e.into_inner()) = None;
                Err(ForgeError::Unauthorized)
            }
            code => {
                let raw = response.body_mut().with_config().limit(64 * 1024).read_to_string().unwrap_or_default();
                Err(ForgeError::Api(format!("statut HTTP {code} : {}", gitlab_message(&raw))))
            }
        }
    }

    fn username(&self) -> Result<String, ForgeError> {
        if let Some(name) = self.username.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return Ok(name);
        }
        let user: ApiUser = self.get("/user")?;
        if !is_username(&user.username) {
            return Err(ForgeError::Api("nom d'utilisateur inattendu".into()));
        }
        *self.username.lock().unwrap_or_else(|e| e.into_inner()) = Some(user.username.clone());
        Ok(user.username)
    }

    fn list(&self, query: &str) -> Result<Vec<ApiMergeRequest>, ForgeError> {
        self.get(&format!("/merge_requests?state=opened&per_page={PER_PAGE}&{query}"))
    }

    fn single(&self, project: &str, iid: u64) -> Result<ApiMergeRequest, ForgeError> {
        self.get(&format!("/projects/{}/merge_requests/{iid}", project.replace('/', "%2F")))
    }

    /// Identifiant d'un utilisateur, que GitLab exige pour assigner.
    fn user_id(&self, username: &str) -> Result<u64, ForgeError> {
        if !is_username(username) {
            return Err(ForgeError::NotFound);
        }
        let found: Vec<ApiUserId> = self.get(&format!("/users?username={username}"))?;
        found
            .into_iter()
            .find(|u| u.username.eq_ignore_ascii_case(username))
            .map(|u| u.id)
            .ok_or_else(|| ForgeError::Api(format!("utilisateur GitLab inconnu : {username}")))
    }

    /// Fils d'échanges sous `item` (`/projects/…/issues/12`), toutes pages lues, sans notes système.
    fn discussions(&self, item: &str) -> Result<Vec<Discussion>, ForgeError> {
        let mut found = vec![];
        for page in 1..=MAX_DISCUSSION_PAGES {
            let listed: Vec<ApiDiscussion> = self.get(&format!("{item}/discussions?per_page={PER_PAGE}&page={page}"))?;
            let last = listed.len() < 100;
            found.extend(listed.into_iter().map(discussion_to_domain));
            if last {
                let mut found = readable(found);
                found.sort_by_key(|d| d.notes[0].created_at);
                return Ok(found);
            }
        }
        Err(ForgeError::Api(format!("plus de {MAX_DISCUSSION_PAGES} pages de discussions GitLab")))
    }
}

impl Forge for GitLab {
    fn me(&self) -> Result<String, ForgeError> {
        self.username()
    }

    fn execute(&self, mr: &MrRef, planned: &Planned) -> Result<(), ForgeError> {
        if !is_project_path(&mr.project) {
            return Err(ForgeError::NotFound);
        }
        let project = format!("/projects/{}", mr.project.replace('/', "%2F"));
        let this = format!("{project}/merge_requests/{}", mr.iid);
        match planned {
            Planned::Comment(body) => self.send(false, &format!("{this}/notes"), json!({ "body": body })),
            Planned::Approve => self.send(false, &format!("{this}/approve"), json!({})),
            Planned::Rebase => self.send(true, &format!("{this}/rebase"), json!({})),
            Planned::RetryPipeline { pipeline_id } => {
                self.send(false, &format!("{project}/pipelines/{pipeline_id}/retry"), json!({}))
            }
            Planned::Retitle(title) => self.send(true, &this, json!({ "title": title })),
            Planned::Merge { sha, squash } => {
                self.send(true, &format!("{this}/merge"), json!({ "sha": sha, "squash": squash }))
            }
        }
    }

    fn execute_on_issue(&self, issue: &IssueRef, planned: &IssuePlanned) -> Result<(), ForgeError> {
        if !is_project_path(&issue.project) {
            return Err(ForgeError::NotFound);
        }
        let this = format!("/projects/{}/issues/{}", issue.project.replace('/', "%2F"), issue.iid);
        let body = match planned {
            IssuePlanned::Close => json!({ "state_event": "close" }),
            IssuePlanned::Reopen => json!({ "state_event": "reopen" }),
            IssuePlanned::Assign { assignees, .. } => {
                let ids = assignees.iter().map(|a| self.user_id(a)).collect::<Result<Vec<_>, _>>()?;
                // `0` est la forme documentée pour retirer tous les assignés.
                json!({ "assignee_ids": if ids.is_empty() { vec![0] } else { ids } })
            }
            IssuePlanned::Relabel { add, remove } => {
                json!({ "add_labels": add.join(","), "remove_labels": remove.join(",") })
            }
        };
        self.send(true, &this, body)
    }

    fn create_issue(&self, new: &NewIssue) -> Result<IssueRef, ForgeError> {
        if !is_project_path(&new.project) {
            return Err(ForgeError::NotFound);
        }
        let assignee_ids = new.assignees.iter().map(|a| self.user_id(a)).collect::<Result<Vec<_>, _>>()?;
        let body = json!({
            "title": new.title,
            "description": new.description,
            "labels": new.labels.join(","),
            "assignee_ids": assignee_ids,
        });
        let raw = self.request(false, &format!("/projects/{}/issues", new.project.replace('/', "%2F")), body)?;
        let created: ApiIssue = serde_json::from_str(&raw).map_err(|e| ForgeError::Api(e.to_string()))?;
        IssueRef::from_url(&created.web_url).ok_or_else(|| ForgeError::Api("issue hors de l'instance configurée".into()))
    }

    fn my_merge_requests(&self) -> Result<Vec<MergeRequest>, ForgeError> {
        let listed = self.list("scope=created_by_me")?;
        // La liste ne donne ni le pipeline ni les approbations : chaque MR est relue, en parallèle.
        let mut mrs: Vec<MergeRequest> = listed
            .into_par_iter()
            .filter_map(|m| {
                let r = MrRef::from_url(&m.web_url)?;
                let detailed = self.single(&r.project, r.iid).ok();
                let mut mr = to_domain(detailed.unwrap_or(m))?;
                mr.approvals = self.approvals(&r).ok();
                Some(mr)
            })
            .collect();
        mrs.sort_by_key(|m| std::cmp::Reverse(m.updated_at));
        Ok(mrs)
    }

    fn review_requests(&self) -> Result<Vec<MergeRequest>, ForgeError> {
        let me = self.username()?;
        let listed =
            self.list(&format!("scope=all&wip=no&reviewer_username={me}&not%5Bapproved_by_usernames%5D%5B%5D={me}"))?;
        let mut mrs: Vec<MergeRequest> = listed.into_iter().filter_map(to_domain).filter(|m| m.author != me).collect();
        mrs.sort_by_key(|m| std::cmp::Reverse(m.updated_at));
        Ok(mrs)
    }

    fn merge_request(&self, mr: &MrRef) -> Result<MergeRequest, ForgeError> {
        let found = self.single(&mr.project, mr.iid)?;
        to_domain(found).ok_or_else(|| ForgeError::Api("MR hors de l'instance configurée".into()))
    }

    fn approvals(&self, mr: &MrRef) -> Result<Approvals, ForgeError> {
        if !is_project_path(&mr.project) {
            return Err(ForgeError::NotFound);
        }
        let path = format!("/projects/{}/merge_requests/{}/approvals", mr.project.replace('/', "%2F"), mr.iid);
        let a: ApiApprovals = self.get(&path)?;
        Ok(Approvals {
            required: a.approvals_required,
            left: a.approvals_left,
            by: a.approved_by.into_iter().map(|b| b.user.username).collect(),
        })
    }

    fn merge_request_discussions(&self, mr: &MrRef) -> Result<Vec<Discussion>, ForgeError> {
        if !is_project_path(&mr.project) {
            return Err(ForgeError::NotFound);
        }
        self.discussions(&format!("/projects/{}/merge_requests/{}", mr.project.replace('/', "%2F"), mr.iid))
    }

    fn issue_discussions(&self, issue: &IssueRef) -> Result<Vec<Discussion>, ForgeError> {
        if !is_project_path(&issue.project) {
            return Err(ForgeError::NotFound);
        }
        self.discussions(&format!("/projects/{}/issues/{}", issue.project.replace('/', "%2F"), issue.iid))
    }

    fn my_issues(&self) -> Result<Vec<Issue>, ForgeError> {
        Ok(sorted_issues(self.get(&format!("/issues?scope=assigned_to_me&state=opened&per_page={PER_PAGE}"))?))
    }

    fn issue(&self, issue: &IssueRef) -> Result<Issue, ForgeError> {
        if !is_project_path(&issue.project) {
            return Err(ForgeError::NotFound);
        }
        let path = format!("/projects/{}/issues/{}", issue.project.replace('/', "%2F"), issue.iid);
        let found: ApiIssue = self.get(&path)?;
        issue_to_domain(found).ok_or_else(|| ForgeError::Api("issue hors de l'instance configurée".into()))
    }

    fn my_contributions(&self, from: Millis, to: Millis) -> Result<Vec<Contribution>, ForgeError> {
        // `after` et `before` sont des dates exclusives, dans un fuseau que l'API ne précise pas :
        // un jour de marge de chaque côté, puis le tri exact sur l'horodatage de chaque événement.
        let date = |at: Millis| {
            chrono::DateTime::from_timestamp_millis(at)
                .map(|d| d.date_naive().format("%Y-%m-%d").to_string())
                .ok_or_else(|| ForgeError::Api("date hors limites".into()))
        };
        let (after, before) = (date(from - DAY_MS)?, date(to + DAY_MS)?);
        let mut found = vec![];
        for page in 1..=MAX_EVENT_PAGES {
            let events: Vec<ApiEvent> =
                self.get(&format!("/events?after={after}&before={before}&per_page={PER_PAGE}&page={page}"))?;
            let last = events.len() < 100;
            found.extend(events.into_iter().filter_map(contribution).filter(|c| (from..to).contains(&c.at)));
            if last {
                found.extend(self.my_merges(from, to)?);
                return Ok(found);
            }
        }
        Err(ForgeError::Api(format!("plus de {MAX_EVENT_PAGES} pages d'événements GitLab sur la période")))
    }

    fn project_issues(&self, project: &str) -> Result<Vec<Issue>, ForgeError> {
        if !is_project_path(project) {
            return Err(ForgeError::NotFound);
        }
        let path = format!("/projects/{}/issues?state=opened&per_page={PER_PAGE}", project.replace('/', "%2F"));
        Ok(sorted_issues(self.get(&path)?))
    }
}

fn sorted_issues(listed: Vec<ApiIssue>) -> Vec<Issue> {
    let mut issues: Vec<Issue> = listed.into_iter().filter_map(issue_to_domain).collect();
    issues.sort_by_key(|i| std::cmp::Reverse(i.updated_at));
    issues
}

#[derive(Deserialize)]
struct ApiIssue {
    title: String,
    state: String,
    #[serde(default)]
    description: Option<String>,
    web_url: String,
    updated_at: String,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    assignees: Vec<ApiUser>,
    #[serde(default)]
    due_date: Option<String>,
    #[serde(default)]
    merge_requests_count: u32,
}

/// `None` pour une issue hors de l'instance configurée ou dans un état inconnu.
fn issue_to_domain(i: ApiIssue) -> Option<Issue> {
    let state = match i.state.as_str() {
        "opened" => IssueState::Opened,
        "closed" => IssueState::Closed,
        _ => return None,
    };
    Some(Issue {
        issue: IssueRef::from_url(&i.web_url)?,
        title: i.title,
        state,
        description: i.description.unwrap_or_default(),
        labels: i.labels,
        assignees: i.assignees.into_iter().map(|a| a.username).collect(),
        updated_at: chrono::DateTime::parse_from_rfc3339(&i.updated_at).ok()?.timestamp_millis(),
        web_url: i.web_url,
        due_date: i.due_date,
        merge_requests: i.merge_requests_count,
    })
}

#[derive(Deserialize)]
struct ApiDiscussion {
    #[serde(default)]
    notes: Vec<ApiNote>,
}

#[derive(Deserialize)]
struct ApiNote {
    #[serde(default)]
    body: String,
    author: ApiUser,
    created_at: String,
    #[serde(default)]
    system: bool,
    #[serde(default)]
    resolvable: bool,
    /// `null` sur une note qui ne se résout pas (vérifié le 2026-11-03), pas absent.
    #[serde(default)]
    resolved: Option<bool>,
}

/// Un fil est résolu quand toutes ses notes résolvables le sont. Une note mal datée est écartée.
fn discussion_to_domain(d: ApiDiscussion) -> Discussion {
    let resolvable: Vec<bool> = d.notes.iter().filter(|n| n.resolvable).map(|n| n.resolved.unwrap_or(false)).collect();
    let resolved = (!resolvable.is_empty()).then(|| resolvable.iter().all(|r| *r));
    let notes = d
        .notes
        .into_iter()
        .filter_map(|n| {
            Some(Note {
                created_at: chrono::DateTime::parse_from_rfc3339(&n.created_at).ok()?.timestamp_millis(),
                author: n.author.username,
                body: n.body,
                system: n.system,
            })
        })
        .collect();
    Discussion { notes, resolved }
}

/// `message` d'une réponse d'erreur GitLab (texte, liste ou objet), ramené à une ligne courte.
fn gitlab_message(raw: &str) -> String {
    let message = match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(serde_json::Value::Object(o)) => match o.get("message").or_else(|| o.get("error")) {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
            None => raw.to_owned(),
        },
        _ => raw.to_owned(),
    };
    let line = message.split_whitespace().collect::<Vec<_>>().join(" ");
    line.chars().take(300).collect()
}

const DAY_MS: Millis = 86_400_000;

impl GitLab {
    /// Mes MR mergées dans `[from, to)`, qui que soit celui qui a mergé. Contrairement aux
    /// événements, le filtre de l'API est à l'instant près (vérifié le 2026-11-02).
    fn my_merges(&self, from: Millis, to: Millis) -> Result<Vec<Contribution>, ForgeError> {
        let instant = |at: Millis| {
            chrono::DateTime::from_timestamp_millis(at)
                .map(|d| d.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
                .ok_or_else(|| ForgeError::Api("date hors limites".into()))
        };
        let (after, before) = (instant(from)?, instant(to)?);
        let mut found = vec![];
        for page in 1..=MAX_EVENT_PAGES {
            let mrs: Vec<ApiMergedMr> = self.get(&format!(
                "/merge_requests?scope=created_by_me&state=merged&merged_after={after}&merged_before={before}\
                 &per_page={PER_PAGE}&page={page}"
            ))?;
            let last = mrs.len() < 100;
            found.extend(mrs.into_iter().filter_map(merge).filter(|c| (from..to).contains(&c.at)));
            if last {
                return Ok(found);
            }
        }
        Err(ForgeError::Api(format!("plus de {MAX_EVENT_PAGES} pages de MR mergées sur la période")))
    }
}

#[derive(Deserialize)]
struct ApiMergedMr {
    #[serde(default)]
    merged_at: Option<String>,
}

/// `None` pour une MR sans date de merge lisible.
fn merge(m: ApiMergedMr) -> Option<Contribution> {
    let at = chrono::DateTime::parse_from_rfc3339(m.merged_at.as_deref()?).ok()?.timestamp_millis();
    Some(Contribution::merged(at))
}

#[derive(Deserialize)]
struct ApiEvent {
    action_name: String,
    #[serde(default)]
    target_type: Option<String>,
    created_at: String,
    #[serde(default)]
    push_data: Option<ApiPushData>,
}

#[derive(Deserialize)]
struct ApiPushData {
    #[serde(default)]
    commit_count: Option<u32>,
}

/// `None` pour un événement qui n'est pas une contribution comptée, ou mal daté.
fn contribution(e: ApiEvent) -> Option<Contribution> {
    let at = chrono::DateTime::parse_from_rfc3339(&e.created_at).ok()?.timestamp_millis();
    let commits = e.push_data.and_then(|p| p.commit_count);
    Contribution::classify(&e.action_name, e.target_type.as_deref(), commits, at)
}

#[derive(Deserialize)]
struct ApiUser {
    username: String,
}

#[derive(Deserialize)]
struct ApiUserId {
    id: u64,
    username: String,
}

#[derive(Deserialize)]
struct ApiApprovals {
    #[serde(default)]
    approvals_required: u32,
    #[serde(default)]
    approvals_left: u32,
    #[serde(default)]
    approved_by: Vec<ApiApprover>,
}

#[derive(Deserialize)]
struct ApiApprover {
    user: ApiUser,
}

#[derive(Deserialize)]
struct ApiPipeline {
    id: u64,
    status: String,
}

#[derive(Deserialize)]
struct ApiMergeRequest {
    title: String,
    #[serde(default)]
    description: Option<String>,
    state: String,
    #[serde(default)]
    draft: bool,
    source_branch: String,
    target_branch: String,
    web_url: String,
    updated_at: String,
    #[serde(default)]
    sha: Option<String>,
    #[serde(default)]
    has_conflicts: bool,
    #[serde(default = "resolved")]
    blocking_discussions_resolved: bool,
    #[serde(default)]
    detailed_merge_status: Option<String>,
    author: ApiUser,
    #[serde(default)]
    head_pipeline: Option<ApiPipeline>,
}

fn resolved() -> bool {
    true
}

/// `None` pour une MR dont l'URL n'est pas sur l'instance configurée ou dont l'état est inconnu.
fn to_domain(m: ApiMergeRequest) -> Option<MergeRequest> {
    let state = match (m.state.as_str(), m.draft) {
        ("merged", _) => MrState::Merged,
        ("closed", _) => MrState::Closed,
        ("opened", true) => MrState::Draft,
        ("opened", false) => MrState::Opened,
        _ => return None,
    };
    Some(MergeRequest {
        mr: MrRef::from_url(&m.web_url)?,
        title: m.title,
        description: m.description.unwrap_or_default(),
        author: m.author.username,
        state,
        source_branch: m.source_branch,
        target_branch: m.target_branch,
        updated_at: chrono::DateTime::parse_from_rfc3339(&m.updated_at).ok()?.timestamp_millis(),
        web_url: m.web_url,
        has_conflicts: m.has_conflicts,
        unresolved_discussions: !m.blocking_discussions_resolved,
        merge_status: m.detailed_merge_status.unwrap_or_default(),
        pipeline: m.head_pipeline.as_ref().and_then(|p| PipelineStatus::parse(&p.status)),
        pipeline_id: m.head_pipeline.map(|p| p.id),
        sha: m.sha.unwrap_or_default(),
        approvals: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use robiade_core::contributions::ContributionKind;

    fn parse(json: &str) -> Option<MergeRequest> {
        to_domain(serde_json::from_str(json).unwrap())
    }

    #[test]
    fn reads_a_merge_request_with_its_pipeline() {
        let mr = parse(
            r#"{"iid":303,"title":"Ajoute le cache","description":"Voir **#12**","state":"opened","draft":true,"source_branch":"cache",
                "target_branch":"staging","web_url":"https://gitlab.example.com/a/b/-/merge_requests/303",
                "updated_at":"2026-10-26T13:43:40.062Z","has_conflicts":true,"blocking_discussions_resolved":false,
                "detailed_merge_status":"draft_status","author":{"username":"robinos33"},
                "head_pipeline":{"status":"failed","id":1}}"#,
        )
        .unwrap();
        assert_eq!(mr.mr, MrRef { project: "a/b".into(), iid: 303 });
        assert_eq!(mr.state, MrState::Draft);
        assert_eq!(mr.pipeline, Some(PipelineStatus::Failed));
        assert!(mr.has_conflicts && mr.unresolved_discussions);
        assert_eq!(mr.updated_at, 1_793_022_220_062);
        assert_eq!(mr.description, "Voir **#12**");
    }

    #[test]
    fn reads_an_issue_published_as_a_work_item() {
        let issue: ApiIssue = serde_json::from_str(
            r#"{"iid":5021,"title":"Cache","state":"closed",
                "web_url":"https://gitlab.example.com/shop/boutique/-/work_items/5021",
                "updated_at":"2026-10-26T13:43:40Z","labels":["category::tech"],"assignees":[{"username":"robinos33"}],
                "due_date":"2026-11-02","merge_requests_count":1}"#,
        )
        .unwrap();
        let issue = issue_to_domain(issue).unwrap();
        assert_eq!(issue.issue, IssueRef { project: "shop/boutique".into(), iid: 5021 });
        assert_eq!((issue.assignees, issue.due_date.as_deref()), (vec!["robinos33".to_owned()], Some("2026-11-02")));
        assert_eq!(issue.state, IssueState::Closed);
    }

    #[test]
    fn reads_a_push_and_skips_a_comment() {
        let event = |json: &str| contribution(serde_json::from_str(json).unwrap());
        let push = event(
            r#"{"action_name":"pushed new","target_type":null,"created_at":"2026-10-26T13:43:40.062Z",
                "push_data":{"commit_count":2,"action":"created","ref_type":"branch","ref":"cache"}}"#,
        )
        .unwrap();
        assert_eq!((push.kind, push.count, push.at), (ContributionKind::Commits, 2, 1_793_022_220_062));
        let comment = r#"{"action_name":"commented on","target_type":"DiffNote","created_at":"2026-10-26T13:43:40Z",
            "note":{"body":"ok"}}"#;
        assert_eq!(event(comment), None);
    }

    #[test]
    fn a_merged_merge_request_counts_at_its_merge() {
        let mr = |json: &str| merge(serde_json::from_str(json).unwrap());
        let merged = mr(r#"{"iid":2805,"merged_at":"2026-10-28T17:23:38.850Z","merged_by":{"username":"x"}}"#);
        assert_eq!(merged, Some(Contribution::merged(1_793_208_218_850)));
        assert_eq!(mr(r#"{"iid":1,"merged_at":null}"#), None);
    }

    #[test]
    fn a_thread_is_resolved_when_all_its_resolvable_notes_are() {
        let thread = |json: &str| discussion_to_domain(serde_json::from_str(json).unwrap());
        let note = |resolvable: bool, resolved: bool| {
            format!(
                r#"{{"body":"ok","author":{{"username":"a"}},"created_at":"2026-10-26T13:43:40Z","system":false,
                    "resolvable":{resolvable},"resolved":{resolved}}}"#
            )
        };
        let open = thread(&format!(r#"{{"notes":[{},{}]}}"#, note(true, true), note(true, false)));
        assert_eq!((open.notes.len(), open.resolved), (2, Some(false)));
        let done = thread(&format!(r#"{{"notes":[{},{}]}}"#, note(true, true), note(false, false)));
        assert_eq!(done.resolved, Some(true));
        // Forme réelle d'une note simple : `resolved` vaut `null`.
        let comment = thread(
            r#"{"individual_note":true,"notes":[{"body":"ok","author":{"username":"a"},
                "created_at":"2026-10-26T13:43:40Z","system":false,"resolvable":false,"resolved":null}]}"#,
        );
        assert_eq!((comment.resolved, comment.notes[0].created_at), (None, 1_793_022_220_000));
    }

    #[test]
    fn keeps_a_short_gitlab_error_message() {
        let stale = gitlab_message(r#"{"message":"SHA does not match HEAD of source branch"}"#);
        assert_eq!(stale, "SHA does not match HEAD of source branch");
        assert_eq!(gitlab_message(r#"{"message":{"title":["is too long"]}}"#), r#"{"title":["is too long"]}"#);
        assert_eq!(gitlab_message(&"x".repeat(1000)).len(), 300);
    }

    #[test]
    fn ignores_merge_requests_outside_the_configured_instance() {
        let elsewhere = r#"{"title":"x","state":"merged","source_branch":"a","target_branch":"b",
            "web_url":"https://gitlab.com/a/b/-/merge_requests/1","updated_at":"2026-10-26T13:43:40Z",
            "author":{"username":"x"}}"#;
        assert_eq!(parse(elsewhere), None);
    }
}
