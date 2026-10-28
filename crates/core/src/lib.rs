//! Domaine et cas d'usage de Robiade.
//!
//! Ce crate ne dépend d'aucune bibliothèque d'I/O : il ne lit ni fichier, ni réseau, ni horloge.
//! Les adapters l'alimentent par les ports déclarés dans chaque module.

pub mod agents;
pub mod assets;
pub mod forge;
pub mod history;
pub mod issue_actions;
pub mod issue_start;
pub mod merge_actions;
pub mod pricing;
pub mod project;
pub mod stats;
pub mod tasks;
pub mod vcs;

/// Instant en millisecondes depuis l'epoch Unix. Fourni par l'appelant : le domaine n'a pas d'horloge.
pub type Millis = i64;
