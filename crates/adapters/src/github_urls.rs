//! Forme des adresses de GitHub, pour une seule instance (github.com ou GitHub Enterprise Server) :
//! URL web d'une pull request ou d'une issue, remotes Git, et adresse de l'API. Pur : aucun appel
//! réseau, l'hôte est donné à la construction.
//!
//! Le domaine parle de MR : une pull request en est une, son numéro tient lieu d'`iid`, et le projet
//! est `owner/repo`, sans sous-groupe.

use robiade_core::forge::{is_project_path, ForgeUrls, IssueRef, MrRef};

use crate::forge_urls::{is_web_url_of, remote_path};

const GITHUB_COM: &str = "github.com";

pub struct GitHubUrls {
    /// `hôte[:port]` des URL web, en minuscules.
    authority: String,
}

/// `owner/repo` : exactement deux segments, sans rien à encoder.
pub fn is_repo_path(path: &str) -> bool {
    is_project_path(path) && path.matches('/').count() == 1
}

impl GitHubUrls {
    /// `authority` : `hôte[:port]`, sans schéma ni chemin.
    pub fn new(authority: &str) -> Self {
        Self { authority: authority.to_ascii_lowercase() }
    }

    /// Racine de l'API REST : `api.github.com` pour github.com, `/api/v3` sur une instance Enterprise.
    pub fn rest_api(&self) -> String {
        if self.authority == GITHUB_COM {
            "https://api.github.com".to_owned()
        } else {
            format!("https://{}/api/v3", self.authority)
        }
    }

    /// Point d'entrée GraphQL, sur le même hôte que l'API REST.
    pub fn graphql_api(&self) -> String {
        if self.authority == GITHUB_COM {
            "https://api.github.com/graphql".to_owned()
        } else {
            format!("https://{}/api/graphql", self.authority)
        }
    }

    /// Hôte sans le port : un remote SSH passe souvent par un autre port que le web.
    fn host(&self) -> &str {
        self.authority.split_once(':').map_or(&self.authority, |(h, _port)| h)
    }

    /// `https://<instance>/<owner>/<repo>/<genre>/<n>[/…]` → dépôt et numéro.
    fn parse_item_url(&self, url: &str, kind: &str) -> Option<(String, u64)> {
        let (authority, rest) = url.strip_prefix("https://")?.split_once('/')?;
        if !authority.eq_ignore_ascii_case(&self.authority) {
            return None;
        }
        let mut parts = rest.splitn(4, '/');
        let (owner, repo, found, tail) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
        let number = tail.split(['/', '?', '#']).next()?;
        let project = format!("{owner}/{repo}");
        if found != kind || !is_repo_path(&project) || number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        Some((project, number.parse().ok()?))
    }
}

impl ForgeUrls for GitHubUrls {
    /// `https://<instance>/<owner>/<repo>/pull/<n>`, tel que Claude Code l'écrit dans le `pr-link`
    /// d'un transcript.
    fn merge_request(&self, url: &str) -> Option<MrRef> {
        let (project, iid) = self.parse_item_url(url, "pull")?;
        Some(MrRef { project, iid })
    }

    fn issue(&self, url: &str) -> Option<IssueRef> {
        let (project, iid) = self.parse_item_url(url, "issues")?;
        Some(IssueRef { project, iid })
    }

    fn issue_url(&self, issue: &IssueRef) -> Option<String> {
        is_repo_path(&issue.project).then(|| format!("https://{}/{}/issues/{}", self.authority, issue.project, issue.iid))
    }

    fn project_of_remote(&self, url: &str) -> Option<String> {
        let path = remote_path(self.host(), url)?;
        is_repo_path(path).then(|| path.to_owned())
    }

    fn is_web_url(&self, url: &str) -> bool {
        is_web_url_of(&self.authority, url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls() -> GitHubUrls {
        GitHubUrls::new("github.com")
    }

    #[test]
    fn only_pull_requests_of_the_instance_are_recognised() {
        let urls = urls();
        assert_eq!(urls.merge_request("https://github.com/acme/shop/pull/42"), Some(MrRef { project: "acme/shop".into(), iid: 42 }));
        assert_eq!(urls.merge_request("https://github.com/acme/shop/pull/7/files#diff").map(|m| m.iid), Some(7));
        assert_eq!(urls.merge_request("https://github.com/acme/shop.js/pull/1?x").map(|m| m.project), Some("acme/shop.js".into()));
        for bad in [
            "https://github.com/acme/shop/issues/1",
            "https://github.com/acme/shop/pulls/1",
            "https://github.com/acme/shop/pull/1x",
            "https://github.com/acme/shop/pull/",
            "https://github.com/acme/../pull/1",
            "https://github.com.evil.io/acme/shop/pull/1",
            "https://gitlab.example.com/acme/shop/-/merge_requests/1",
            "http://github.com/acme/shop/pull/1",
        ] {
            assert_eq!(urls.merge_request(bad), None, "{bad}");
        }
    }

    #[test]
    fn an_issue_url_reads_back_as_the_same_issue() {
        let urls = urls();
        let issue = IssueRef { project: "acme/shop".into(), iid: 12 };
        let url = urls.issue_url(&issue).unwrap();
        assert_eq!(url, "https://github.com/acme/shop/issues/12");
        assert_eq!(urls.issue(&url), Some(issue));
        assert_eq!(urls.issue("https://github.com/acme/shop/pull/12"), None);
        assert_eq!(urls.issue_url(&IssueRef { project: "a/b/c".into(), iid: 1 }), None, "pas de sous-groupe sur GitHub");
    }

    #[test]
    fn finds_the_repository_of_a_remote_without_its_credentials() {
        let urls = urls();
        let project = |url| urls.project_of_remote(url);
        assert_eq!(project("git@github.com:robinos33/robiade.git").as_deref(), Some("robinos33/robiade"));
        assert_eq!(project("https://github.com/robinos33/robiade").as_deref(), Some("robinos33/robiade"));
        assert_eq!(project("https://moi:ghp_x@github.com/a/b.git").as_deref(), Some("a/b"));
        assert_eq!(project("ssh://git@github.com:443/a/b.git").as_deref(), Some("a/b"));
        assert_eq!(project("https://github.com/a/b/c.git"), None, "trois segments : pas un dépôt GitHub");
        assert_eq!(project("git@gitlab.example.com:a/b.git"), None);
        assert_eq!(project("https://github.com.evil.io/a/b.git"), None);
    }

    #[test]
    fn the_api_of_github_com_is_on_its_own_host() {
        assert_eq!(urls().rest_api(), "https://api.github.com");
        assert_eq!(urls().graphql_api(), "https://api.github.com/graphql");
        let enterprise = GitHubUrls::new("GitHub.Example.com:8443");
        assert_eq!(enterprise.rest_api(), "https://github.example.com:8443/api/v3");
        assert_eq!(enterprise.graphql_api(), "https://github.example.com:8443/api/graphql");
        assert!(enterprise.is_web_url("https://github.example.com:8443/a/b/pull/1"));
        assert!(!enterprise.is_web_url("https://github.example.com/a/b/pull/1"));
        assert_eq!(enterprise.project_of_remote("git@github.example.com:a/b.git").as_deref(), Some("a/b"));
    }
}
