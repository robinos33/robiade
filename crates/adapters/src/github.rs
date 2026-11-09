//! Pull requests et issues sur GitHub (github.com ou Enterprise Server), instance réglée uniquement,
//! avec le jeton du trousseau (PAT, de préférence fine-grained). Les écritures passent toutes par
//! `execute`, `execute_on_issue` et `create_issue`, avec une action déjà validée par le domaine.
//!
//! Les pull requests se lisent en GraphQL : un seul appel donne l'état, le brouillon, les fils non
//! résolus, la synthèse des checks, la décision de review et les méthodes de merge du dépôt, que
//! l'API REST disperse en une demi-douzaine d'appels. Le reste passe par l'API REST.
//!
//! Correspondances avec le domaine : une pull request est une MR, son numéro tient lieu d'`iid`, le
//! projet est `owner/repo`. Le pipeline est la synthèse des checks et des statuts du dernier commit.
//! Un message de bot est une note système : il disparaît des fils, comme celles de GitLab.

use std::sync::Mutex;
use std::time::Duration;

use rayon::prelude::*;
use robiade_core::contributions::{Contribution, ContributionKind};
use robiade_core::forge::{
    is_username, readable, Approvals, Discussion, Forge, ForgeError, ForgeUrls, Issue, IssueRef, IssueState,
    MergeMethods, MergeRequest, MrRef, MrState, Note, PipelineStatus,
};
use robiade_core::issue_actions::{IssuePlanned, NewIssue};
use robiade_core::merge_actions::Planned;
use robiade_core::Millis;
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::forge_urls::encode_path;
use crate::github_urls::{is_repo_path, GitHubUrls};
use crate::keychain;

/// Version de l'API REST demandée : les réponses ne changent pas de forme sans qu'on la change.
const API_VERSION: &str = "2022-11-28";
/// Une page suffit pour une liste de suivi, comme pour GitLab.
const PER_PAGE: usize = 100;
/// Pages de commentaires lues au plus.
const MAX_PAGES: usize = 10;
/// GitHub ne sert que les 300 derniers événements d'un utilisateur, sur 90 jours : trois pages.
const MAX_EVENT_PAGES: usize = 3;
/// Résultats d'une recherche lus au plus : la recherche GitHub s'arrête à 1 000.
const MAX_SEARCH_PAGES: usize = 10;
/// Réponse plus grosse refusée : une liste de suivi n'atteint pas cette taille.
const BODY_LIMIT: u64 = 16 * 1024 * 1024;
const DAY_MS: Millis = 86_400_000;
/// Fenêtre des événements que GitHub sert.
const EVENTS_WINDOW_MS: Millis = 90 * DAY_MS;

/// Champs d'une pull request lus à chaque fois. Les listes imbriquées sont bornées : au-delà, ce
/// n'est plus une PR qu'on suit d'un coup d'œil.
const PR_FRAGMENT: &str = "
fragment Pr on PullRequest {
  id number title body url updatedAt isDraft state merged
  headRefName baseRefName headRefOid
  author { login }
  mergeable mergeStateStatus reviewDecision
  repository { squashMergeAllowed mergeCommitAllowed }
  reviewThreads(first: 50) { nodes { isResolved } }
  latestOpinionatedReviews(first: 50) { nodes { state author { login } } }
  baseRef { branchProtectionRule { requiredApprovingReviewCount } }
  commits(last: 1) { nodes { commit { statusCheckRollup { state contexts(first: 50) { nodes {
    __typename ... on CheckRun { conclusion checkSuite { workflowRun { databaseId } } }
  } } } } } }
}";

enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

pub struct GitHub {
    http: ureq::Agent,
    urls: GitHubUrls,
    /// Service du trousseau qui porte le jeton.
    token_service: String,
    /// Lu au premier appel, oublié sur un 401 pour relire un jeton renouvelé.
    token: Mutex<Option<SecretString>>,
    login: Mutex<Option<String>>,
}

