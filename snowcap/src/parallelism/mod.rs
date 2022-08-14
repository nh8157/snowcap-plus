/// This module defines a parallel execution object
mod dag;
pub use dag::Dag;

mod dependency_builder;
pub use dependency_builder::DependencyBuilder;

mod types;
pub use types::{ParallelError, ConfigId};