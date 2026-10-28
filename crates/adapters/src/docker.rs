//! Docker par l'API du démon, sur son socket Unix (`bollard`) : rien n'est lu dans la sortie de
//! `docker ps`. Les appels restent synchrones pour l'application ; un petit runtime tokio les porte.

use std::path::PathBuf;

use bollard::models::{ContainerSummary, ContainerSummaryStateEnum, PortSummary};
use bollard::query_parameters::{
    ListContainersOptions, LogsOptions, RestartContainerOptions, StartContainerOptions, StopContainerOptions,
};
use bollard::{Docker, API_DEFAULT_VERSION};
use futures_util::StreamExt;
use robiade_core::containers::{
    Container, ContainerAction, ContainerError, ContainerId, ContainerState, Containers, LogSink, Port, StopLogs,
    COMPOSE_PROJECT, COMPOSE_SERVICE, COMPOSE_WORKING_DIR,
};

/// Délai d'une requête jusqu'aux en-têtes de la réponse ; un flux de logs dure ensuite sans limite.
const TIMEOUT_SECS: u64 = 30;

pub struct DockerEngine {
    runtime: tokio::runtime::Runtime,
    home: PathBuf,
}

impl DockerEngine {
    pub fn new(home: PathBuf) -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("docker")
            .enable_all()
            .build()?;
        Ok(Self { runtime, home })
    }

    /// Socket du démon, relu à chaque appel : Docker Desktop ou OrbStack peuvent démarrer après
    /// Robiade. `DOCKER_HOST` n'est pas suivi : une app lancée du Finder n'hérite pas du shell.
    fn connect(&self) -> Result<Docker, ContainerError> {
        let socket = [
            PathBuf::from("/var/run/docker.sock"),
            self.home.join(".docker/run/docker.sock"),
            self.home.join(".orbstack/run/docker.sock"),
        ]
        .into_iter()
        .find(|p| p.exists())
        .ok_or(ContainerError::Unavailable)?;
        Docker::connect_with_unix(&socket.to_string_lossy(), TIMEOUT_SECS, API_DEFAULT_VERSION)
            .map_err(|_| ContainerError::Unavailable)
    }
}

fn docker_error(e: bollard::errors::Error) -> ContainerError {
    use bollard::errors::Error;
    match e {
        Error::DockerResponseServerError { status_code: 404, .. } => ContainerError::NotFound,
        Error::DockerResponseServerError { message, .. } => ContainerError::Docker(message),
        Error::IOError { .. } | Error::HyperResponseError { .. } | Error::RequestTimeoutError => {
            ContainerError::Unavailable
        }
        other => ContainerError::Docker(other.to_string()),
    }
}

fn state_of(state: Option<ContainerSummaryStateEnum>) -> ContainerState {
    match state {
        Some(ContainerSummaryStateEnum::CREATED) => ContainerState::Created,
        Some(ContainerSummaryStateEnum::RUNNING) => ContainerState::Running,
        Some(ContainerSummaryStateEnum::PAUSED) => ContainerState::Paused,
        Some(ContainerSummaryStateEnum::RESTARTING) => ContainerState::Restarting,
        Some(ContainerSummaryStateEnum::EXITED) => ContainerState::Exited,
        Some(ContainerSummaryStateEnum::DEAD) => ContainerState::Dead,
        _ => ContainerState::Other,
    }
}

fn port_of(p: PortSummary) -> Port {
    let protocol = p.typ.map_or_else(|| "tcp".to_owned(), |t| t.to_string());
    Port { private: p.private_port, public: p.public_port, protocol }
}

/// `None` pour un conteneur sans identifiant lisible : on ne pourrait rien en faire.
fn to_domain(c: ContainerSummary) -> Option<Container> {
    let id = ContainerId::parse(c.id.as_deref()?).ok()?;
    let mut labels = c.labels.unwrap_or_default();
    // Docker préfixe chaque nom d'un `/`, hérité des liens entre conteneurs.
    let name = c.names.and_then(|n| n.into_iter().next()).map(|n| n.trim_start_matches('/').to_owned());
    Some(Container {
        name: name.unwrap_or_else(|| id.as_str()[..12].to_owned()),
        id,
        image: c.image.unwrap_or_default(),
        state: state_of(c.state),
        status: c.status.unwrap_or_default(),
        ports: Port::distinct(c.ports.unwrap_or_default().into_iter().map(port_of).collect()),
        compose_project: labels.remove(COMPOSE_PROJECT),
        service: labels.remove(COMPOSE_SERVICE),
        working_dir: labels.remove(COMPOSE_WORKING_DIR),
    })
}