impl GitHub {
    /// `authority` : `hôte[:port]` de l'instance web. Le jeton ne part que vers son API.
    pub fn new(authority: &str, token_service: &str) -> Self {
        let http = ureq::Agent::config_builder()
            .https_only(true)
            // Aucune redirection : le jeton, porté par `Authorization`, la suivrait vers un autre hôte.
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(15)))
            // GitHub refuse une requête sans User-Agent.
            .user_agent("robiade")
            .build()
            .into();
        Self {
            http,
            urls: GitHubUrls::new(authority),
            token_service: token_service.to_owned(),
            token: Mutex::default(),
            login: Mutex::default(),
        }
    }

    fn token(&self) -> Result<SecretString, ForgeError> {
        let mut cached = self.token.lock().unwrap_or_else(|e| e.into_inner());
        if cached.is_none() {
            *cached = keychain::generic_password(&self.token_service);
        }
        cached.clone().ok_or_else(|| ForgeError::NoToken(self.token_service.clone()))
    }

    /// Un appel ; `url` est construite ici, jamais reprise telle quelle d'une entrée. Le message
    /// d'erreur de GitHub est remonté, tronqué.
    fn call(&self, method: Method, url: &str, body: Option<&Value>) -> Result<String, ForgeError> {
        let token = self.token()?;
        let auth = format!("Bearer {}", token.expose_secret());
        let empty = json!({});
        let body = body.unwrap_or(&empty);
        macro_rules! prepared {
            ($request:expr) => {
                $request
                    .config()
                    .http_status_as_error(false)
                    .build()
                    .header("Authorization", auth.as_str())
                    .header("Accept", "application/vnd.github+json")
                    .header("X-GitHub-Api-Version", API_VERSION)
            };
        }
        let sent = match method {
            Method::Get => prepared!(self.http.get(url)).call(),
            Method::Delete => prepared!(self.http.delete(url)).call(),
            Method::Post => prepared!(self.http.post(url)).send_json(body),
            Method::Put => prepared!(self.http.put(url)).send_json(body),
            Method::Patch => prepared!(self.http.patch(url)).send_json(body),
        };
        let mut response = sent.map_err(|e| ForgeError::Network(e.to_string()))?;
        let status = response.status().as_u16();
        let text = response.body_mut().with_config().limit(BODY_LIMIT).read_to_string();
        match status {
            200..=299 => text.map_err(|e| ForgeError::Api(e.to_string())),
            401 => {
                *self.token.lock().unwrap_or_else(|e| e.into_inner()) = None;
                Err(ForgeError::Unauthorized)
            }
            404 => Err(ForgeError::NotFound),
            code => Err(ForgeError::Api(format!("statut HTTP {code} : {}", github_message(&text.unwrap_or_default())))),
        }
    }

    fn get<T: DeserializeOwned>(&self, path_and_query: &str) -> Result<T, ForgeError> {
        let raw = self.call(Method::Get, &format!("{}{path_and_query}", self.urls.rest_api()), None)?;
        serde_json::from_str(&raw).map_err(|e| ForgeError::Api(e.to_string()))
    }

    fn send(&self, method: Method, path: &str, body: Value) -> Result<String, ForgeError> {
        self.call(method, &format!("{}{path}", self.urls.rest_api()), Some(&body))
    }

    /// Toutes les pages d'une liste, `MAX_PAGES` au plus.
    fn pages<T: DeserializeOwned>(&self, path: &str) -> Result<Vec<T>, ForgeError> {
        let mut found = vec![];
        for page in 1..=MAX_PAGES {
            let listed: Vec<T> = self.get(&format!("{path}?per_page={PER_PAGE}&page={page}"))?;
            let last = listed.len() < PER_PAGE;
            found.extend(listed);
            if last {
                return Ok(found);
            }
        }
        Err(ForgeError::Api(format!("plus de {MAX_PAGES} pages sur GitHub")))
    }

    /// Une requête GraphQL. Une erreur partielle (un champ illisible avec ce jeton) laisse le champ
    /// à `null` sans faire échouer le tout : c'est à l'appelant de décider quoi en conclure.
    fn query<T: DeserializeOwned>(&self, query: &str, variables: Value) -> Result<(Option<T>, Vec<GqlError>), ForgeError> {
        let body = json!({ "query": query, "variables": variables });
        let raw = self.call(Method::Post, &self.urls.graphql_api(), Some(&body))?;
        let response: GqlResponse<T> = serde_json::from_str(&raw).map_err(|e| ForgeError::Api(e.to_string()))?;
        Ok((response.data, response.errors.unwrap_or_default()))
    }

    /// Une mutation : la moindre erreur la fait échouer.
    fn mutate(&self, mutation: &str, id: &str) -> Result<(), ForgeError> {
        let (_, errors) = self.query::<Value>(mutation, json!({ "id": id }))?;
        match errors.first() {
            Some(e) => Err(ForgeError::Api(github_message(&e.message))),
            None => Ok(()),
        }
    }

    fn me_login(&self) -> Result<String, ForgeError> {
        if let Some(login) = self.login.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return Ok(login);
        }
        let user: RestUser = self.get("/user")?;
        if !is_username(&user.login) {
            return Err(ForgeError::Api("nom d'utilisateur inattendu".into()));
        }
        *self.login.lock().unwrap_or_else(|e| e.into_inner()) = Some(user.login.clone());
        Ok(user.login)
    }

    /// Pull requests d'une recherche, avec ce que le domaine en lit.
    fn search_prs(&self, q: &str) -> Result<Vec<(MergeRequest, ReviewFacts)>, ForgeError> {
        let query = format!(
            "query($q: String!) {{ search(query: $q, type: ISSUE, first: {PER_PAGE}) {{ nodes {{ ... on PullRequest {{ ...Pr }} }} }} }}{PR_FRAGMENT}"
        );
        let (data, errors) = self.query::<GqlSearch<Value>>(&query, json!({ "q": q }))?;
        let data = data.ok_or_else(|| graphql_failure(&errors))?;
        let mut found: Vec<_> = data
            .search
            .nodes
            .into_iter()
            .flatten()
            .filter_map(|node| serde_json::from_value::<GqlPr>(node).ok())
            .filter_map(|pr| pr_to_domain(pr, &self.urls))
            .collect();
        found.sort_by_key(|(m, _)| std::cmp::Reverse(m.updated_at));
        Ok(found)
    }

    fn pull_request(&self, mr: &MrRef) -> Result<(MergeRequest, ReviewFacts, String), ForgeError> {
        let (owner, name) = repo_parts(&mr.project)?;
        let query = format!(
            "query($owner: String!, $name: String!, $number: Int!) {{ repository(owner: $owner, name: $name) {{ pullRequest(number: $number) {{ ...Pr }} }} }}{PR_FRAGMENT}"
        );
        let (data, errors) = self.query::<GqlRepository<GqlPullRequest>>(&query, json!({ "owner": owner, "name": name, "number": mr.iid }))?;
        let pr = data.and_then(|d| d.repository).and_then(|r| r.pull_request).ok_or_else(|| not_found_or(&errors))?;
        let id = pr.id.clone();
        let (found, facts) = pr_to_domain(pr, &self.urls).ok_or_else(|| ForgeError::Api("pull request hors de l'instance configurée".into()))?;
        Ok((found, facts, id))
    }

    /// Approbations requises par les rulesets de la branche cible ; `None` si aucune règle n'en
    /// exige, ou si elles sont illisibles.
    fn ruleset_reviews(&self, project: &str, branch: &str) -> Option<u32> {
        let rules: Vec<RestRule> = self.get(&format!("/repos/{project}/rules/branches/{}", encode_path(branch))).ok()?;
        rules
            .iter()
            .filter(|r| r.kind == "pull_request")
            .filter_map(|r| r.parameters.as_ref()?.get("required_approving_review_count")?.as_u64())
            .max()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n > 0)
    }

    fn approvals_for(&self, mr: &MergeRequest, facts: &ReviewFacts) -> Option<Approvals> {
        let ruleset = match facts.decision.as_deref() {
            None => None,
            Some(_) => self.ruleset_reviews(&mr.mr.project, &mr.target_branch),
        };
        approvals_of(facts, ruleset)
    }

    fn issue_comments(&self, project: &str, number: u64) -> Result<Vec<Discussion>, ForgeError> {
        let comments: Vec<RestComment> = self.pages(&format!("/repos/{project}/issues/{number}/comments"))?;
        Ok(comments.into_iter().filter_map(|c| Some(Discussion { notes: vec![c.into_note()?], resolved: None })).collect())
    }

    /// Mes PR mergées dans `[from, to)`, qui que soit celui qui a mergé, datées de leur merge.
    fn my_merges(&self, login: &str, from: Millis, to: Millis) -> Result<Vec<Contribution>, ForgeError> {
        let day = |at: Millis| {
            chrono::DateTime::from_timestamp_millis(at)
                .map(|d| d.date_naive().format("%Y-%m-%d").to_string())
                .ok_or_else(|| ForgeError::Api("date hors limites".into()))
        };
        // La recherche filtre au jour, dans un fuseau qu'elle ne précise pas : un jour de marge,
        // puis le tri exact sur l'instant du merge.
        let q = format!("is:pr is:merged author:{login} merged:{}..{}", day(from - DAY_MS)?, day(to + DAY_MS)?);
        let query = format!(
            "query($q: String!, $after: String) {{ search(query: $q, type: ISSUE, first: {PER_PAGE}, after: $after) {{
               pageInfo {{ hasNextPage endCursor }} nodes {{ ... on PullRequest {{ mergedAt }} }} }} }}"
        );
        let mut found = vec![];
        let mut after: Option<String> = None;
        for _ in 0..MAX_SEARCH_PAGES {
            let (data, errors) = self.query::<GqlSearch<GqlMerged>>(&query, json!({ "q": q, "after": after }))?;
            let search = data.ok_or_else(|| graphql_failure(&errors))?.search;
            found.extend(search.nodes.into_iter().flatten().filter_map(|n| merged_at(&n)).filter(|at| (from..to).contains(at)).map(Contribution::merged));
            match search.page_info {
                Some(GqlPageInfo { has_next_page: true, end_cursor: Some(cursor) }) => after = Some(cursor),
                _ => return Ok(found),
            }
        }
        Err(ForgeError::Api("plus de 1 000 pull requests mergées sur la période".into()))
    }
}

