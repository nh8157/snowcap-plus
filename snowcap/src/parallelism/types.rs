use std::fmt::Debug;

use thiserror::Error;

/// The absolute position of configurations in a vector
pub type ConfigId = usize;

/// 
#[derive(Error, Debug)]
pub enum ParallelError<T: Debug + 'static> {
    ///
    #[error("Dag Error: {0:?}")]
    DagError(#[from] DagError<T>),
    ///
    #[error("Encountered error during execution")]
    ExecutionFailed,
}

#[derive(Error, Debug)]
pub enum DagError<T: Debug> {
    #[error("Node {0:?} already exists in the DAG")]
    NodeAlreadyExists(T),
    #[error("Node {0:?} does not exist in the DAG")]
    NodeDoesNotExist(T),
    #[error("Found a cycle in the DAG")]
    DagHasCycle,
}