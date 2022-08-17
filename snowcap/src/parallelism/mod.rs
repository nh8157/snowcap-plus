/// This module defines a parallel execution object
mod dag;
pub use dag::Dag;

mod solution_builder;
pub use solution_builder::SolutionBuilder;

mod types;
pub use types::{ParallelError, DagError, ConfigId};