impl Forge for GitHub {
    fn me(&self) -> Result<String, ForgeError> {
        self.me_login()
    }

    fn my_merge_requests(&self) -> Result<Vec<MergeRequest>, ForgeError> {
        let me = self.me_login()?;
        let found = self.search_prs(&format!("is:pr is:open archived:false author:{me}"))?;
        // Les approbations demandent les règles de la branche cible : lues en parallèle.
        Ok(found
            .into_par_iter()
            .map(|(mut mr, facts)| {
                mr.approvals = self.approvals_for(&mr, &facts);
                mr
            })
            .collect())
    }

    /// Une demande de review disparaît dès que j'en rends une : une PR que j'ai approuvée n'y est plus.
    fn review_requests(&self) -> Result<Vec<MergeRequest>, ForgeError> {
        let me = self.me_login()?;
        let found = self.search_prs(&format!("is:pr is:open archived:false draft:false review-requested:{me} -author:{me}"))?;
        Ok(found.into_iter().map(|(mr, _)| mr).filter(|m| m.author != me).collect())
    }

    fn merge_request(&self, mr: &MrRef) -> Result<MergeRequest, ForgeError> {
        self.pull_request(mr).map(|(found, _, _)| found)
    }

    fn my_issues(&self) -> Result<Vec<Issue>, ForgeError> {
        let listed: Vec<RestIssue> = self.get(&format!("/issues?filter=assigned&state=open&per_page={PER_PAGE}"))?;
        Ok(sorted_issues(listed, &self.urls))
    }

    fn approvals(&self, mr: &MrRef) -> Result<Approvals, ForgeError> {
        let (found, facts, _) = self.pull_request(mr)?;
        // Illisibles avec ce jeton : inconnues, jamais « approuvée ».
        self.approvals_for(&found, &facts).ok_or_else(|| ForgeError::Api("règles d'approbation illisibles avec ce jeton".into()))
    }

    fn merge_request_discussions(&self, mr: &MrRef) -> Result<Vec<Discussion>, ForgeError> {
        repo_parts(&mr.project)?;
        let project = &mr.project;
        let mut found = self.issue_comments(project, mr.iid)?;
        let reviews: Vec<RestReview> = self.pages(&format!("/repos/{project}/pulls/{}/reviews", mr.iid))?;
        found.extend(reviews.into_iter().filter_map(|r| Some(Discussion { notes: vec![r.into_note()?], resolved: None })));
        let comments: Vec<RestReviewComment> = self.pages(&format!("/repos/{project}/pulls/{}/comments", mr.iid))?;
        found.extend(review_threads(comments));
        let mut found = readable(found);
        found.sort_by_key(|d| d.notes[0].created_at);
        Ok(found)
    }

    fn issue(&self, issue: &IssueRef) -> Result<Issue, ForgeError> {
        repo_parts(&issue.project)?;
        let found: RestIssue = self.get(&format!("/repos/{}/issues/{}", issue.project, issue.iid))?;
        // Le numéro d'une pull request se lit aussi comme une issue : ce n'en est pas une.
        issue_to_domain(found, &self.urls).ok_or(ForgeError::NotFound)
    }

    fn issue_discussions(&self, issue: &IssueRef) -> Result<Vec<Discussion>, ForgeError> {
        repo_parts(&issue.project)?;
        let mut found = readable(self.issue_comments(&issue.project, issue.iid)?);
        found.sort_by_key(|d| d.notes[0].created_at);
        Ok(found)
    }

    fn project_issues(&self, project: &str) -> Result<Vec<Issue>, ForgeError> {
        repo_parts(project)?;
        let listed: Vec<RestIssue> = self.get(&format!("/repos/{project}/issues?state=open&per_page={PER_PAGE}"))?;
        Ok(sorted_issues(listed, &self.urls))
    }

    fn default_branch(&self, project: &str) -> Result<String, ForgeError> {
        repo_parts(project)?;
        let found: RestRepository = self.get(&format!("/repos/{project}"))?;
        found.default_branch.ok_or_else(|| ForgeError::Api("dépôt sans branche par défaut".into()))
    }