impl Containers for DockerEngine {
    fn list(&self) -> Result<Vec<Container>, ContainerError> {
        let docker = self.connect()?;
        let options = ListContainersOptions { all: true, ..Default::default() };
        let found = self.runtime.block_on(docker.list_containers(Some(options))).map_err(docker_error)?;
        let mut containers: Vec<Container> = found.into_iter().filter_map(to_domain).collect();
        containers.sort_by(|a, b| (&a.compose_project, &a.name).cmp(&(&b.compose_project, &b.name)));
        Ok(containers)
    }

    fn run(&self, id: &ContainerId, action: ContainerAction) -> Result<(), ContainerError> {
        let docker = self.connect()?;
        let id = id.as_str();
        self.runtime
            .block_on(async {
                match action {
                    ContainerAction::Start => docker.start_container(id, None::<StartContainerOptions>).await,
                    ContainerAction::Stop => docker.stop_container(id, None::<StopContainerOptions>).await,
                    ContainerAction::Restart => docker.restart_container(id, None::<RestartContainerOptions>).await,
                }
            })
            .map_err(docker_error)
    }

    fn follow_logs(&self, id: &ContainerId, tail: u32, mut sink: LogSink) -> Result<StopLogs, ContainerError> {
        let docker = self.connect()?;
        let options =
            LogsOptions { follow: true, stdout: true, stderr: true, tail: tail.to_string(), ..Default::default() };
        let id = id.as_str().to_owned();
        let task = self.runtime.spawn(async move {
            let mut stream = docker.logs(&id, Some(options));
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(chunk) => sink(Some(chunk.as_ref())),
                    Err(e) => {
                        sink(Some(format!("\r\n[{}]\r\n", docker_error(e)).as_bytes()));
                        break;
                    }
                }
            }
            sink(None);
        });
        Ok(Box::new(move || task.abort()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bollard::models::PortSummaryTypeEnum;
    use std::collections::HashMap;

    #[test]
    fn reads_a_compose_container_with_its_published_ports_once() {
        let port = |public| PortSummary {
            ip: Some("0.0.0.0".into()),
            private_port: 5432,
            public_port: public,
            typ: Some(PortSummaryTypeEnum::TCP),
        };
        let labels = HashMap::from([
            (COMPOSE_PROJECT.to_owned(), "boutique".to_owned()),
            (COMPOSE_SERVICE.to_owned(), "postgres".to_owned()),
            (COMPOSE_WORKING_DIR.to_owned(), "/Users/r/PhpstormProjects/boutique".to_owned()),
        ]);
        let container = to_domain(ContainerSummary {
            id: Some("3f2a9c1b7d4e".repeat(5) + "abcd"),
            names: Some(vec!["/boutique-postgres".into()]),
            image: Some("postgres:16".into()),
            state: Some(ContainerSummaryStateEnum::RUNNING),
            status: Some("Up 3 hours".into()),
            ports: Some(vec![port(Some(8032)), port(Some(8032))]),
            labels: Some(labels),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(container.name, "boutique-postgres");
        assert_eq!(container.state, ContainerState::Running);
        assert_eq!(container.ports, vec![Port { private: 5432, public: Some(8032), protocol: "tcp".into() }]);
        assert_eq!(container.compose_project.as_deref(), Some("boutique"));
        assert_eq!(container.service.as_deref(), Some("postgres"));
        assert_eq!(container.working_dir.as_deref(), Some("/Users/r/PhpstormProjects/boutique"));
    }

    #[test]
    fn ignores_a_container_without_a_usable_id() {
        assert_eq!(to_domain(ContainerSummary { id: Some("../x".into()), ..Default::default() }), None);
        assert_eq!(to_domain(ContainerSummary::default()), None);
    }
}
