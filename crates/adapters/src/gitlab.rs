//! MR lues sur l'API REST de GitLab, instance configurée uniquement, avec le token du trousseau.
//! Lecture seule : aucune requête autre que `GET` ne part d'ici.

use std::sync::Mutex;
use std::time::Duration;

use rayon::prelude::*;
use robiade_core::forge::{Forge, ForgeError, MergeRequest, MrRef, MrState, PipelineStatus, GITLAB_HOST};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

use crate::keychain;

/// Service du trousseau qui porte le token (le même que mes scripts).
const TOKEN_SERVICE: &str = "gitlab-api-token";
/// Une page suffit : au-delà de 100 MR ouvertes, la liste n'est plus un outil de suivi.
const PER_PAGE: &str = "100";

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

    fn username(&self) -> Result<String, ForgeError> {
        if let Some(name) = self.username.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return Ok(name);
        }
        let user: ApiUser = self.get("/user")?;
        if !valid_username(&user.username) {
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
}

impl Forge for GitLab {
    fn my_merge_requests(&self) -> Result<Vec<MergeRequest>, ForgeError> {
        let listed = self.list("scope=created_by_me")?;
        // La liste ne donne pas le pipeline : chaque MR est relue, en parallèle.
        let mut mrs: Vec<MergeRequest> = listed
            .into_par_iter()
            .filter_map(|m| {
                let detailed = MrRef::from_url(&m.web_url).and_then(|r| self.single(&r.project, r.iid).ok());
                to_domain(detailed.unwrap_or(m))
            })
            .collect();
        mrs.sort_by_key(|m| std::cmp::Reverse(m.updated_at));
        Ok(mrs)
    }

    fn review_requests(&self) -> Result<Vec<MergeRequest>, ForgeError> {
        let me = self.username()?;
        let listed = self.list(&format!("scope=all&wip=no&reviewer_username={me}"))?;
        let mut mrs: Vec<MergeRequest> = listed.into_iter().filter_map(to_domain).filter(|m| m.author != me).collect();
        mrs.sort_by_key(|m| std::cmp::Reverse(m.updated_at));
        Ok(mrs)
    }

    fn merge_request(&self, mr: &MrRef) -> Result<MergeRequest, ForgeError> {
        let found = self.single(&mr.project, mr.iid)?;
        to_domain(found).ok_or_else(|| ForgeError::Api("MR hors de l'instance configurée".into()))
    }
}

fn valid_username(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

#[derive(Deserialize)]
struct ApiUser {
    username: String,
}

#[derive(Deserialize)]
struct ApiPipeline {
    status: String,
}

#[derive(Deserialize)]
struct ApiMergeRequest {
    title: String,
    state: String,
    #[serde(default)]
    draft: bool,
    source_branch: String,
    target_branch: String,
    web_url: String,
    updated_at: String,
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
        author: m.author.username,
        state,
        source_branch: m.source_branch,
        target_branch: m.target_branch,
        updated_at: chrono::DateTime::parse_from_rfc3339(&m.updated_at).ok()?.timestamp_millis(),
        web_url: m.web_url,
        has_conflicts: m.has_conflicts,
        unresolved_discussions: !m.blocking_discussions_resolved,
        merge_status: m.detailed_merge_status.unwrap_or_default(),
        pipeline: m.head_pipeline.and_then(|p| PipelineStatus::parse(&p.status)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Option<MergeRequest> {
        to_domain(serde_json::from_str(json).unwrap())
    }

    #[test]
    fn reads_a_merge_request_with_its_pipeline() {
        let mr = parse(
            r#"{"iid":303,"title":"Ajoute le cache","state":"opened","draft":true,"source_branch":"cache",
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
    }

    #[test]
    fn ignores_merge_requests_outside_the_configured_instance() {
        let elsewhere = r#"{"title":"x","state":"merged","source_branch":"a","target_branch":"b",
            "web_url":"https://gitlab.com/a/b/-/merge_requests/1","updated_at":"2026-10-26T13:43:40Z",
            "author":{"username":"x"}}"#;
        assert_eq!(parse(elsewhere), None);
    }
}