    fn execute(&self, mr: &MrRef, planned: &Planned) -> Result<(), ForgeError> {
        repo_parts(&mr.project)?;
        let (project, number) = (&mr.project, mr.iid);
        let node_id = || self.pull_request(mr).map(|(_, _, id)| id);
        match planned {
            Planned::Comment(body) => {
                self.send(Method::Post, &format!("/repos/{project}/issues/{number}/comments"), json!({ "body": body })).map(drop)
            }
            Planned::Approve => {
                self.send(Method::Post, &format!("/repos/{project}/pulls/{number}/reviews"), json!({ "event": "APPROVE" })).map(drop)
            }
            // Rebase, jamais merge de la cible dans la branche : un wagon ne reçoit pas le contenu de ses voisins.
            Planned::Rebase => self.mutate(
                "mutation($id: ID!) { updatePullRequestBranch(input: { pullRequestId: $id, updateMethod: REBASE }) { clientMutationId } }",
                &node_id()?,
            ),
            Planned::RetryPipeline { pipeline_id } => self
                .send(Method::Post, &format!("/repos/{project}/actions/runs/{pipeline_id}/rerun-failed-jobs"), json!({}))
                .map(drop),
            Planned::MarkReady => self.mutate(
                "mutation($id: ID!) { markPullRequestReadyForReview(input: { pullRequestId: $id }) { clientMutationId } }",
                &node_id()?,
            ),
            Planned::MarkDraft => self.mutate(
                "mutation($id: ID!) { convertPullRequestToDraft(input: { pullRequestId: $id }) { clientMutationId } }",
                &node_id()?,
            ),
            Planned::Merge { sha, squash } => {
                let method = if *squash { "squash" } else { "merge" };
                let body = json!({ "sha": sha, "merge_method": method });
                self.send(Method::Put, &format!("/repos/{project}/pulls/{number}/merge"), body).map(drop)
            }
        }
    }

    fn execute_on_issue(&self, issue: &IssueRef, planned: &IssuePlanned) -> Result<(), ForgeError> {
        repo_parts(&issue.project)?;
        let this = format!("/repos/{}/issues/{}", issue.project, issue.iid);
        match planned {
            IssuePlanned::Close => self.send(Method::Patch, &this, json!({ "state": "closed", "state_reason": "completed" })).map(drop),
            IssuePlanned::Reopen => self.send(Method::Patch, &this, json!({ "state": "open" })).map(drop),
            // GitHub remplace la liste, comme GitLab : la liste finale part.
            IssuePlanned::Assign { assignees, .. } => self.send(Method::Patch, &this, json!({ "assignees": assignees })).map(drop),
            // Seul l'écart part : un label posé entre-temps par un autre n'est pas effacé.
            IssuePlanned::Relabel { add, remove } => {
                if !add.is_empty() {
                    self.send(Method::Post, &format!("{this}/labels"), json!({ "labels": add }))?;
                }
                for label in remove {
                    let url = format!("{}{this}/labels/{}", self.urls.rest_api(), encode_path(label));
                    match self.call(Method::Delete, &url, None) {
                        // Déjà retiré par un autre : le résultat voulu est là.
                        Ok(_) | Err(ForgeError::NotFound) => {}
                        Err(e) => return Err(e),
                    }
                }
                Ok(())
            }
        }
    }

    fn create_issue(&self, new: &NewIssue) -> Result<IssueRef, ForgeError> {
        repo_parts(&new.project)?;
        let body = json!({ "title": new.title, "body": new.description, "labels": new.labels, "assignees": new.assignees });
        let raw = self.send(Method::Post, &format!("/repos/{}/issues", new.project), body)?;
        let created: RestIssue = serde_json::from_str(&raw).map_err(|e| ForgeError::Api(e.to_string()))?;
        self.urls.issue(&created.html_url).ok_or_else(|| ForgeError::Api("issue hors de l'instance configurée".into()))
    }

    /// Commits, PR ouvertes et reviews viennent des événements : GitHub n'en sert que 300, sur
    /// 90 jours. Avant, la période est vide, et l'interface le dit. Les PR mergées viennent de la
    /// recherche, sans cette limite.
    fn my_contributions(&self, from: Millis, to: Millis) -> Result<Vec<Contribution>, ForgeError> {
        let me = self.me_login()?;
        let mut found = vec![];
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(Millis::MAX, |d| Millis::try_from(d.as_millis()).unwrap_or(Millis::MAX));
        // Une période entièrement hors de la fenêtre n'a aucun événement à lire.
        let pages = if to > now - EVENTS_WINDOW_MS { MAX_EVENT_PAGES } else { 0 };
        for page in 1..=pages {
            let events: Vec<RestEvent> = self.get(&format!("/users/{me}/events?per_page={PER_PAGE}&page={page}"))?;
            let last = events.len() < PER_PAGE;
            let oldest = events.iter().filter_map(|e| millis(&e.created_at)).min();
            found.extend(events.into_iter().filter_map(contribution).filter(|c| (from..to).contains(&c.at)));
            // Du plus récent au plus ancien : dès qu'une page passe avant `from`, la suite aussi.
            if last || oldest.is_some_and(|at| at < from) {
                break;
            }
        }
        found.extend(self.my_merges(&me, from, to)?);
        Ok(found)
    }
}

/// `owner` et `repo` d'un projet GitHub validé ; tout autre chemin est refusé avant l'appel.
fn repo_parts(project: &str) -> Result<(&str, &str), ForgeError> {
    if !is_repo_path(project) {
        return Err(ForgeError::NotFound);
    }
    project.split_once('/').ok_or(ForgeError::NotFound)
}

fn millis(rfc3339: &str) -> Option<Millis> {
    chrono::DateTime::parse_from_rfc3339(rfc3339).ok().map(|d| d.timestamp_millis())
}

/// `message` d'une réponse d'erreur GitHub, ramené à une ligne courte.
fn github_message(raw: &str) -> String {
    let message = match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(o)) => match o.get("message") {
            Some(Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
            None => raw.to_owned(),
        },
        _ => raw.to_owned(),
    };
    let line = message.split_whitespace().collect::<Vec<_>>().join(" ");
    line.chars().take(300).collect()
}

fn graphql_failure(errors: &[GqlError]) -> ForgeError {
    ForgeError::Api(errors.first().map_or_else(|| "réponse GraphQL vide".to_owned(), |e| github_message(&e.message)))
}

/// Objet absent d'une réponse : introuvable si GitHub le dit, sinon son erreur.
fn not_found_or(errors: &[GqlError]) -> ForgeError {
    if errors.is_empty() || errors.iter().any(|e| e.kind.as_deref() == Some("NOT_FOUND")) {
        ForgeError::NotFound
    } else {
        graphql_failure(errors)
    }
}

#[derive(Deserialize)]
struct GqlResponse<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Option<Vec<GqlError>>,
}

