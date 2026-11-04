//! Forme des adresses de GitLab, pour une seule instance : URL web d'une MR ou d'une issue, et
//! remotes Git. Pur : aucun appel réseau, l'hôte est donné à la construction.

use robiade_core::forge::{is_project_path, ForgeUrls, IssueRef, MrRef};

pub struct GitLabUrls {
    /// `hôte[:port]` des URL web, en minuscules.
    authority: String,
}

impl GitLabUrls {
    /// `authority` : `hôte[:port]`, sans schéma ni chemin.
    pub fn new(authority: &str) -> Self {
        Self { authority: authority.to_ascii_lowercase() }
    }

    /// Hôte sans le port : un remote SSH passe souvent par un autre port que le web.
    fn host(&self) -> &str {
        self.authority.split_once(':').map_or(&self.authority, |(h, _port)| h)
    }

    /// `https://<instance>/<projet>/-/<genre>/<iid>[/…]` → projet et numéro, pour l'un des genres donnés.
    fn parse_item_url(&self, url: &str, kinds: &[&str]) -> Option<(String, u64)> {
        let (authority, rest) = url.strip_prefix("https://")?.split_once('/')?;
        if !authority.eq_ignore_ascii_case(&self.authority) {
            return None;
        }
        let (project, tail) = rest.split_once("/-/")?;
        let (kind, tail) = tail.split_once('/')?;
        let iid = tail.split(['/', '?', '#']).next()?;
        if !kinds.contains(&kind) || !is_project_path(project) || iid.is_empty() || !iid.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        Some((project.to_owned(), iid.parse().ok()?))
    }
}

impl ForgeUrls for GitLabUrls {
    /// `https://<instance>/<projet>/-/merge_requests/<iid>`, tel que Claude Code l'écrit dans le
    /// `pr-link` d'un transcript.
    fn merge_request(&self, url: &str) -> Option<MrRef> {
        let (project, iid) = self.parse_item_url(url, &["merge_requests"])?;
        Some(MrRef { project, iid })
    }

    /// GitLab 19 publie une issue en `/-/work_items/<iid>`, les liens plus anciens en
    /// `/-/issues/<iid>` : les deux désignent la même issue.
    fn issue(&self, url: &str) -> Option<IssueRef> {
        let (project, iid) = self.parse_item_url(url, &["issues", "work_items"])?;
        Some(IssueRef { project, iid })
    }

    fn issue_url(&self, issue: &IssueRef) -> Option<String> {
        Some(format!("https://{}/{}/-/issues/{}", self.authority, issue.project, issue.iid))
    }

    /// `https://<instance>` suivi d'un chemin, d'une requête ou d'un fragment, ou de rien. Des
    /// identifiants (`moi@`) ou un autre port changent l'autorité : refusés. Un caractère de
    /// contrôle ou une espace aussi : `open` ne doit recevoir qu'une URL d'un seul tenant.
    fn is_web_url(&self, url: &str) -> bool {
        let Some(rest) = url.strip_prefix("https://") else { return false };
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        authority.eq_ignore_ascii_case(&self.authority) && !url.chars().any(|c| c.is_control() || c.is_whitespace())
    }