#[derive(Deserialize)]
struct GqlError {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    message: String,
}

#[derive(Deserialize)]
struct GqlSearch<T> {
    search: GqlConnection<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GqlConnection<T> {
    #[serde(default = "Vec::new")]
    nodes: Vec<Option<T>>,
    #[serde(default)]
    page_info: Option<GqlPageInfo>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GqlPageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[derive(Deserialize)]
struct GqlRepository<T> {
    repository: Option<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GqlPullRequest {
    pull_request: Option<GqlPr>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GqlMerged {
    #[serde(default)]
    merged_at: Option<String>,
}

fn merged_at(n: &GqlMerged) -> Option<Millis> {
    millis(n.merged_at.as_deref()?)
}

#[derive(Deserialize)]
struct GqlActor {
    login: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GqlMergeMethods {
    squash_merge_allowed: bool,
    merge_commit_allowed: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GqlThread {
    is_resolved: bool,
}

#[derive(Deserialize)]
struct GqlReview {
    state: String,
    author: Option<GqlActor>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GqlBaseRef {
    branch_protection_rule: Option<GqlProtection>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GqlProtection {
    required_approving_review_count: Option<u32>,
}

#[derive(Deserialize)]
struct GqlCommitNode {
    commit: GqlCommit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GqlCommit {
    status_check_rollup: Option<GqlRollup>,
}

#[derive(Deserialize)]
struct GqlRollup {
    state: String,
    contexts: GqlConnection<GqlContext>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GqlContext {
    #[serde(rename = "__typename")]
    typename: String,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    check_suite: Option<GqlCheckSuite>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GqlCheckSuite {
    workflow_run: Option<GqlWorkflowRun>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GqlWorkflowRun {
    database_id: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GqlPr {
    id: String,
    title: String,
    #[serde(default)]
    body: String,
    url: String,
    updated_at: String,
    is_draft: bool,
    state: String,
    merged: bool,
    head_ref_name: String,
    base_ref_name: String,
    head_ref_oid: String,
    author: Option<GqlActor>,
    mergeable: Option<String>,
    merge_state_status: Option<String>,
    review_decision: Option<String>,
    repository: GqlMergeMethods,
    review_threads: GqlConnection<GqlThread>,
    latest_opinionated_reviews: GqlConnection<GqlReview>,
    base_ref: Option<GqlBaseRef>,
    commits: GqlConnection<GqlCommitNode>,
}

/// Ce dont les approbations se déduisent : la décision de review de GitHub, qui a approuvé, et le
/// nombre requis par la protection classique de la branche s'il est lisible.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ReviewFacts {
    decision: Option<String>,
    approved_by: Vec<String>,
    classic_required: Option<u32>,
}

/// La décision de GitHub fait foi ; le nombre requis vient des règles lisibles. Sans décision,
/// aucune review n'est exigée. Exigée mais d'un nombre illisible : inconnu, jamais « approuvée ».
fn approvals_of(facts: &ReviewFacts, ruleset: Option<u32>) -> Option<Approvals> {
    let by = facts.approved_by.clone();
    let approved = u32::try_from(by.len()).unwrap_or(u32::MAX);
    let known = facts.classic_required.filter(|n| *n > 0).max(ruleset.filter(|n| *n > 0));
    match facts.decision.as_deref() {
        None => Some(Approvals { required: 0, left: 0, by }),
        Some("APPROVED") => Some(Approvals { required: known.unwrap_or(approved), left: 0, by }),
        Some("REVIEW_REQUIRED" | "CHANGES_REQUESTED") => {
            known.map(|required| Approvals { required, left: required.saturating_sub(approved).max(1), by })
        }
        Some(_) => None,
    }
}

/// Synthèse des checks et statuts du dernier commit, ramenée au pipeline du domaine.
fn pipeline_of(rollup: Option<&GqlRollup>) -> Option<PipelineStatus> {
    Some(match rollup?.state.as_str() {
        "PENDING" | "EXPECTED" => PipelineStatus::Running,
        "SUCCESS" => PipelineStatus::Success,
        "FAILURE" | "ERROR" => PipelineStatus::Failed,
        _ => return None,
    })
}

/// Workflow à relancer : celui d'un check en échec, sinon le premier lu.
fn workflow_run_of(rollup: Option<&GqlRollup>) -> Option<u64> {
    let runs: Vec<(bool, u64)> = rollup?
        .contexts
        .nodes
        .iter()
        .flatten()
        .filter(|c| c.typename == "CheckRun")
        .filter_map(|c| {
            let failed = matches!(c.conclusion.as_deref(), Some("FAILURE" | "TIMED_OUT" | "CANCELLED" | "STARTUP_FAILURE"));
            Some((failed, c.check_suite.as_ref()?.workflow_run.as_ref()?.database_id?))
        })
        .collect();
    runs.iter().find(|(failed, _)| *failed).or(runs.first()).map(|(_, id)| *id)
}

/// `mergeStateStatus` de GitHub traduit dans les valeurs de `detailed_merge_status` que l'interface
/// connaît déjà. Une PR bloquée dit pourquoi, dans l'ordre où il faudra la débloquer.
fn merge_status(
    open: bool,
    draft: bool,
    merge_state: Option<&str>,
    decision: Option<&str>,
    unresolved: bool,
    pipeline: Option<PipelineStatus>,
) -> &'static str {
    if !open {
        return "not_open";
    }
    if draft {
        return "draft_status";
    }
    match merge_state {
        // `UNSTABLE` : des checks non requis échouent, GitHub laisse merger.
        Some("CLEAN" | "HAS_HOOKS" | "UNSTABLE") => "mergeable",
        Some("DIRTY") => "conflict",
        Some("BEHIND") => "need_rebase",
        Some("DRAFT") => "draft_status",
        Some("BLOCKED") => match () {
            _ if decision == Some("CHANGES_REQUESTED") => "requested_changes",
            _ if decision == Some("REVIEW_REQUIRED") => "not_approved",
            _ if unresolved => "discussions_not_resolved",
            _ if pipeline == Some(PipelineStatus::Running) => "ci_still_running",
            _ if matches!(pipeline, Some(PipelineStatus::Failed | PipelineStatus::Canceled)) => "ci_must_pass",
            _ => "blocked_status",
        },
        // `UNKNOWN`, ou absent : GitHub calcule encore.
        _ => "checking",
    }
}

/// `None` pour une PR hors de l'instance, dans un état inconnu ou mal datée.
fn pr_to_domain(p: GqlPr, urls: &GitHubUrls) -> Option<(MergeRequest, ReviewFacts)> {
    let state = match (p.state.as_str(), p.merged, p.is_draft) {
        (_, true, _) => MrState::Merged,
        ("CLOSED", false, _) => MrState::Closed,
        ("OPEN", false, true) => MrState::Draft,
        ("OPEN", false, false) => MrState::Opened,
        _ => return None,
    };
    let rollup = p.commits.nodes.iter().flatten().last().and_then(|n| n.commit.status_check_rollup.as_ref());
    let pipeline = pipeline_of(rollup);
    let unresolved = p.review_threads.nodes.iter().flatten().any(|t| !t.is_resolved);
    let facts = ReviewFacts {
        decision: p.review_decision.clone(),
        approved_by: p
            .latest_opinionated_reviews
            .nodes
            .iter()
            .flatten()
            .filter(|r| r.state == "APPROVED")
            .filter_map(|r| Some(r.author.as_ref()?.login.clone()))
            .collect(),
        classic_required: p.base_ref.as_ref().and_then(|b| b.branch_protection_rule.as_ref()?.required_approving_review_count),
    };
    let status = merge_status(
        matches!(state, MrState::Opened | MrState::Draft),
        p.is_draft,
        p.merge_state_status.as_deref(),
        p.review_decision.as_deref(),
        unresolved,
        pipeline,
    );
    let mr = MergeRequest {
        mr: urls.merge_request(&p.url)?,
        title: p.title,
        description: p.body,
        // Compte supprimé : GitHub l'attribue à `ghost`.
        author: p.author.map_or_else(|| "ghost".to_owned(), |a| a.login),
        state,
        source_branch: p.head_ref_name,
        target_branch: p.base_ref_name,
        updated_at: millis(&p.updated_at)?,
        web_url: p.url,
        sha: p.head_ref_oid,
        has_conflicts: p.mergeable.as_deref() == Some("CONFLICTING"),
        unresolved_discussions: unresolved,
        merge_status: status.to_owned(),
        pipeline,
        pipeline_id: workflow_run_of(rollup),
        approvals: None,
        merge_methods: MergeMethods { squash: p.repository.squash_merge_allowed, merge_commit: p.repository.merge_commit_allowed },
    };
    Some((mr, facts))
}

#[derive(Deserialize)]
struct RestUser {
    login: String,
    #[serde(default, rename = "type")]
    kind: Option<String>,
}

impl RestUser {
    fn is_bot(&self) -> bool {
        self.kind.as_deref() == Some("Bot")
    }
}

#[derive(Deserialize)]
struct RestRepository {
    #[serde(default)]
    default_branch: Option<String>,
}

#[derive(Deserialize)]
struct RestRule {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    parameters: Option<Value>,
}

#[derive(Deserialize)]
struct RestLabel {
    name: String,
}

#[derive(Deserialize)]
struct RestIssue {
    title: String,
    state: String,
    #[serde(default)]
    body: Option<String>,
    html_url: String,
    updated_at: String,
    #[serde(default)]
    labels: Vec<RestLabel>,
    #[serde(default)]
    assignees: Vec<RestUser>,
    /// Présent sur une pull request, que l'API des issues liste aussi.
    #[serde(default)]
    pull_request: Option<Value>,
}

/// `None` pour une pull request, une issue hors de l'instance ou dans un état inconnu. GitHub n'a
/// ni échéance ni décompte de MR liées.
fn issue_to_domain(i: RestIssue, urls: &GitHubUrls) -> Option<Issue> {
    if i.pull_request.is_some() {
        return None;
    }
    let state = match i.state.as_str() {
        "open" => IssueState::Opened,
        "closed" => IssueState::Closed,
        _ => return None,
    };
    Some(Issue {
        issue: urls.issue(&i.html_url)?,
        title: i.title,
        state,
        description: i.body.unwrap_or_default(),
        labels: i.labels.into_iter().map(|l| l.name).collect(),
        assignees: i.assignees.into_iter().map(|a| a.login).collect(),
        updated_at: millis(&i.updated_at)?,
        web_url: i.html_url,
        due_date: None,
        merge_requests: 0,
    })
}

fn sorted_issues(listed: Vec<RestIssue>, urls: &GitHubUrls) -> Vec<Issue> {
    let mut issues: Vec<Issue> = listed.into_iter().filter_map(|i| issue_to_domain(i, urls)).collect();
    issues.sort_by_key(|i| std::cmp::Reverse(i.updated_at));
    issues
}

#[derive(Deserialize)]
struct RestComment {
    user: Option<RestUser>,
    #[serde(default)]
    body: Option<String>,
    created_at: String,
}

impl RestComment {
    fn into_note(self) -> Option<Note> {
        note(self.user, self.body.unwrap_or_default(), &self.created_at)
    }
}

/// Une review rendue avec un texte ; une approbation sans un mot ne fait pas un message.
#[derive(Deserialize)]
struct RestReview {
    user: Option<RestUser>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    submitted_at: Option<String>,
}

impl RestReview {
    fn into_note(self) -> Option<Note> {
        let body = self.body.filter(|b| !b.trim().is_empty())?;
        note(self.user, body, self.submitted_at.as_deref()?)
    }
}

#[derive(Deserialize)]
struct RestReviewComment {
    id: u64,
    #[serde(default)]
    in_reply_to_id: Option<u64>,
    user: Option<RestUser>,
    #[serde(default)]
    body: String,
    created_at: String,
}

/// Un message ; celui d'un bot est une note système. Mal daté : écarté.
fn note(user: Option<RestUser>, body: String, created_at: &str) -> Option<Note> {
    let system = user.as_ref().is_some_and(RestUser::is_bot);
    Some(Note { author: user.map_or_else(|| "ghost".to_owned(), |u| u.login), body, created_at: millis(created_at)?, system })
}

/// Commentaires de code regroupés en fils : une réponse pointe le premier commentaire de son fil.
/// L'API REST ne dit pas si un fil est résolu : `resolved` reste inconnu.
fn review_threads(comments: Vec<RestReviewComment>) -> Vec<Discussion> {
    let mut threads: Vec<(u64, Discussion)> = vec![];
    for c in comments {
        let root = c.in_reply_to_id.unwrap_or(c.id);
        let Some(n) = note(c.user, c.body, &c.created_at) else { continue };
        match threads.iter_mut().find(|(id, _)| *id == root) {
            Some((_, d)) => d.notes.push(n),
            None => threads.push((root, Discussion { notes: vec![n], resolved: None })),
        }
    }
    threads
        .into_iter()
        .map(|(_, mut d)| {
            d.notes.sort_by_key(|n| n.created_at);
            d
        })
        .collect()
}

#[derive(Deserialize)]
struct RestEvent {
    #[serde(rename = "type")]
    kind: String,
    created_at: String,
    #[serde(default)]
    payload: Value,
}

/// `None` pour un événement qui n'est pas une contribution comptée. Un push compte ses commits ;
/// sans décompte dans la charge utile, il en compte un, le minimum certain.
fn contribution(e: RestEvent) -> Option<Contribution> {
    let at = millis(&e.created_at)?;
    let payload = &e.payload;
    let (kind, count) = match e.kind.as_str() {
        "PushEvent" => {
            let size = payload.get("size").and_then(Value::as_u64);
            let listed = payload.get("commits").and_then(Value::as_array).map(|c| c.len() as u64);
            (ContributionKind::Commits, u32::try_from(size.or(listed).unwrap_or(1)).ok().filter(|c| *c > 0)?)
        }
        "PullRequestEvent" if payload.get("action").and_then(Value::as_str) == Some("opened") => (ContributionKind::MrOpened, 1),
        "PullRequestReviewEvent"
            if payload.pointer("/review/state").and_then(Value::as_str).is_some_and(|s| s.eq_ignore_ascii_case("approved")) =>
        {
            (ContributionKind::Review, 1)
        }
        _ => return None,
    };
    Some(Contribution { kind, at, count })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls() -> GitHubUrls {
        GitHubUrls::new("github.com")
    }

    fn pr_json(extra: &str) -> String {
        format!(
            r#"{{"id":"PR_kw1","number":42,"title":"Ajoute le cache","body":"Voir **#12**",
              "url":"https://github.com/acme/shop/pull/42","updatedAt":"2026-10-26T13:43:40Z",
              "isDraft":false,"state":"OPEN","merged":false,"headRefName":"cache","baseRefName":"main",
              "headRefOid":"0123456789abcdef","author":{{"login":"jdoe"}},"mergeable":"MERGEABLE",
              "repository":{{"squashMergeAllowed":true,"mergeCommitAllowed":false}},
              "reviewThreads":{{"nodes":[{{"isResolved":true}},{{"isResolved":false}}]}},
              "latestOpinionatedReviews":{{"nodes":[{{"state":"APPROVED","author":{{"login":"ana"}}}},
                {{"state":"CHANGES_REQUESTED","author":{{"login":"bob"}}}}]}},
              "baseRef":{{"branchProtectionRule":{{"requiredApprovingReviewCount":2}}}},
              "commits":{{"nodes":[{{"commit":{{"statusCheckRollup":{{"state":"FAILURE","contexts":{{"nodes":[
                {{"__typename":"StatusContext"}},
                {{"__typename":"CheckRun","conclusion":"SUCCESS","checkSuite":{{"workflowRun":{{"databaseId":7}}}}}},
                {{"__typename":"CheckRun","conclusion":"FAILURE","checkSuite":{{"workflowRun":{{"databaseId":9}}}}}}
              ]}}}}}}}}]}}{extra}}}"#
        )
    }

    fn parse(json: &str) -> (MergeRequest, ReviewFacts) {
        pr_to_domain(serde_json::from_str(json).unwrap(), &urls()).unwrap()
    }

    #[test]
    fn reads_a_pull_request_as_a_merge_request() {
        let (mr, facts) = parse(&pr_json(r#","mergeStateStatus":"BLOCKED","reviewDecision":"CHANGES_REQUESTED""#));
        assert_eq!(mr.mr, MrRef { project: "acme/shop".into(), iid: 42 });
        assert_eq!((mr.state, mr.author.as_str(), mr.target_branch.as_str()), (MrState::Opened, "jdoe", "main"));
        assert_eq!((mr.pipeline, mr.pipeline_id), (Some(PipelineStatus::Failed), Some(9)), "le workflow en échec est celui qu'on relance");
        assert!(mr.unresolved_discussions && !mr.has_conflicts);
        assert_eq!(mr.merge_status, "requested_changes");
        assert_eq!(mr.merge_methods, MergeMethods { squash: true, merge_commit: false });
        assert_eq!(mr.updated_at, 1_793_022_220_000);
        assert_eq!(facts.approved_by, vec!["ana".to_owned()]);
        assert_eq!(facts.classic_required, Some(2));
    }

    #[test]
    fn a_merged_or_draft_pull_request_keeps_its_state() {
        let merged = pr_json(r#","mergeStateStatus":"UNKNOWN","reviewDecision":null"#).replace(r#""merged":false"#, r#""merged":true"#);
        let merged = merged.replace(r#""state":"OPEN""#, r#""state":"MERGED""#);
        assert_eq!((parse(&merged).0.state, parse(&merged).0.merge_status.as_str()), (MrState::Merged, "not_open"));
        let draft = pr_json(r#","mergeStateStatus":"DRAFT","reviewDecision":null"#).replace(r#""isDraft":false"#, r#""isDraft":true"#);
        assert_eq!((parse(&draft).0.state, parse(&draft).0.merge_status.as_str()), (MrState::Draft, "draft_status"));
        let elsewhere = pr_json(r#","mergeStateStatus":"CLEAN""#).replace("https://github.com/", "https://github.example.com/");
        assert!(pr_to_domain(serde_json::from_str(&elsewhere).unwrap(), &urls()).is_none());
    }

    #[test]
    fn a_blocked_pull_request_says_what_blocks_it_first() {
        let status = |decision, unresolved, pipeline| merge_status(true, false, Some("BLOCKED"), decision, unresolved, pipeline);
        assert_eq!(status(Some("REVIEW_REQUIRED"), true, None), "not_approved");
        assert_eq!(status(Some("APPROVED"), true, None), "discussions_not_resolved");
        assert_eq!(status(Some("APPROVED"), false, Some(PipelineStatus::Running)), "ci_still_running");
        assert_eq!(status(None, false, Some(PipelineStatus::Failed)), "ci_must_pass");
        assert_eq!(status(None, false, Some(PipelineStatus::Success)), "blocked_status");
        assert_eq!(merge_status(true, false, Some("UNSTABLE"), None, false, Some(PipelineStatus::Failed)), "mergeable");
        assert_eq!(merge_status(true, false, Some("DIRTY"), None, false, None), "conflict");
        assert_eq!(merge_status(true, false, Some("BEHIND"), None, false, None), "need_rebase");
        assert_eq!(merge_status(true, false, None, None, false, None), "checking");
    }

    #[test]
    fn approvals_are_unknown_rather_than_granted_when_the_rules_are_unreadable() {
        let facts = |decision: Option<&str>, classic| ReviewFacts {
            decision: decision.map(str::to_owned),
            approved_by: vec!["ana".into()],
            classic_required: classic,
        };
        assert_eq!(approvals_of(&facts(None, None), None), Some(Approvals { required: 0, left: 0, by: vec!["ana".into()] }));
        assert_eq!(approvals_of(&facts(Some("REVIEW_REQUIRED"), None), None), None, "nombre requis illisible : inconnu");
        assert_eq!(approvals_of(&facts(Some("REVIEW_REQUIRED"), Some(0)), None), None);
        let left = approvals_of(&facts(Some("REVIEW_REQUIRED"), Some(1)), Some(3)).unwrap();
        assert_eq!((left.required, left.left), (3, 2), "la règle la plus exigeante l'emporte");
        let blocked = approvals_of(&facts(Some("CHANGES_REQUESTED"), Some(1)), None).unwrap();
        assert_eq!(blocked.left, 1, "des changements demandés bloquent même avec assez d'approbations");
        assert_eq!(approvals_of(&facts(Some("APPROVED"), None), None).map(|a| (a.required, a.left)), Some((1, 0)));
    }

    #[test]
    fn an_issue_listing_skips_pull_requests() {
        let listed: Vec<RestIssue> = serde_json::from_str(
            r#"[{"title":"Cache","state":"open","body":null,"html_url":"https://github.com/acme/shop/issues/5",
                 "updated_at":"2026-10-26T13:43:40Z","labels":[{"name":"bug"}],"assignees":[{"login":"jdoe"}]},
                {"title":"PR","state":"open","html_url":"https://github.com/acme/shop/pull/6",
                 "updated_at":"2026-10-27T13:43:40Z","pull_request":{"url":"x"}}]"#,
        )
        .unwrap();
        let issues = sorted_issues(listed, &urls());
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].issue, IssueRef { project: "acme/shop".into(), iid: 5 });
        assert_eq!((issues[0].labels.clone(), issues[0].assignees.clone()), (vec!["bug".to_owned()], vec!["jdoe".to_owned()]));
        assert_eq!((issues[0].due_date.clone(), issues[0].merge_requests), (None, 0));
    }

    #[test]
    fn review_comments_are_grouped_in_threads_and_bots_are_system_notes() {
        let comments: Vec<RestReviewComment> = serde_json::from_str(
            r#"[{"id":1,"user":{"login":"ana","type":"User"},"body":"pourquoi ?","created_at":"2026-10-26T10:00:00Z"},
                {"id":2,"user":{"login":"lint[bot]","type":"Bot"},"body":"style","created_at":"2026-10-26T09:00:00Z"},
                {"id":3,"in_reply_to_id":1,"user":{"login":"jdoe","type":"User"},"body":"parce que","created_at":"2026-10-26T11:00:00Z"}]"#,
        )
        .unwrap();
        let threads = readable(review_threads(comments));
        assert_eq!(threads.len(), 1, "le fil du bot disparaît");
        let authors: Vec<_> = threads[0].notes.iter().map(|n| n.author.as_str()).collect();
        assert_eq!((authors, threads[0].resolved), (vec!["ana", "jdoe"], None));
        let silent: RestReview = serde_json::from_str(r#"{"user":{"login":"ana"},"body":"","submitted_at":"2026-10-26T10:00:00Z","state":"APPROVED"}"#).unwrap();
        assert_eq!(silent.into_note(), None, "une approbation sans texte n'est pas un message");
    }

    #[test]
    fn events_count_pushes_opened_pull_requests_and_approvals() {
        let event = |json: &str| contribution(serde_json::from_str(json).unwrap());
        let push = event(r#"{"type":"PushEvent","created_at":"2026-10-26T13:43:40Z","payload":{"size":3,"commits":[{},{}]}}"#).unwrap();
        assert_eq!((push.kind, push.count, push.at), (ContributionKind::Commits, 3, 1_793_022_220_000));
        let bare = event(r#"{"type":"PushEvent","created_at":"2026-10-26T13:43:40Z","payload":{"ref":"refs/heads/main"}}"#).unwrap();
        assert_eq!(bare.count, 1, "sans décompte : le minimum certain");
        let opened = event(r#"{"type":"PullRequestEvent","created_at":"2026-10-26T13:43:40Z","payload":{"action":"opened"}}"#);
        assert_eq!(opened.map(|c| c.kind), Some(ContributionKind::MrOpened));
        let closed = event(r#"{"type":"PullRequestEvent","created_at":"2026-10-26T13:43:40Z","payload":{"action":"closed"}}"#);
        assert_eq!(closed, None);
        let review = event(r#"{"type":"PullRequestReviewEvent","created_at":"2026-10-26T13:43:40Z","payload":{"review":{"state":"approved"}}}"#);
        assert_eq!(review.map(|c| c.kind), Some(ContributionKind::Review));
        let comment = event(r#"{"type":"IssueCommentEvent","created_at":"2026-10-26T13:43:40Z","payload":{}}"#);
        assert_eq!(comment, None);
    }

    #[test]
    fn a_project_path_has_an_owner_and_a_repository() {
        assert_eq!(repo_parts("a/b/c"), Err(ForgeError::NotFound));
        assert_eq!(repo_parts("acme/shop"), Ok(("acme", "shop")));
    }

    #[test]
    fn keeps_a_short_github_error_message() {
        assert_eq!(github_message(r#"{"message":"Head branch was modified. Review and try the merge again."}"#), "Head branch was modified. Review and try the merge again.");
        assert_eq!(github_message(&"x".repeat(1000)).len(), 300);
    }
}