    /// En SSH (`git@hôte:projet.git`, `ssh://git@hôte[:port]/projet.git`) ou en HTTPS. Des
    /// identifiants dans l'URL ne sont jamais repris.
    fn project_of_remote(&self, url: &str) -> Option<String> {
        let url = url.trim();
        let (authority, path) = if let Some(rest) = url.strip_prefix("https://").or_else(|| url.strip_prefix("ssh://")) {
            rest.split_once('/')?
        } else {
            // Forme scp : `utilisateur@hôte:chemin`.
            let (authority, path) = url.split_once(':')?;
            if authority.contains('/') {
                return None;
            }
            (authority, path)
        };
        let host = authority.rsplit('@').next()?;
        let host = host.split_once(':').map_or(host, |(h, _port)| h);
        if !host.eq_ignore_ascii_case(self.host()) {
            return None;
        }
        let path = path.trim_start_matches('/').trim_end_matches('/');
        let path = path.strip_suffix(".git").unwrap_or(path);
        is_project_path(path).then(|| path.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls() -> GitLabUrls {
        GitLabUrls::new("gitlab.example.com")
    }

    #[test]
    fn only_merge_requests_of_the_configured_instance_are_recognised() {
        let urls = urls();
        let mr = urls.merge_request("https://gitlab.example.com/a/b-c/d_e/-/merge_requests/42");
        assert_eq!(mr, Some(MrRef { project: "a/b-c/d_e".into(), iid: 42 }));
        let with_tail = urls.merge_request("https://gitlab.example.com/a/b/-/merge_requests/7/diffs#x");
        assert_eq!(with_tail.map(|m| m.iid), Some(7));
        assert_eq!(urls.merge_request("https://gitlab.com/a/b/-/merge_requests/1"), None);
        assert_eq!(urls.merge_request("https://gitlab.example.com.evil.io/a/-/merge_requests/1"), None);
        assert_eq!(urls.merge_request("https://gitlab.example.com/a/../b/-/merge_requests/1"), None);
        assert_eq!(urls.merge_request("https://gitlab.example.com/a%2Fb/-/merge_requests/1"), None);
        assert_eq!(urls.merge_request("https://gitlab.example.com/a/b/-/merge_requests/1x"), None);
        assert_eq!(urls.merge_request("https://gitlab.example.com/a/b/-/issues/1"), None);
    }

    #[test]
    fn an_issue_is_recognised_under_both_of_its_urls() {
        let urls = urls();
        let expected = Some(IssueRef { project: "shop/boutique".into(), iid: 5021 });
        let base = "https://gitlab.example.com/shop/boutique/-";
        assert_eq!(urls.issue(&format!("{base}/work_items/5021")), expected);
        assert_eq!(urls.issue(&format!("{base}/issues/5021")), expected);
        assert_eq!(urls.issue(&format!("{base}/merge_requests/1")), None);
        let issue = IssueRef { project: "shop/boutique".into(), iid: 12 };
        let url = urls.issue_url(&issue).unwrap();
        assert_eq!(url, "https://gitlab.example.com/shop/boutique/-/issues/12");
        assert_eq!(urls.issue(&url), Some(issue));
    }

    #[test]
    fn finds_the_gitlab_project_of_a_remote_without_its_credentials() {
        let urls = urls();
        let project = |url| urls.project_of_remote(url);
        let boutique = project("git@gitlab.example.com:shop/boutique.git");
        assert_eq!(boutique.as_deref(), Some("shop/boutique"));
        assert_eq!(project("https://gitlab.example.com/groupe/poc/zork.git").as_deref(), Some("groupe/poc/zork"));
        assert_eq!(project("ssh://git@gitlab.example.com:2222/a/b").as_deref(), Some("a/b"));
        assert_eq!(project("https://moi:glpat-x@gitlab.example.com/a/b.git").as_deref(), Some("a/b"));
        assert_eq!(project("git@gitlab.com:a/b.git"), None);
        assert_eq!(project("https://gitlab.example.com.evil.io/a/b.git"), None);
        assert_eq!(project("https://evil.io/x@gitlab.example.com/a/b.git"), None);
        assert_eq!(project("/Users/r/depot-local"), None);
    }

    #[test]
    fn only_web_pages_of_the_instance_are_opened() {
        let urls = urls();
        assert!(urls.is_web_url("https://gitlab.example.com/a/b/-/merge_requests/1"));
        assert!(urls.is_web_url("https://GITLAB.example.com"));
        assert!(urls.is_web_url("https://gitlab.example.com?x#y"));
        for bad in [
            "http://gitlab.example.com/a",
            "https://gitlab.example.com.evil.io/a",
            "https://moi@gitlab.example.com/a",
            "https://gitlab.example.com@evil.io/a",
            "https://gitlab.example.com:8443/a",
            "https://gitlab.example.com/a b",
            "https://gitlab.example.com/a\n",
            "file:///etc/passwd",
            "https://github.com/a/b",
        ] {
            assert!(!urls.is_web_url(bad), "{bad}");
        }
    }

    #[test]
    fn a_port_is_part_of_the_web_address_but_not_of_the_ssh_host() {
        let urls = GitLabUrls::new("Forge.Example.com:8443");
        assert!(urls.merge_request("https://forge.example.com:8443/a/b/-/merge_requests/1").is_some());
        assert_eq!(urls.merge_request("https://forge.example.com/a/b/-/merge_requests/1"), None);
        assert_eq!(urls.project_of_remote("git@forge.example.com:a/b.git").as_deref(), Some("a/b"));
    }
}
